//! Ghostty's side of the editor: the schema merge (ghostty-source.json + ghostty-overlay.json), the
//! line-based config format, value checks, validation through `ghostty +validate-config`, the
//! AppleScript reload, the default config path, themes and suggestion lists.

mod format;
mod process;
mod schema_merge;
mod themes;
mod values;

pub use format::GhosttyFormat;
pub use schema_merge::{ACTIONS_LIST, merge_schema};
pub use settings_core::SchemaDiagnostic;
pub use values::to_lines;

use process::{Output, PrivateTempFile, RunError, TIMEOUT};
use settings_core::{AppDefinition, FieldErrors, MergedSchema, Schema};
use std::io::Write;
use std::path::{Path, PathBuf};

const SOURCE_JSON: &str = include_str!("../../../../schema/ghostty/ghostty-source.json");
const OVERLAY_JSON: &str = include_str!("../../../../schema/ghostty/ghostty-overlay.json");

/// Name of the X11 color names list (see `AppDefinition::lists`).
pub const X11_COLORS_LIST: &str = "x11-colors";

// Both files are compiled in and covered by tests, so a failure here is a build defect.
static MERGED: MergedSchema =
    MergedSchema::new(|| merge_schema(SOURCE_JSON, OVERLAY_JSON).expect("the embedded Ghostty schema files are valid JSON"));

/// Ghostty's settings schema, merged from the checked-in source and overlay.
pub fn schema() -> Schema {
    MERGED.schema()
}

/// What the merge of the checked-in schema files reported (GHO001..GHO007).
pub fn schema_diagnostics() -> Vec<SchemaDiagnostic> {
    MERGED.diagnostics()
}

/// Ghostty's editor definition: line-based config, checked by Ghostty itself, reloaded via AppleScript.
pub fn app() -> AppDefinition {
    AppDefinition {
        name: "ghostty-settings",
        terminal: "Ghostty",
        schema: schema(),
        format: Box::new(GhosttyFormat),
        scopes: &[],
        default_config_path,
        after_save: reload,
        themes: Some(|config| themes::list(config, process::ghostty_binary().as_deref())),
        install_theme: None,
        validate: Some(validate),
        lists: Some(lists),
        // Entries such as `macos-icon-screen-color = #111,#222` or a keybind can contain commas.
        list_comma_separates: false,
    }
}

/// The Ghostty CLI this editor uses: $GHOSTTY_BIN, the macOS app bundles, then PATH.
pub fn ghostty_binary() -> Option<PathBuf> {
    process::ghostty_binary()
}

/// Matches Ghostty's `preferredDefaultFilePath`: reads `$XDG_CONFIG_HOME/ghostty/config[.ghostty]`
/// (usually `~/.config/ghostty/…`) and, on macOS, also
/// `~/Library/Application Support/com.mitchellh.ghostty/config[.ghostty]` (loaded later, so it
/// wins when non-empty). Zero-byte files are treated as missing, matching Ghostty's loader.
fn default_config_path() -> PathBuf {
    let home = std::env::home_dir().unwrap_or_default();
    let xdg_base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(xdg) => PathBuf::from(xdg),
        None => home.join(".config"),
    };
    let app_support = cfg!(target_os = "macos")
        .then(|| home.join("Library").join("Application Support").join("com.mitchellh.ghostty"));
    choose_config(&xdg_base.join("ghostty"), app_support.as_deref())
}

fn choose_config(xdg: &Path, app_support: Option<&Path>) -> PathBuf {
    app_support
        .and_then(existing_in)
        .or_else(|| existing_in(xdg))
        .unwrap_or_else(|| app_support.unwrap_or(xdg).join("config.ghostty"))
}

fn existing_in(dir: &Path) -> Option<PathBuf> {
    ["config.ghostty", "config"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|p| p.metadata().is_ok_and(|m| m.is_file() && m.len() > 0))
}

/// Suggestion lists from the Ghostty CLI: "actions" and "x11-colors". Empty without Ghostty.
fn lists(name: &str) -> Vec<String> {
    let args: &[&str] = match name {
        ACTIONS_LIST => &["+list-actions"],
        X11_COLORS_LIST => &["+list-colors", "--plain"],
        _ => return Vec::new(),
    };
    match process::ghostty_binary() {
        Some(bin) => list_lines(&bin, args),
        None => Vec::new(),
    }
}

