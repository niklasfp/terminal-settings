use crate::{AppDefinition, Change, FieldErrors, FormatError, Values};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// First 16 hex chars of the SHA-256 of the text: detects changes on disk since load.
pub fn version_of(text: &str) -> String {
    // Hex-encoded by hand: the digest type changes between sha2 versions and isn't always `LowerHex`.
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// What the UI shows for the file right now.
#[derive(Debug, Clone)]
pub struct ConfigState {
    pub path: PathBuf,
    /// The file actually written: the final symlink target, or `path`.
    pub target: PathBuf,
    pub exists: bool,
    pub version: String,
    pub values: Values,
    pub raw: BTreeMap<String, String>,
    pub notices: Vec<String>,
    /// Set when the file can't be parsed; values are then empty.
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub before: String,
    pub after: String,
    /// Version of `before` for a preview, of `after` after a save.
    pub version: String,
    /// Note from `AppDefinition::after_save`, only after a save that wrote something.
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SaveError {
    /// The file changed on disk since `version` was read.
    Conflict,
    /// The file can't be parsed.
    Parse(String),
    /// Invalid values, unknown keys or locked keys, by setting key ("" = general).
    Invalid(FieldErrors),
    /// Reading or writing failed.
    Io(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => f.write_str("The config file changed on disk since it was loaded."),
            Self::Parse(message) | Self::Io(message) => f.write_str(message),
            Self::Invalid(errors) => match errors.get("") {
                Some(general) if errors.len() == 1 => f.write_str(general),
                _ => f.write_str("Some values are invalid."),
            },
        }
    }
}

impl std::error::Error for SaveError {}

/// Saves run one at a time, so the version check and the write can't interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

const BOM: char = '\u{FEFF}';

/// The text-level core of the save flow: parses, previews and saves config text held in memory.
/// It never touches the filesystem; `ConfigStore` wraps it for a file on disk, and an in-memory
/// `Document` uses it directly.
pub struct TextStore<'a> {
    pub app: &'a AppDefinition,
    /// Where the text belongs: shown in the UI and passed to `AppDefinition::validate`
    /// (relative includes resolve against its folder). Never read or written here.
    pub path: PathBuf,
}

impl<'a> TextStore<'a> {
    pub fn new(app: &'a AppDefinition, path: impl Into<PathBuf>) -> Self {
        Self {
            app,
            path: path.into(),
        }
    }

    /// What the UI shows for this text (without a byte-order mark).
    pub fn state(&self, text: &str) -> ConfigState {
        let mut state = ConfigState {
            path: self.path.clone(),
            target: self.path.clone(),
            exists: true,
            version: version_of(text),
            values: Values::new(),
            raw: BTreeMap::new(),
            notices: Vec::new(),
            parse_error: None,
        };
        match self.app.format.parse(text, &self.app.schema) {
            Ok(parsed) => {
                state.values = parsed.values;
                state.raw = parsed.raw;
                state.notices = parsed.notices;
            }
            Err(e) => state.parse_error = Some(e),
        }
        state
    }

