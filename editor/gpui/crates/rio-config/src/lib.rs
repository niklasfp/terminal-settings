//! Rio's side of the editor: the schema merge (rio-source.json + rio-overlay.json), the
//! span-splicing config.toml format, value validation, the default config path and themes.

mod format;
mod schema_merge;
mod themes;
mod toml_doc;
mod values;

pub use format::RioTomlFormat;
pub use schema_merge::merge_schema;
pub use settings_core::SchemaDiagnostic;
pub use values::to_toml;
pub use themes::theme_url;

use settings_core::{AppDefinition, MergedSchema, Schema};
use std::path::PathBuf;

const SOURCE_JSON: &str = include_str!("../../../../schema/rio/rio-source.json");
const OVERLAY_JSON: &str = include_str!("../../../../schema/rio/rio-overlay.json");

/// Platform scopes Rio supports under `[platform.<scope>]`.
pub const SCOPES: &[&str] = &["macos", "linux", "windows"];

// Both files are compiled in and covered by tests, so a failure here is a build defect.
static MERGED: MergedSchema =
    MergedSchema::new(|| merge_schema(SOURCE_JSON, OVERLAY_JSON).expect("the embedded Rio schema files are valid JSON"));

/// Rio's settings schema, merged from the checked-in source and overlay.
pub fn schema() -> Schema {
    MERGED.schema()
}

/// What the merge of the checked-in schema files reported (RIO001..RIO008).
pub fn schema_diagnostics() -> Vec<SchemaDiagnostic> {
    MERGED.diagnostics()
}

/// The file name Rio reads, for a config made in the browser playground.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Where Rio reads its config, by platform (see `default_config_path`), for the playground's banner.
pub const CONFIG_LOCATIONS: &[(&str, &str)] = &[
    ("macOS", "~/.config/rio/config.toml"),
    ("Linux", "~/.config/rio/config.toml (or $XDG_CONFIG_HOME/rio)"),
    ("Windows", "%USERPROFILE%\\AppData\\Local\\rio\\config.toml"),
];

/// Rio's editor as the browser playground runs it (`AppDefinition::for_browser`), offering the theme
/// catalogue: none of it is installed there, so picking one also downloads its file.
pub fn browser_app() -> AppDefinition {
    AppDefinition {
        themes: Some(|_| themes::catalogue_themes()),
        ..app().for_browser()
    }
}

/// Rio's editor definition: TOML config, per-platform overrides, and Rio's own hot reload.
pub fn app() -> AppDefinition {
    AppDefinition {
        name: "rio-settings",
        terminal: "Rio",
        schema: schema(),
        format: Box::new(RioTomlFormat),
        scopes: SCOPES,
        default_config_path,
        // Rio watches its config folder and reloads by itself.
        after_save: || Some("Saved. Rio picks up the change right away.".to_string()),
        themes: Some(themes::list),
        install_theme: Some(themes::install),
        validate: None,
        lists: None,
        list_comma_separates: true,
    }
}

fn default_config_path() -> PathBuf {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(home) = env("RIO_CONFIG_HOME") {
        return home.join("config.toml");
    }
    // Same lookup as rio-backend's config_dir_path(): Windows uses %USERPROFILE%\AppData\Local\rio,
    // macOS always ~/.config/rio, other Unix honours XDG_CONFIG_HOME.
    let user_home = std::env::home_dir().unwrap_or_default();
    if cfg!(windows) {
        return user_home
            .join("AppData")
            .join("Local")
            .join("rio")
            .join("config.toml");
    }
    if !cfg!(target_os = "macos")
        && let Some(xdg) = env("XDG_CONFIG_HOME")
    {
        return xdg.join("rio").join("config.toml");
    }
    user_home.join(".config").join("rio").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_is_wired_up() {
        let app = app();
        assert_eq!(app.name, "rio-settings");
        assert_eq!(app.schema.scopes, SCOPES);
        assert!(app.schema.theme_locks_colors);
        assert_eq!(app.schema.brand, "rio");
        assert!((app.default_config_path)().ends_with("config.toml"));
        assert_eq!(
            (app.after_save)().as_deref(),
            Some("Saved. Rio picks up the change right away.")
        );
    }

    #[test]
    fn the_browser_offers_the_catalogue_to_download() {
        let app = browser_app();
        assert!(app.install_theme.is_none(), "the browser downloads themes itself");
        let themes = (app.themes.expect("a theme list"))(std::path::Path::new("config.toml"));
        assert!(themes.len() > 100, "{} themes", themes.len());
        assert!(themes.iter().all(|t| !t.installed), "nothing is installed in a browser");
        let url = theme_url("Dracula").expect("in the catalogue");
        assert!(url.starts_with("https://raw.githubusercontent.com/") && url.ends_with("/Dracula.toml"), "{url}");
        assert!(theme_url("../../etc/passwd").is_err(), "only catalogue names");
    }

    #[test]
    fn format_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<RioTomlFormat>();
    }
}