fn list_lines(bin: &Path, args: &[&str]) -> Vec<String> {
    match process::run(bin, args, TIMEOUT) {
        Ok(out) => out
            .text
            .split('\n')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Shown when no Ghostty can be found to check a save.
pub const NO_GHOSTTY: &str = "Couldn't find Ghostty to check the config, so nothing was saved. Install Ghostty, or set GHOSTTY_BIN to the path of its `ghostty` program.";

/// Runs Ghostty's own parser on the candidate text. `None` when it passes. Every save must pass,
/// so a missing Ghostty blocks the save instead of skipping the check.
/// The candidate is written (privately, as a hidden file) next to the config, so relative
/// `config-file` includes resolve as they will for the real file.
fn validate(text: &str, schema: &Schema, config: &Path) -> Option<FieldErrors> {
    validate_found(process::ghostty_binary(), text, schema, config)
}

fn validate_found(bin: Option<PathBuf>, text: &str, schema: &Schema, config: &Path) -> Option<FieldErrors> {
    let Some(bin) = bin else {
        return Some(FieldErrors::from([(String::new(), NO_GHOSTTY.to_string())]));
    };
    let dir = config.parent().filter(|d| d.is_dir()).map_or_else(std::env::temp_dir, Path::to_path_buf);
    validate_with(&bin, text, schema, &dir, TIMEOUT)
}

fn validate_with(
    bin: &Path,
    text: &str,
    schema: &Schema,
    dir: &Path,
    timeout: std::time::Duration,
) -> Option<FieldErrors> {
    let general = |message: String| Some(FieldErrors::from([(String::new(), message)]));
    let temp =
        PrivateTempFile::create(dir, ".ghostty-settings-check", ".ghostty").and_then(|(temp, mut file)| {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            Ok(temp)
        });
    let temp = match temp {
        Ok(temp) => temp,
        Err(e) => {
            return general(format!(
                "Couldn't write a temporary file to check the config with Ghostty: {e}"
            ));
        }
    };
    let mut arg = std::ffi::OsString::from("--config-file=");
    arg.push(&temp.path);
    let result = process::run(
        bin,
        &[std::ffi::OsString::from("+validate-config"), arg],
        timeout,
    );
    drop(temp);
    match result {
        Ok(Output { success: true, .. }) => None,
        Ok(Output { text, .. }) => Some(map_validation_output(&text, schema)),
        Err(RunError::Start(e)) => general(format!(
            "Couldn't run Ghostty ({}) to check the config: {e}",
            bin.display()
        )),
        Err(RunError::TimedOut) => general(format!(
            "Ghostty didn't finish checking the config within {} seconds.",
            timeout.as_secs()
        )),
    }
}

/// Maps Ghostty's "file:line:key: message" lines to settings ("" for keys the schema doesn't know).
fn map_validation_output(output: &str, schema: &Schema) -> FieldErrors {
    let mut errors = FieldErrors::new();
    for line in output.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some((key, message)) = validation_line(line) {
            let key = if schema.find(key).is_some() { key } else { "" };
            errors.insert(key.to_string(), format!("Ghostty: {message}"));
        }
    }
    if errors.is_empty() {
        errors.insert(
            String::new(),
            format!("Ghostty rejected the config: {}", output.trim()),
        );
    }
    errors
}

/// The first `:<digits>:<key>: <message>` in the line (the path before it may contain colons).
fn validation_line(line: &str) -> Option<(&str, &str)> {
    line.match_indices(':').find_map(|(i, _)| {
        let rest = &line[i + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let rest = rest[digits..].strip_prefix(':').filter(|_| digits > 0)?;
        let key_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
            .count();
        if key_len == 0 {
            return None;
        }
        let (key, rest) = rest.split_at(key_len);
        let message = rest.strip_prefix(": ")?;
        (!message.is_empty()).then_some((key, message))
    })
}

const RELOAD_SCRIPT: &str = r#"if application "Ghostty" is running then
  tell application "Ghostty"
    if (count of windows) is 0 then return "no-window"
    perform action "reload_config" on focused terminal of selected tab of front window
  end tell
  return "reloaded"
end if
return "not-running""#;

const MANUAL_RELOAD_MACOS: &str = "Saved. Press ⌘⇧, in Ghostty to reload.";

/// Asks a running Ghostty to reload its config. Never launches Ghostty.
fn reload() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some("Saved. Press Ctrl+Shift+, in Ghostty to reload.".into());
    }
    let result = process::run(
        Path::new("/usr/bin/osascript"),
        &["-e", RELOAD_SCRIPT],
        TIMEOUT,
    );
    Some(reload_note(result.ok().as_ref()).into())
}