    /// Applies changes (key -> value, `Null` removes) to `before`. `version` is the version the UI
    /// loaded; `None` skips the conflict check. The result's version is that of `before`.
    pub fn apply(
        &self,
        before: &str,
        version: Option<&str>,
        scope: &str,
        changes: &[(String, Value)],
    ) -> Result<ApplyResult, SaveError> {
        if version.is_some_and(|v| v != version_of(before)) {
            return Err(SaveError::Conflict);
        }
        if !scope.is_empty() && !self.app.scopes.contains(&scope) {
            let mut errors = FieldErrors::new();
            errors.insert(String::new(), format!("Unknown platform '{scope}'."));
            return Err(SaveError::Invalid(errors));
        }

        let mut errors = FieldErrors::new();
        let mut accepted = Vec::new();
        for (key, value) in changes {
            let Some(setting) = self.app.schema.find(key) else {
                errors.insert(key.clone(), "Unknown setting.".to_string());
                continue;
            };
            if !scope.is_empty() && !setting.overridable {
                errors.insert(
                    key.clone(),
                    "This setting can't be overridden per platform.".to_string(),
                );
                continue;
            }
            accepted.push(Change {
                setting,
                value: value.clone(),
            });
        }

        let after = match self.app.format.apply(before, &accepted, scope) {
            Ok(after) => match_line_endings(before, &after),
            Err(FormatError::Parse(message)) => return Err(SaveError::Parse(message)),
            Err(FormatError::Invalid(invalid)) => {
                // Report value errors together with unknown or locked keys.
                errors.extend(invalid);
                return Err(SaveError::Invalid(errors));
            }
        };
        if !errors.is_empty() {
            return Err(SaveError::Invalid(errors));
        }
        if after != before
            && let Some(validate) = self.app.validate
            && let Some(invalid) = validate(&after, &self.app.schema, &self.path)
            && !invalid.is_empty()
        {
            return Err(SaveError::Invalid(invalid));
        }
        Ok(ApplyResult {
            before: before.to_string(),
            after,
            version: version_of(before),
            note: None,
        })
    }

    /// `apply`, then takes the new text as saved: when something changed, the result carries the
    /// version of `after` and the app's after-save note. The caller keeps `after` as the new text.
    pub fn save(
        &self,
        before: &str,
        version: Option<&str>,
        scope: &str,
        changes: &[(String, Value)],
    ) -> Result<ApplyResult, SaveError> {
        let result = self.apply(before, version, scope, changes)?;
        Ok(self.saved(result))
    }

    /// Marks an applied result as written: the version of `after`, and the after-save note.
    fn saved(&self, mut result: ApplyResult) -> ApplyResult {
        if result.after != result.before {
            result.version = version_of(&result.after);
            result.note = (self.app.after_save)();
        }
        result
    }
}

/// The config being edited: a file on disk, or text held in memory (where there is no file to
/// write, e.g. in a browser).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document {
    /// Read and written through `ConfigStore`.
    File(PathBuf),
    /// Text without a byte-order mark; `bom` says whether the original had one.
    /// `name` is what the UI shows and what the text would be saved as.
    Memory { name: String, text: String, bom: bool },
}

impl From<PathBuf> for Document {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

impl From<&Path> for Document {
    fn from(path: &Path) -> Self {
        Self::File(path.to_path_buf())
    }
}

impl Document {
    /// In-memory text called `name`. A leading byte-order mark is stripped and remembered.
    pub fn memory(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let (text, bom) = match text.strip_prefix(BOM) {
            Some(rest) => (rest.to_string(), true),
            None => (text, false),
        };
        Self::Memory {
            name: name.into(),
            text,
            bom,
        }
    }

    /// The file's path, or the in-memory document's name as a path.
    pub fn path(&self) -> PathBuf {
        match self {
            Self::File(path) => path.clone(),
            Self::Memory { name, .. } => PathBuf::from(name),
        }
    }

    /// The file name, e.g. "config.toml".
    pub fn file_name(&self) -> Option<String> {
        match self {
            Self::File(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()),
            Self::Memory { name, .. } => Some(name.clone()),
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    /// The in-memory contents as they'd be written (with the byte-order mark if the original had
    /// one); `None` for a file.
    pub fn contents(&self) -> Option<String> {
        match self {
            Self::File(_) => None,
            Self::Memory { text, bom, .. } => Some(if *bom { format!("{BOM}{text}") } else { text.clone() }),
        }
    }
}

/// The store for a `Document`: `ConfigStore` for a file, `TextStore` for text in memory.
/// Saving in memory replaces the document's text.
pub struct DocumentStore<'a> {
    pub app: &'a AppDefinition,
    pub document: Document,
}

impl<'a> DocumentStore<'a> {
    pub fn new(app: &'a AppDefinition, document: impl Into<Document>) -> Self {
        Self {
            app,
            document: document.into(),
        }
    }

