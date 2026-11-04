//! Terminal-agnostic core of the settings editors: the schema model, the config-format seam,
//! the save flow (version check, validation, atomic write) and the line diff shown before saving.
//! Nothing terminal-specific belongs here.

mod diff;
pub mod merge;
mod schema;
mod store;
pub mod values;

pub use diff::{DiffLine, DiffOp, line_diff};
pub use merge::{MergedSchema, SchemaDiagnostic, humanize};
pub use values::format_number;
pub use schema::{Field, Group, Schema, Setting, SettingType};
pub use store::{ApplyResult, ConfigState, ConfigStore, Document, DocumentStore, SaveError, TextStore, version_of};

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Values by setting key (or dotted path), flattened. Inline tables are flattened too.
pub type Values = Map<String, Value>;

/// Errors by setting key; "" is a general error not tied to one setting.
pub type FieldErrors = BTreeMap<String, String>;

/// What a format reads out of a config file.
#[derive(Debug, Clone, Default)]
pub struct ParsedConfig {
    pub values: Values,
    /// Raw text for settings edited as text blocks (`SettingType::Raw`), by setting key.
    pub raw: BTreeMap<String, String>,
    /// Things the user should know about the file (e.g. includes the editor doesn't follow).
    pub notices: Vec<String>,
}

/// One change from the UI. `Value::Null` removes the setting from the file.
#[derive(Debug, Clone)]
pub struct Change<'a> {
    pub setting: &'a Setting,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub enum FormatError {
    /// The file can't be parsed.
    Parse(String),
    /// Some values are invalid, by setting key.
    Invalid(FieldErrors),
}

/// A config file format: how values are read from it and how changes are spliced in.
pub trait ConfigFormat: Send + Sync {
    /// Reads every value. `Err` carries a user-facing parse error.
    fn parse(&self, text: &str, schema: &Schema) -> Result<ParsedConfig, String>;

    /// Returns `text` with the changes applied, touching nothing else. `scope` is "" for
    /// all platforms, or one of `AppDefinition::scopes`.
    fn apply(&self, text: &str, changes: &[Change<'_>], scope: &str)
    -> Result<String, FormatError>;
}

/// A theme for the picker: name, whether it's usable right away, and colors for a preview.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeInfo {
    pub name: String,
    pub installed: bool,
    pub background: Option<String>,
    pub foreground: Option<String>,
    /// The 8 normal ANSI colors, black..white.
    pub ansi: Vec<Option<String>>,
}

/// The user's theme folder: `themes` next to the config file.
pub fn themes_dir(config_path: &std::path::Path) -> std::path::PathBuf {
    config_path.parent().unwrap_or(std::path::Path::new("")).join("themes")
}

/// A theme list as every app shows it: the user's themes, then the others they don't override;
/// each part sorted by name, ignoring case.
pub fn theme_list(mut user: Vec<ThemeInfo>, others: impl IntoIterator<Item = ThemeInfo>) -> Vec<ThemeInfo> {
    let names: std::collections::HashSet<String> = user.iter().map(|t| t.name.clone()).collect();
    let mut rest: Vec<ThemeInfo> = others.into_iter().filter(|t| !names.contains(&t.name)).collect();
    user.sort_by_cached_key(|t| t.name.to_uppercase());
    rest.sort_by_cached_key(|t| t.name.to_uppercase());
    user.extend(rest);
    user
}

/// Installs a catalogue theme for the config at the given path. Blocking. `Err` is user-facing.
pub type InstallTheme = fn(&std::path::Path, &str) -> Result<(), String>;

/// Checks a candidate file before it's written (e.g. with the terminal's own parser). Blocking.
/// Gets the candidate text and the config file's path (relative includes resolve against its folder).
/// `None` means it's fine (or can't be checked); `Some` holds errors by setting key, "" for general.
pub type Validate = fn(&str, &Schema, &std::path::Path) -> Option<FieldErrors>;

/// Suggestion lists for pickers by name (e.g. "actions", "x11-colors"); empty when unknown. Blocking.
pub type Lists = fn(&str) -> Vec<String>;

/// Everything the UI needs to know about one terminal's editor.
pub struct AppDefinition {
    /// Executable name, e.g. "rio-settings".
    pub name: &'static str,
    /// Human name of the terminal, e.g. "Rio".
    pub terminal: &'static str,
    pub schema: Schema,
    pub format: Box<dyn ConfigFormat>,
    /// Platform scopes for per-platform overrides (empty = none).
    pub scopes: &'static [&'static str],
    /// Resolves the config file to edit when --config isn't given.
    pub default_config_path: fn() -> std::path::PathBuf,
    /// Note shown after a successful save (e.g. "Rio picks up the change right away.").
    pub after_save: fn() -> Option<String>,
    /// Themes for the theme picker (installed first, then installable), given the config path.
    pub themes: Option<fn(&std::path::Path) -> Vec<ThemeInfo>>,
    /// Installs a catalogue theme so the config can use it. Blocking (may download). `Err` is user-facing.
    pub install_theme: Option<InstallTheme>,
    /// Runs on every preview and save whose result differs from the file.
    pub validate: Option<Validate>,
    /// Suggestion lists for pickers.
    pub lists: Option<Lists>,
    /// Whether typing a comma in a list field ends the entry. True where list items are separate
    /// values (Rio's TOML arrays); false where an entry may itself contain commas (Ghostty).
    pub list_comma_separates: bool,
}