fn reload_note(output: Option<&Output>) -> &'static str {
    match output {
        Some(Output {
            success: true,
            text,
        }) if text.trim() == "reloaded" => "Saved and reloaded in Ghostty.",
        Some(Output {
            success: true,
            text,
        }) if text.trim() == "not-running" => "Saved. Ghostty will use it next time it starts.",
        _ => MANUAL_RELOAD_MACOS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn app_is_wired_up() {
        let app = app();
        assert_eq!(app.name, "ghostty-settings");
        assert_eq!(app.terminal, "Ghostty");
        assert!(app.scopes.is_empty());
        assert!(app.schema.scopes.is_empty());
        assert_eq!(app.schema.brand, "ghostty");
        assert!(!app.schema.theme_locks_colors);
        assert!(app.install_theme.is_none());
        assert!(app.validate.is_some() && app.lists.is_some() && app.themes.is_some());
        let path = (app.default_config_path)();
        assert!(path.ends_with("config.ghostty") || path.ends_with("config"));
        assert!((app.lists.unwrap())("no-such-list").is_empty());
    }

    #[test]
    fn default_config_checks_macos_application_support_and_xdg() {
        let dir = tempfile::tempdir().unwrap();
        let xdg = dir.path().join("xdg");
        let app_support = dir.path().join("app-support");
        fs::create_dir_all(&xdg).unwrap();
        fs::create_dir_all(&app_support).unwrap();

        // Nothing on disk yet: matches Ghostty's preferredDefaultFilePath (Application Support on
        // macOS, XDG on Linux).
        assert_eq!(choose_config(&xdg, Some(&app_support)), app_support.join("config.ghostty"));
        assert_eq!(choose_config(&xdg, None), xdg.join("config.ghostty"));

        // Zero-byte files are skipped like in Ghostty's loader.
        fs::write(xdg.join("config"), "font-size = 14\n").unwrap();
        fs::write(xdg.join("config.ghostty"), "").unwrap();
        fs::write(app_support.join("config.ghostty"), "").unwrap();
        assert_eq!(choose_config(&xdg, Some(&app_support)), xdg.join("config"));

        fs::write(xdg.join("config.ghostty"), "font-size = 15\n").unwrap();
        assert_eq!(choose_config(&xdg, Some(&app_support)), xdg.join("config.ghostty"));

        // Ghostty loads Application Support after XDG on macOS, so it wins when non-empty.
        fs::write(app_support.join("config"), "font-size = 16\n").unwrap();
        assert_eq!(choose_config(&xdg, Some(&app_support)), app_support.join("config"));
        assert_eq!(choose_config(&xdg, None), xdg.join("config.ghostty"));
        fs::write(app_support.join("config.ghostty"), "font-size = 17\n").unwrap();
        assert_eq!(choose_config(&xdg, Some(&app_support)), app_support.join("config.ghostty"));
    }

    #[test]
    fn validation_lines_map_to_settings() {
        let schema = schema();
        let errors = map_validation_output(
            "/tmp/x:y/ghostty-settings-1.ghostty:2:window-padding-x: invalid value \"nope\"\n\
             /tmp/a.ghostty:3:not-a-key: unknown field\n",
            &schema,
        );
        assert_eq!(
            errors["window-padding-x"],
            "Ghostty: invalid value \"nope\""
        );
        assert_eq!(errors[""], "Ghostty: unknown field");
        let errors = map_validation_output("  something odd\n", &schema);
        assert_eq!(errors[""], "Ghostty rejected the config: something odd");
    }

    #[test]
    fn validation_line_needs_line_number_key_and_message() {
        assert_eq!(
            validation_line("C:\\x:12:font-size: bad"),
            Some(("font-size", "bad"))
        );
        assert_eq!(validation_line("file::font-size: bad"), None);
        assert_eq!(validation_line("file:1:Font: bad"), None);
        assert_eq!(validation_line("file:1:font-size:bad"), None);
        assert_eq!(validation_line("file:1:font-size: "), None);
    }

    #[test]
    fn reload_notes() {
        let out = |success, text: &str| Output {
            success,
            text: text.into(),
        };
        assert_eq!(
            reload_note(Some(&out(true, "reloaded\n"))),
            "Saved and reloaded in Ghostty."
        );
        assert_eq!(
            reload_note(Some(&out(true, "not-running\n"))),
            "Saved. Ghostty will use it next time it starts."
        );
        assert_eq!(
            reload_note(Some(&out(true, "no-window\n"))),
            MANUAL_RELOAD_MACOS
        );
        assert_eq!(
            reload_note(Some(&out(false, "reloaded"))),
            MANUAL_RELOAD_MACOS
        );
        assert_eq!(reload_note(None), MANUAL_RELOAD_MACOS);
    }

    #[test]
    fn reload_script_checks_running_before_telling() {
        // `tell application` would launch Ghostty; the running check must guard it.
        let check = RELOAD_SCRIPT.find("is running then").unwrap();
        let tell = RELOAD_SCRIPT.find("tell application").unwrap();
        assert!(check < tell);
        assert!(!RELOAD_SCRIPT.contains("activate"));
        assert!(!RELOAD_SCRIPT.contains("launch"));
    }

    #[cfg(unix)]
    mod fake_ghostty {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        fn fake(dir: &Path, body: &str) -> PathBuf {
            let path = dir.join("ghostty");
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        #[test]
        fn validation_passes_the_candidate_in_a_temp_file() {
            let dir = tempfile::tempdir().unwrap();
            let seen = dir.path().join("seen");
            // Copy the file Ghostty was asked to check, then fail on it.
            let bin = fake(
                dir.path(),
                &format!(
                    "[ \"$1\" = +validate-config ] || exit 9\nf=\"${{2#--config-file=}}\"\ncp \"$f\" '{}'\necho \"$f:1:font-size: bad\"\nexit 1",
                    seen.display()
                ),
            );
            let errors = validate_with(&bin, "font-size = x\n", &schema(), &std::env::temp_dir(), TIMEOUT).unwrap();
            assert_eq!(errors["font-size"], "Ghostty: bad");
            assert_eq!(fs::read_to_string(&seen).unwrap(), "font-size = x\n");
        }

        #[test]
        fn passing_validation_is_none() {
            let dir = tempfile::tempdir().unwrap();
            let bin = fake(dir.path(), "exit 0");
            assert!(validate_with(&bin, "font-size = 13\n", &schema(), &std::env::temp_dir(), TIMEOUT).is_none());
        }

        #[test]
        fn a_hanging_or_broken_ghostty_is_a_general_error_not_a_pass() {
            let dir = tempfile::tempdir().unwrap();
            let bin = fake(dir.path(), "exec sleep 30");
            let errors = validate_with(&bin, "", &schema(), &std::env::temp_dir(), Duration::from_millis(200)).unwrap();
            assert!(errors[""].contains("didn't finish"), "{errors:?}");

            let not_executable = dir.path().join("plain");
            fs::write(&not_executable, "").unwrap();
            let errors = validate_with(&not_executable, "", &schema(), &std::env::temp_dir(), TIMEOUT).unwrap();
            assert!(errors[""].starts_with("Couldn't run Ghostty"), "{errors:?}");
        }

        #[test]
        fn lists_split_and_trim_lines() {
            let dir = tempfile::tempdir().unwrap();
            let bin = fake(dir.path(), "printf ' new_tab \\n\\nclose_surface\\n'");
            assert_eq!(
                list_lines(&bin, &["+list-actions"]),
                ["new_tab", "close_surface"]
            );
        }
    }

    /// The real Ghostty, when it's installed (skipped otherwise).
    mod real_ghostty {
        use super::*;

        fn bin() -> Option<PathBuf> {
            let bin = ghostty_binary();
            if bin.is_none() {
                eprintln!("skipped: Ghostty isn't installed");
            }
            bin
        }

        #[test]
        fn a_missing_ghostty_blocks_the_save() {
            let errors = validate_found(None, "font-size = 13\n", &schema(), Path::new("/tmp/config.ghostty")).expect("must not pass unchecked");
            assert_eq!(errors[""], NO_GHOSTTY);
        }

        #[test]
        fn relative_includes_resolve_next_to_the_config() {
            let Some(_) = bin() else { return };
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("extra.ghostty"), "font-size = 13\n").unwrap();
            let config = dir.path().join("config.ghostty");
            let text = "config-file = extra.ghostty\ncursor-style = bar\n";
            assert_eq!(validate(text, &schema(), &config), None, "the include is found next to the config");
            let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
            assert_eq!(left.len(), 1, "the check file is removed again: {left:?}");
        }

        #[test]
        fn ghostty_itself_validates_the_candidate_file() {
            let Some(bin) = bin() else { return };
            let schema = schema();
            assert_eq!(
                validate_with(
                    &bin,
                    "font-size = 13\ncursor-style = bar\n",
                    &schema,
                    &std::env::temp_dir(),
                    TIMEOUT
                ),
                None
            );
            let bad = validate_with(
                &bin,
                "cursor-style = bar\nwindow-padding-x = nope\n",
                &schema,
                &std::env::temp_dir(),
                TIMEOUT,
            )
            .expect("Ghostty rejects it");
            assert!(bad.contains_key("window-padding-x"), "{bad:?}");
        }

        #[test]
        fn lists_come_from_the_ghostty_cli() {
            let Some(bin) = bin() else { return };
            let actions = list_lines(&bin, &["+list-actions"]);
            assert!(actions.iter().any(|a| a == "new_tab"), "{actions:?}");
            let colors = list_lines(&bin, &["+list-colors", "--plain"]);
            assert!(colors.len() > 100, "{} colors", colors.len());
        }
    }
}