    pub fn state(&self) -> ConfigState {
        match &self.document {
            Document::File(path) => ConfigStore::new(self.app, path.clone()).state(),
            Document::Memory { name, text, .. } => TextStore::new(self.app, name.clone()).state(text),
        }
    }

    pub fn apply(
        &self,
        version: Option<&str>,
        scope: &str,
        changes: &[(String, Value)],
    ) -> Result<ApplyResult, SaveError> {
        match &self.document {
            Document::File(path) => ConfigStore::new(self.app, path.clone()).apply(version, scope, changes),
            Document::Memory { name, text, .. } => {
                TextStore::new(self.app, name.clone()).apply(text, version, scope, changes)
            }
        }
    }

    pub fn save(
        &mut self,
        version: Option<&str>,
        scope: &str,
        changes: &[(String, Value)],
    ) -> Result<ApplyResult, SaveError> {
        match &mut self.document {
            Document::File(path) => ConfigStore::new(self.app, path.clone()).save(version, scope, changes),
            Document::Memory { name, text, .. } => {
                let result = TextStore::new(self.app, name.clone()).save(text, version, scope, changes)?;
                text.clone_from(&result.after);
                Ok(result)
            }
        }
    }
}

/// Reads and writes one config file through the app's format (`TextStore` does the text work).
/// Writes are atomic, keep a `.bak`, and go through symlinks to their target.
pub struct ConfigStore<'a> {
    pub app: &'a AppDefinition,
    pub path: PathBuf,
}

impl<'a> ConfigStore<'a> {
    pub fn new(app: &'a AppDefinition, path: impl Into<PathBuf>) -> Self {
        Self {
            app,
            path: path.into(),
        }
    }

    fn text_store(&self) -> TextStore<'a> {
        TextStore::new(self.app, self.path.clone())
    }

    pub fn target_path(&self) -> PathBuf {
        resolve_link(&self.path)
    }

    /// The file's text without a byte-order mark; "" when it doesn't exist or can't be read.
    pub fn read_text(&self) -> String {
        self.read().map(|(text, _)| text).unwrap_or_default()
    }

    /// The text (BOM stripped) and whether the file started with a BOM.
    fn read(&self) -> Result<(String, bool), String> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|_| {
                    format!(
                        "{} isn't UTF-8 text, so it can't be edited here.",
                        self.path.display()
                    )
                })?;
                Ok(match text.strip_prefix(BOM) {
                    Some(rest) => (rest.to_string(), true),
                    None => (text, false),
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((String::new(), false)),
            Err(e) => Err(format!("Couldn't read {}: {e}", self.path.display())),
        }
    }

    pub fn state(&self) -> ConfigState {
        let mut state = match self.read() {
            Ok((text, _)) => self.text_store().state(&text),
            Err(e) => ConfigState {
                path: self.path.clone(),
                target: self.path.clone(),
                exists: true,
                version: version_of(""),
                values: Values::new(),
                raw: BTreeMap::new(),
                notices: Vec::new(),
                parse_error: Some(e),
            },
        };
        state.target = self.target_path();
        state.exists = self.path.exists();
        state
    }

    /// Applies changes (key -> value, `Null` removes) to the current text without writing.
    /// `version` is the version the UI loaded; `None` skips the conflict check.
    pub fn apply(
        &self,
        version: Option<&str>,
        scope: &str,
        changes: &[(String, Value)],
    ) -> Result<ApplyResult, SaveError> {
        let (before, _) = self.read().map_err(SaveError::Io)?;
        self.text_store().apply(&before, version, scope, changes)
    }

    /// `apply`, then write atomically when something changed.
    pub fn save(
        &self,
        version: Option<&str>,
        scope: &str,
        changes: &[(String, Value)],
    ) -> Result<ApplyResult, SaveError> {
        let _guard = SAVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (before, bom) = self.read().map_err(SaveError::Io)?;
        let core = self.text_store();
        let result = core.apply(&before, version, scope, changes)?;
        if result.after == result.before {
            return Ok(result);
        }
        // Validation can take a moment (it runs the terminal); don't overwrite an edit made meanwhile.
        if self.read().map(|(now, _)| now != result.before).unwrap_or(true) {
            return Err(SaveError::Conflict);
        }
        self.write_atomically(&result.before, &result.after, bom)
            .map_err(|e| SaveError::Io(format!("Couldn't save {}: {e}", self.path.display())))?;
        Ok(core.saved(result))
    }

    fn write_atomically(&self, before: &str, after: &str, bom: bool) -> io::Result<()> {
        let target = self.target_path();
        let dir = match target.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        fs::create_dir_all(&dir)?;
        let prefix = if bom { BOM.to_string() } else { String::new() };
        let existing = fs::metadata(&target).ok().filter(|m| m.is_file());
        // Both files keep the config's permissions.
        let permissions = existing.as_ref().map(fs::Metadata::permissions);
        if existing.is_some() {
            let mut bak = target.clone().into_os_string();
            bak.push(".bak");
            replace_file(&dir, &PathBuf::from(bak), &format!("{prefix}{before}"), permissions.clone())?;
        }
        replace_file(&dir, &target, &format!("{prefix}{after}"), permissions)
    }
}