impl AppDefinition {
    /// The editor as a browser can run it: the text is downloaded instead of saved, and nothing
    /// reaches the system. So no theme list or install (they read and write the theme folder or
    /// download), no validation or lists (they run the terminal), and no reload note.
    pub fn for_browser(self) -> Self {
        Self {
            after_save: || Some(BROWSER_SAVE_NOTE.to_string()),
            themes: None,
            install_theme: None,
            validate: None,
            lists: None,
            ..self
        }
    }
}

/// The note after a save in the browser, where "saving" downloads the file.
pub const BROWSER_SAVE_NOTE: &str = "Downloaded. Put the file where your terminal reads it.";

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) => f.write_str(message),
            Self::Invalid(_) => f.write_str("Some values are invalid."),
        }
    }
}

impl std::error::Error for FormatError {}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoFormat;

    impl ConfigFormat for NoFormat {
        fn parse(&self, _: &str, _: &Schema) -> Result<ParsedConfig, String> {
            Ok(ParsedConfig::default())
        }

        fn apply(&self, text: &str, _: &[Change<'_>], _: &str) -> Result<String, FormatError> {
            Ok(text.to_string())
        }
    }

    #[test]
    fn the_browser_editor_drops_everything_that_reaches_the_system() {
        let app = AppDefinition {
            name: "x-settings",
            terminal: "X",
            schema: Schema {
                revision: "r".into(),
                terminal: "X".into(),
                title: "X settings".into(),
                brand: "x".into(),
                scopes: vec![],
                theme_locks_colors: false,
                groups: vec![],
                settings: vec![],
            },
            format: Box::new(NoFormat),
            scopes: &["macos"],
            default_config_path: || "config".into(),
            after_save: || Some("Reloaded.".into()),
            themes: Some(|_| Vec::new()),
            install_theme: Some(|_, _| Ok(())),
            validate: Some(|_, _, _| None),
            lists: Some(|_| Vec::new()),
            list_comma_separates: false,
        }
        .for_browser();
        assert!(app.themes.is_none() && app.install_theme.is_none());
        assert!(app.validate.is_none() && app.lists.is_none());
        assert_eq!((app.after_save)().as_deref(), Some(BROWSER_SAVE_NOTE));
        assert_eq!((app.name, app.scopes, app.list_comma_separates), ("x-settings", &["macos"][..], false));
    }
}
