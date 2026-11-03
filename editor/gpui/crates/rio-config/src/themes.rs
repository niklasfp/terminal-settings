//! Rio ships no themes: it loads `<config dir>/themes/<name>.toml`. The picker lists the installed
//! ones plus the catalogue baked in from mbadolato/iTerm2-Color-Schemes (see tools/rio/update-themes.ts);
//! picking a catalogue theme downloads that one file from the pinned commit.

use crate::toml_doc::ConfigDocument;
use serde_json::Value;
use settings_core::{ThemeInfo, theme_list, themes_dir};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

const ANSI_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

struct Catalogue {
    repo: String,
    commit: String,
    path: String,
    themes: Vec<ThemeInfo>,
}

fn catalogue() -> &'static Catalogue {
    static CATALOGUE: OnceLock<Catalogue> = OnceLock::new();
    CATALOGUE.get_or_init(|| load_catalogue(include_str!("../../../../schema/rio/rio-themes.json")))
}

fn load_catalogue(json: &str) -> Catalogue {
    let root: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let mut themes: Vec<ThemeInfo> = Vec::new();
    for t in root
        .get("themes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = text(t, "name") else {
            continue;
        };
        let ansi = t
            .get("ansi")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|c| c.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let info = ThemeInfo {
            name,
            installed: false,
            background: text(t, "bg"),
            foreground: text(t, "fg"),
            ansi,
        };
        // Later entries win, like a dictionary keyed by name.
        match themes.iter_mut().find(|x| x.name == info.name) {
            Some(existing) => *existing = info,
            None => themes.push(info),
        }
    }
    themes.sort_by_cached_key(|t| t.name.to_uppercase());
    Catalogue {
        repo: text(&root, "repo").unwrap_or_default(),
        commit: text(&root, "commit").unwrap_or_default(),
        path: text(&root, "path").unwrap_or_else(|| "rio".into()),
        themes,
    }
}

/// Installed themes first, then catalogue themes that aren't installed; each sorted by name.
pub fn list(config_path: &Path) -> Vec<ThemeInfo> {
    let installed: Vec<ThemeInfo> = fs::read_dir(themes_dir(config_path))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
        .map(|p| read_installed(&p))
        .collect();
    theme_list(installed, catalogue().themes.iter().cloned())
}

fn read_installed(file: &Path) -> ThemeInfo {
    let name = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parsed = fs::read_to_string(file)
        .ok()
        .and_then(|text| ConfigDocument::parse(&text).ok());
    let Some(doc) = parsed else {
        return ThemeInfo {
            name,
            installed: true,
            background: None,
            foreground: None,
            ansi: Vec::new(),
        };
    };
    let values = doc.values();
    let color = |key: &str| {
        values.get(&format!("colors.{key}")).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    };
    ThemeInfo {
        name,
        installed: true,
        background: color("background"),
        foreground: color("foreground"),
        ansi: ANSI_NAMES.iter().map(|n| color(n)).collect(),
    }
}

/// Downloads one catalogue theme into the themes folder. Blocking. `Err` is user-facing.
pub fn install(config_path: &Path, name: &str) -> Result<(), String> {
    let cat = catalogue();
    // Only names from the baked-in catalogue: they also become file names, so no paths sneak in.
    if !cat.themes.iter().any(|t| t.name == name) {
        return Err(format!("'{name}' isn't in the theme catalogue."));
    }
    if name.chars().any(invalid_file_name_char) {
        return Err(format!(
            "'{name}' can't be used as a file name on this system."
        ));
    }
    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}.toml",
        cat.repo,
        cat.commit,
        cat.path,
        escape_data_string(name)
    );
    let text = download(&url).map_err(|e| format!("Couldn't download the theme ({e})."))?;
    if ConfigDocument::parse(&text).is_err() {
        return Err("The downloaded theme isn't valid TOML.".into());
    }
    let dir = themes_dir(config_path);
    fs::create_dir_all(&dir)
        .and_then(|()| fs::write(dir.join(format!("{name}.toml")), text))
        .map_err(|e| format!("Couldn't save the theme in {} ({e}).", dir.display()))
}

fn download(url: &str) -> Result<String, ureq::Error> {
    let config = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20)));
    // The system's TLS and trusted roots (see Cargo.toml); Linux keeps rustls' defaults.
    #[cfg(not(target_os = "linux"))]
    let config = config.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build(),
    );
    let agent: ureq::Agent = config.build().into();
    agent.get(url).call()?.body_mut().read_to_string()
}

fn invalid_file_name_char(c: char) -> bool {
    if cfg!(windows) {
        matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 0x20
    } else {
        c == '/' || c == '\0'
    }
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn escape_data_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_embedded_with_preview_colors() {
        let dir = tempfile::tempdir().unwrap();
        let themes = list(&dir.path().join("config.toml"));
        assert!(themes.len() > 500);
        let dracula = themes
            .iter()
            .find(|t| t.name == "Dracula")
            .expect("Dracula in catalogue");
        assert!(!dracula.installed);
        assert_eq!(dracula.background.as_deref(), Some("#282a36"));
        assert_eq!(dracula.ansi.len(), 8);
    }

    #[test]
    fn installed_themes_come_first_and_win_over_the_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        fs::create_dir_all(dir.path().join("themes")).unwrap();
        fs::write(
            dir.path().join("themes").join("Dracula.toml"),
            "[colors]\nbackground = '#000000'\nforeground = '#ffffff'\nred = '#ff0000'\n",
        )
        .unwrap();
        let themes = list(&config);
        assert_eq!(themes[0].name, "Dracula");
        assert!(themes[0].installed);
        assert_eq!(themes[0].background.as_deref(), Some("#000000"));
        assert_eq!(themes[0].ansi[1].as_deref(), Some("#ff0000"));
        assert_eq!(themes.iter().filter(|t| t.name == "Dracula").count(), 1);
    }

    #[test]
    fn broken_installed_theme_is_listed_without_colors() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("themes")).unwrap();
        fs::write(dir.path().join("themes").join("Mine.toml"), "[colors\n").unwrap();
        let themes = list(&dir.path().join("config.toml"));
        assert_eq!(themes[0].name, "Mine");
        assert!(themes[0].installed && themes[0].background.is_none() && themes[0].ansi.is_empty());
    }

    #[test]
    fn install_only_accepts_catalogue_names() {
        for name in ["../../evil", "Not A Real Theme"] {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("config.toml");
            assert!(install(&config, name).is_err(), "{name}");
            assert!(!dir.path().join("themes").exists());
        }
    }

    #[test]
    fn theme_names_are_escaped_like_uri_data() {
        assert_eq!(
            escape_data_string("Builtin Solarized (Dark)+"),
            "Builtin%20Solarized%20%28Dark%29%2B"
        );
        assert_eq!(escape_data_string("é"), "%C3%A9");
    }
}