/// Writes `contents` to a new temp file in `dir`, then renames it over `path`. The temp file is
/// created exclusively, so a file or symlink already sitting at its name is never written through,
/// and the rename replaces `path` itself (never the target of a symlink at `path`).
fn replace_file(dir: &Path, path: &Path, contents: &str, permissions: Option<fs::Permissions>) -> io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for _ in 0..16 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let temp = dir.join(format!(".{name}.{}.{nanos:x}.{n}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temp) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let written = file
            .write_all(contents.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| match &permissions {
                Some(p) => fs::set_permissions(&temp, p.clone()),
                None => Ok(()),
            })
            .and_then(|()| fs::rename(&temp, path));
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return written;
    }
    Err(io::Error::other("couldn't create a temporary file next to the config"))
}

/// Follows a chain of symlinks to the final path (which may not exist yet).
fn resolve_link(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    // Same limit as most systems' ELOOP.
    for _ in 0..40 {
        let is_link = fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink());
        if !is_link {
            break;
        }
        let Ok(next) = fs::read_link(&current) else {
            break;
        };
        current = if next.is_absolute() {
            next
        } else {
            current.parent().map(|p| p.join(&next)).unwrap_or(next)
        };
    }
    current
}

/// Keeps the file's line-ending style: lines a format adds use \n, which would mix with CRLF files on Windows.
pub(crate) fn match_line_endings(before: &str, after: &str) -> String {
    let bytes = before.as_bytes();
    let (mut crlf, mut lf) = (0usize, 0usize);
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            if i > 0 && bytes[i - 1] == b'\r' {
                crlf += 1;
            } else {
                lf += 1;
            }
        }
    }
    let unix = after.replace("\r\n", "\n");
    if crlf > lf {
        unix.replace('\n', "\r\n")
    } else {
        unix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigFormat, ParsedConfig, Schema, Setting, SettingType};
    use serde_json::json;

    #[test]
    fn version_is_16_hex_chars_of_sha256() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb924...
        assert_eq!(version_of(""), "e3b0c44298fc1c14");
        assert_eq!(version_of("a = 1\n").len(), 16);
    }

    #[test]
    fn crlf_files_stay_crlf_after_edits() {
        let before = "# mine\r\n[window]\r\nopacity = 1.0\r\n";
        let after = "# mine\r\n[window]\r\nopacity = 0.8\r\n\n[cursor]\nshape = \"beam\"\n";
        assert_eq!(
            match_line_endings(before, after),
            "# mine\r\n[window]\r\nopacity = 0.8\r\n\r\n[cursor]\r\nshape = \"beam\"\r\n"
        );
    }

    #[test]
    fn lf_files_stay_lf() {
        assert_eq!(
            match_line_endings("a = 1\n", "a = 2\nb = 3\n"),
            "a = 2\nb = 3\n"
        );
    }

    /// `key = <json>` lines; a value of "bad" is rejected.
    struct LineFormat;

    impl ConfigFormat for LineFormat {
        fn parse(&self, text: &str, _schema: &Schema) -> Result<ParsedConfig, String> {
            let mut parsed = ParsedConfig::default();
            for line in text.lines() {
                let (k, v) = line.split_once(" = ").ok_or("bad line")?;
                parsed.values.insert(
                    k.to_string(),
                    serde_json::from_str(v).map_err(|e| e.to_string())?,
                );
            }
            Ok(parsed)
        }

        fn apply(
            &self,
            text: &str,
            changes: &[Change<'_>],
            scope: &str,
        ) -> Result<String, FormatError> {
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            let mut errors = FieldErrors::new();
            for c in changes {
                let key = if scope.is_empty() {
                    c.setting.key.clone()
                } else {
                    format!("{scope}.{}", c.setting.key)
                };
                if c.value == json!("bad") {
                    errors.insert(c.setting.key.clone(), "Bad value.".into());
                    continue;
                }
                lines.retain(|l| !l.starts_with(&format!("{key} = ")));
                if !c.value.is_null() {
                    lines.push(format!("{key} = {}", c.value));
                }
            }
            if !errors.is_empty() {
                return Err(FormatError::Invalid(errors));
            }
            Ok(lines.iter().map(|l| format!("{l}\n")).collect())
        }
    }

    fn setting(key: &str, overridable: bool) -> Setting {
        let mut s = Setting::new(key, "general", SettingType::Int);
        s.label = key.into();
        s.overridable = overridable;
        s
    }

    fn app() -> AppDefinition {
        AppDefinition {
            name: "test-settings",
            terminal: "Test",
            schema: Schema {
                revision: "r".into(),
                terminal: "Test".into(),
                title: "Test settings".into(),
                brand: "test".into(),
                scopes: vec!["macos".into()],
                theme_locks_colors: false,
                groups: vec![],
                settings: vec![setting("a", true), setting("b", false)],
            },
            format: Box::new(LineFormat),
            scopes: &["macos"],
            default_config_path: || PathBuf::from("config"),
            after_save: || Some("Saved.".into()),
            themes: None,
            install_theme: None,
            validate: None,
            lists: None,
            list_comma_separates: true,
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_backup_path_is_replaced_not_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let path = dir.path().join("config");
        let victim = dir.path().join("victim");
        fs::write(&path, "a = 1\n").unwrap();
        fs::write(&victim, "precious\n").unwrap();
        std::os::unix::fs::symlink(&victim, dir.path().join("config.bak")).unwrap();
        ConfigStore::new(&app, &path).save(None, "", &[("a".to_string(), json!(2))]).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "precious\n", "the link's target is untouched");
        let bak = dir.path().join("config.bak");
        assert!(!fs::symlink_metadata(&bak).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(bak).unwrap(), "a = 1\n");
        let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty(), "no temp files left behind");
    }

    #[test]
    fn validation_errors_block_preview_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.validate = Some(|text, _, _| {
            text.contains("a = 9").then(|| FieldErrors::from([("a".to_string(), "Rejected by the terminal.".to_string())]))
        });
        let path = dir.path().join("config");
        fs::write(&path, "a = 1\n").unwrap();
        let store = ConfigStore::new(&app, &path);
        let changes = [("a".to_string(), json!(9))];
        let Err(SaveError::Invalid(errors)) = store.apply(None, "", &changes) else { panic!("preview must fail") };
        assert_eq!(errors["a"], "Rejected by the terminal.");
        assert!(matches!(store.save(None, "", &changes), Err(SaveError::Invalid(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1\n", "nothing written");
        assert!(store.save(None, "", &[("a".to_string(), json!(2))]).is_ok(), "valid values still save");
    }

    /// Where the slow-validation test's validator edits the file, imitating another editor.
    static EDIT_DURING_VALIDATION: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

    #[test]
    fn an_edit_made_while_validating_is_a_conflict_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.validate = Some(|_, _, _| {
            if let Some(path) = EDIT_DURING_VALIDATION.lock().unwrap().as_ref() {
                fs::write(path, "a = 1\nb = 5\n").unwrap();
            }
            None
        });
        let path = dir.path().join("config");
        fs::write(&path, "a = 1\n").unwrap();
        *EDIT_DURING_VALIDATION.lock().unwrap() = Some(path.clone());
        let result = ConfigStore::new(&app, &path).save(None, "", &[("a".to_string(), json!(2))]);
        *EDIT_DURING_VALIDATION.lock().unwrap() = None;
        assert!(matches!(result, Err(SaveError::Conflict)), "{result:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1\nb = 5\n", "the other edit survives");
    }

    #[test]
    fn missing_file_is_empty_and_save_creates_it_without_bak() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let path = dir.path().join("sub").join("config");
        let store = ConfigStore::new(&app, &path);
        let state = store.state();
        assert!(!state.exists);
        assert_eq!(state.version, version_of(""));

        let r = store
            .save(Some(&state.version), "", &[("a".into(), json!(1))])
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1\n");
        assert_eq!(r.version, version_of("a = 1\n"));
        assert_eq!(r.note.as_deref(), Some("Saved."));
        assert!(!dir.path().join("sub").join("config.bak").exists());
        assert_eq!(store.state().values["a"], json!(1));
    }

    #[test]
    fn unchanged_save_writes_nothing_and_has_no_note() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let path = dir.path().join("config");
        fs::write(&path, "a = 1\n").unwrap();
        let r = ConfigStore::new(&app, &path)
            .save(None, "", &[("a".into(), json!(1))])
            .unwrap();
        assert_eq!(r.before, r.after);
        assert!(r.note.is_none());
        assert!(!dir.path().join("config.bak").exists());
    }

    #[test]
    fn unknown_and_locked_keys_are_merged_with_value_errors() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let store = ConfigStore::new(&app, dir.path().join("config"));
        let changes = [
            ("a".into(), json!("bad")),
            ("b".into(), json!(2)),
            ("nope".into(), json!(3)),
        ];
        let Err(SaveError::Invalid(errors)) = store.apply(None, "macos", &changes) else {
            panic!("expected errors")
        };
        assert_eq!(
            errors["b"],
            "This setting can't be overridden per platform."
        );
        assert_eq!(errors["nope"], "Unknown setting.");
        assert_eq!(errors["a"], "Bad value.");
    }

    #[test]
    fn unknown_scope_is_a_general_error() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let store = ConfigStore::new(&app, dir.path().join("config"));
        let Err(SaveError::Invalid(errors)) = store.apply(None, "beos", &[]) else {
            panic!("expected errors")
        };
        assert_eq!(errors[""], "Unknown platform 'beos'.");
    }

    #[test]
    fn bom_is_kept_but_not_part_of_the_text() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let path = dir.path().join("config");
        fs::write(&path, "\u{FEFF}a = 1\n").unwrap();
        let store = ConfigStore::new(&app, &path);
        assert_eq!(store.read_text(), "a = 1\n");
        store
            .save(Some(&version_of("a = 1\n")), "", &[("a".into(), json!(2))])
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "\u{FEFF}a = 2\n");
    }

    #[test]
    fn unreadable_text_is_an_io_error_not_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let path = dir.path().join("config");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let store = ConfigStore::new(&app, &path);
        assert!(store.state().parse_error.is_some());
        assert!(matches!(
            store.save(None, "", &[("a".into(), json!(1))]),
            Err(SaveError::Io(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe, 0x00]);
    }

    // ---------- the text core: no filesystem ----------

    #[test]
    fn text_state_parses_without_reading_anything() {
        let app = app();
        let state = TextStore::new(&app, "/nowhere/config").state("a = 1\n");
        assert_eq!(state.values["a"], json!(1));
        assert_eq!(state.version, version_of("a = 1\n"));
        assert_eq!(state.path, PathBuf::from("/nowhere/config"));
        assert!(state.parse_error.is_none());
        assert!(TextStore::new(&app, "config").state("nonsense").parse_error.is_some());
    }

    #[test]
    fn text_save_returns_the_new_text_its_version_and_the_note() {
        let app = app();
        let store = TextStore::new(&app, "config");
        let r = store.save("a = 1\n", Some(&version_of("a = 1\n")), "", &[("a".into(), json!(2))]).unwrap();
        assert_eq!(r.before, "a = 1\n");
        assert_eq!(r.after, "a = 2\n");
        assert_eq!(r.version, version_of("a = 2\n"));
        assert_eq!(r.note.as_deref(), Some("Saved."));

        let unchanged = store.save("a = 1\n", None, "", &[("a".into(), json!(1))]).unwrap();
        assert_eq!(unchanged.version, version_of("a = 1\n"));
        assert!(unchanged.note.is_none());
    }

    #[test]
    fn text_apply_checks_the_version_and_the_values() {
        let app = app();
        let store = TextStore::new(&app, "config");
        assert!(matches!(store.apply("a = 1\n", Some("stale"), "", &[]), Err(SaveError::Conflict)));
        let Err(SaveError::Invalid(errors)) = store.apply("", None, "macos", &[("b".into(), json!(1))]) else {
            panic!("expected errors")
        };
        assert_eq!(errors["b"], "This setting can't be overridden per platform.");
        let preview = store.apply("a = 1\n", None, "", &[("a".into(), json!(3))]).unwrap();
        assert_eq!((preview.after.as_str(), preview.version.as_str()), ("a = 3\n", version_of("a = 1\n").as_str()));
        assert!(preview.note.is_none(), "a preview has no after-save note");
    }

    #[test]
    fn memory_documents_keep_the_bom_and_save_into_their_text() {
        let app = app();
        let mut store = DocumentStore::new(&app, Document::memory("config", "\u{FEFF}a = 1\r\n"));
        let state = store.state();
        assert_eq!(state.values["a"], json!(1));
        assert_eq!(state.path, PathBuf::from("config"));
        store.save(Some(&state.version), "", &[("a".into(), json!(2)), ("b".into(), json!(3))]).unwrap();
        assert_eq!(store.document.contents().as_deref(), Some("\u{FEFF}a = 2\r\nb = 3\r\n"));
        assert_eq!(store.state().version, version_of("a = 2\r\nb = 3\r\n"));
        assert_eq!(store.document.file_name().as_deref(), Some("config"));
        assert!(!store.document.is_file());
    }

    #[test]
    fn memory_and_file_saves_write_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let original = "\u{FEFF}a = 1\r\n";
        let changes = [("a".to_string(), json!(5)), ("b".to_string(), json!(6))];
        let path = dir.path().join("config");
        fs::write(&path, original).unwrap();
        ConfigStore::new(&app, &path).save(None, "", &changes).unwrap();
        let mut memory = DocumentStore::new(&app, Document::memory("config", original));
        memory.save(None, "", &changes).unwrap();
        assert_eq!(memory.document.contents().unwrap(), fs::read_to_string(&path).unwrap());
    }
}
