//! The editor's state, without any UI: what the file has, what the user changed, and what each
//! setting resolves to. Ported from the browser runtime's state handling (app.js).

use serde_json::{Map, Value};
use settings_core::{ConfigState, Schema, Setting, SettingType};

/// The 8 normal ANSI color names, in order.
pub const ANSI: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

pub struct Editor {
    pub schema: &'static Schema,
    pub state: ConfigState,
    /// "" = all platforms, else a platform scope ("macos", ...).
    pub scope: String,
    /// Unsaved edits, key -> value. `Null` removes the line on save.
    dirty: Map<String, Value>,
}

/// Why a row can't be edited right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lock {
    /// Editing a platform scope, and this setting can't be overridden per platform.
    Scope,
    /// The terminal ignores colors while a theme is set.
    Theme,
}

impl Editor {
    pub fn new(schema: &'static Schema, state: ConfigState) -> Self {
        Self { schema, state, scope: String::new(), dirty: Map::new() }
    }

    fn scoped_key(&self, key: &str) -> String {
        if self.scope.is_empty() { key.to_string() } else { format!("platform.{}.{key}", self.scope) }
    }

    /// The value the file sets for this setting in the current scope, if any.
    pub fn file_value(&self, s: &Setting) -> Option<Value> {
        if s.ty == SettingType::Raw {
            if !self.scope.is_empty() {
                return None;
            }
            return self.state.raw.get(&s.key).filter(|r| !r.is_empty()).map(|r| Value::String(r.clone()));
        }
        self.state.values.get(&self.scoped_key(&s.key)).cloned()
    }

    /// What the setting resolves to if the current scope doesn't set it.
    pub fn fallback_value(&self, s: &Setting) -> Value {
        if !self.scope.is_empty()
            && let Some(v) = self.state.values.get(&s.key)
        {
            return v.clone();
        }
        s.default.clone()
    }

    pub fn current_value(&self, s: &Setting) -> Value {
        match self.dirty.get(&s.key) {
            Some(Value::Null) => self.fallback_value(s),
            Some(v) => v.clone(),
            None => self.file_value(s).unwrap_or_else(|| self.fallback_value(s)),
        }
    }

    /// Whether the file sets it (after pending edits).
    pub fn is_set(&self, s: &Setting) -> bool {
        match self.dirty.get(&s.key) {
            Some(v) => !v.is_null(),
            None => self.file_value(s).is_some(),
        }
    }

    pub fn is_dirty(&self, key: &str) -> bool {
        self.dirty.contains_key(key)
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Records an edit. `Null` means "remove from the file". Edits back to what the file has (or
    /// to the default while unset) drop out of the pending changes.
    pub fn edit(&mut self, s: &Setting, value: Value) {
        let file = self.file_value(s);
        if value.is_null() {
            if file.is_none() {
                self.dirty.remove(&s.key);
            } else {
                self.dirty.insert(s.key.clone(), Value::Null);
            }
            return;
        }
        let unchanged = match &file {
            Some(f) => same(&value, f),
            None => same(&value, &self.fallback_value(s)) && s.ty != SettingType::Raw,
        };
        if unchanged {
            self.dirty.remove(&s.key);
        } else {
            self.dirty.insert(s.key.clone(), value);
        }
    }

    /// The row's reset button: first undoes an unsaved edit, then removes a set value from the file.
    pub fn reset(&mut self, s: &Setting) {
        if self.dirty.remove(&s.key).is_none() {
            self.edit(s, Value::Null);
        }
    }

    pub fn discard(&mut self) {
        self.dirty.clear();
    }

    /// Drops the pending changes that a save wrote, keeping any made (or changed again) since.
    pub fn clear_saved(&mut self, saved: &[(String, Value)]) {
        for (key, value) in saved {
            if self.dirty.get(key).is_some_and(|pending| same(pending, value)) {
                self.dirty.remove(key);
            }
        }
    }

    pub fn changes(&self) -> Vec<(String, Value)> {
        self.dirty.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn set_scope(&mut self, scope: &str) {
        self.scope = scope.to_string();
    }

    /// Settings set in the file (after pending edits), for a group's sidebar count.
    /// How many of a section's settings the saved file sets (unsaved edits don't count).
    pub fn in_file_count(&self, group: &str) -> usize {
        self.schema.settings.iter().filter(|s| s.group == group && self.file_value(s).is_some()).count()
    }

    pub fn theme_active(&self) -> bool {
        self.schema
            .find("theme")
            .map(|t| matches!(self.current_value(t), Value::String(ref s) if !s.is_empty()))
            .unwrap_or(false)
    }

    pub fn lock(&self, s: &Setting) -> Option<Lock> {
        if !self.scope.is_empty() && (!s.overridable || s.ty == SettingType::Raw) {
            Some(Lock::Scope)
        } else if self.schema.theme_locks_colors && s.group == "colors" && self.theme_active() {
            Some(Lock::Theme)
        } else {
            None
        }
    }

    /// The "default: …" / "all platforms: …" note under a setting's key.
    pub fn default_note(&self, s: &Setting) -> String {
        if !self.scope.is_empty() && self.state.values.contains_key(&s.key) {
            format!("all platforms: {}", describe(self.schema, s, &self.fallback_value(s)))
        } else if s.ty == SettingType::Raw {
            String::new()
        } else {
            format!("default: {}", describe(self.schema, s, &self.fallback_value(s)))
        }
    }

    /// A color for the terminal preview: the setting's current hex value, if it is one.
    pub fn color(&self, name: &str) -> Option<String> {
        let s = self.schema.find(&format!("colors.{name}"))?;
        match self.current_value(s) {
            Value::String(v) if is_hex_color(&v) => Some(v),
            _ => None,
        }
    }
}

/// Short human description of a value, for the "default: …" note.
pub fn describe(schema: &Schema, s: &Setting, v: &Value) -> String {
    let terminal = &schema.terminal;
    let empty = match v {
        Value::Null => true,
        Value::String(t) => t.is_empty(),
        _ => false,
    };
    if empty {
        return if matches!(s.ty, SettingType::Theme | SettingType::String) {
            "empty".into()
        } else {
            format!("{terminal} decides")
        };
    }
    let list = |v: &Value| -> Vec<String> {
        v.as_array().map(|a| a.iter().map(plain).collect()).unwrap_or_default()
    };
    match s.ty {
        SettingType::Bool => if v.as_bool() == Some(true) { "On" } else { "Off" }.into(),
        SettingType::Margin => format!("{} px", list(v).join(" ")),
        SettingType::Pair => {
            let l = list(v);
            format!("{} / {} ms", l.first().map_or("", |s| s), l.get(1).map_or("", |s| s))
        }
        SettingType::Strings | SettingType::Fonts | SettingType::Flags => {
            let l = list(v);
            if l.is_empty() { "none".into() } else { l.join(", ") }
        }
        SettingType::Records => match v.as_array().map_or(0, Vec::len) {
            0 => "none".into(),
            1 => "1 entry".into(),
            n => format!("{n} entries"),
        },
        SettingType::Blur => match v {
            Value::Bool(false) => "Off".into(),
            Value::Bool(true) => "Blur".into(),
            other => plain(other),
        },
        SettingType::FontStyle => match v {
            Value::Bool(false) => "Off".into(),
            other => plain(other),
        },
        SettingType::Raw => format!("{terminal}'s built-in rules"),
        _ => plain(v),
    }
}

/// A value as plain text: strings without quotes, numbers without a trailing ".0".
pub fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => format_number(n.as_f64().unwrap_or(0.0)),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub use settings_core::format_number;

/// Value equality as the UI sees it: numbers by value, hex colors case-insensitively.
pub fn same(a: &Value, b: &Value) -> bool {
    norm(a) == norm(b)
}

fn norm(v: &Value) -> Value {
    match v {
        Value::Array(items) => Value::Array(items.iter().map(norm).collect()),
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), norm(&map[k]))).collect())
        }
        Value::Number(n) => n.as_f64().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
        Value::String(s) if is_hex_color(s) => Value::String(s.to_lowercase()),
        other => other.clone(),
    }
}

pub fn is_hex_color(s: &str) -> bool {
    let h = s.strip_prefix('#').unwrap_or("");
    matches!(h.len(), 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
}

/// Lower-cased text a setting is searched by: label, key, doc, group title and subgroup.
pub fn search_text(schema: &Schema, s: &Setting) -> String {
    let group = schema.groups.iter().find(|g| g.id == s.group).map_or("", |g| g.title.as_str());
    [s.label.as_str(), &s.key, s.doc.as_deref().unwrap_or(""), group, s.subgroup.as_deref().unwrap_or("")]
        .join(" ")
        .to_lowercase()
}

/// Every whitespace-separated term must appear in the setting's search text.
pub fn matches_search(haystack: &str, query: &str) -> bool {
    let q = query.to_lowercase();
    q.split_whitespace().all(|t| haystack.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use settings_core::{Group, Setting};
    use std::path::PathBuf;

    fn setting(key: &str, ty: SettingType, default: Value) -> Setting {
        let mut s = Setting::new(key, key.split('.').next().unwrap(), ty);
        s.label = key.into();
        s.default = default;
        s.overridable = true;
        s
    }

    fn schema() -> &'static Schema {
        let mut no_override = setting("window.opacity-cells", SettingType::Bool, json!(false));
        no_override.overridable = false;
        Box::leak(Box::new(Schema {
            revision: "test".into(),
            terminal: "Rio".into(),
            title: "Rio settings".into(),
            brand: "rio".into(),
            scopes: vec!["macos".into()],
            theme_locks_colors: true,
            groups: ["window", "theme", "colors"]
                .iter()
                .map(|g| Group { id: (*g).into(), title: (*g).into(), icon: String::new(), doc: None })
                .collect(),
            settings: vec![
                setting("window.opacity", SettingType::Float, json!(1)),
                no_override,
                setting("theme", SettingType::Theme, json!("")),
                setting("colors.background", SettingType::Color, json!("#0f0d0e")),
            ],
        }))
    }

    fn state(values: Value) -> ConfigState {
        ConfigState {
            path: PathBuf::from("config.toml"),
            target: PathBuf::from("config.toml"),
            exists: true,
            version: "v1".into(),
            values: values.as_object().cloned().unwrap_or_default(),
            raw: Default::default(),
            notices: vec![],
            parse_error: None,
        }
    }

    fn find(key: &str) -> &'static Setting {
        schema().settings.iter().find(|s| s.key == key).unwrap()
    }

    #[test]
    fn edit_back_to_default_while_unset_is_not_a_change() {
        let mut e = Editor::new(schema(), state(json!({})));
        let s = find("window.opacity");
        e.edit(s, json!(0.5));
        assert!(e.is_dirty("window.opacity"));
        e.edit(s, json!(1.0));
        assert!(!e.is_dirty("window.opacity"), "1.0 equals the default 1");
    }

    #[test]
    fn reset_undoes_first_then_removes() {
        let mut e = Editor::new(schema(), state(json!({"window.opacity": 0.9})));
        let s = find("window.opacity");
        e.edit(s, json!(0.5));
        e.reset(s);
        assert!(!e.is_dirty("window.opacity"));
        assert_eq!(e.current_value(s), json!(0.9));
        e.reset(s);
        assert_eq!(e.changes(), vec![("window.opacity".to_string(), Value::Null)]);
        assert_eq!(e.current_value(s), json!(1));
        assert!(!e.is_set(s));
    }

    #[test]
    fn scope_falls_back_to_all_platform_value() {
        let mut e = Editor::new(schema(), state(json!({"window.opacity": 0.9, "platform.macos.window.opacity": 0.7})));
        let s = find("window.opacity");
        e.set_scope("macos");
        assert_eq!(e.current_value(s), json!(0.7));
        assert_eq!(e.fallback_value(s), json!(0.9));
        assert_eq!(e.default_note(s), "all platforms: 0.9");
        assert_eq!(e.lock(find("window.opacity-cells")), Some(Lock::Scope));
    }

    #[test]
    fn clear_saved_keeps_edits_made_after_the_save_started() {
        let mut e = Editor::new(schema(), state(json!({})));
        let s = find("window.opacity");
        e.edit(s, json!(0.5));
        let saved = e.changes();
        e.edit(s, json!(0.7)); // changed again while the save ran
        e.edit(find("theme"), json!("nord")); // and a new edit
        e.clear_saved(&saved);
        assert_eq!(e.dirty_count(), 2, "both later edits stay pending");
        let saved = e.changes();
        e.clear_saved(&saved);
        assert_eq!(e.dirty_count(), 0);
    }

    #[test]
    fn theme_locks_colors() {
        let mut e = Editor::new(schema(), state(json!({})));
        assert_eq!(e.lock(find("colors.background")), None);
        e.edit(find("theme"), json!("dracula"));
        assert_eq!(e.lock(find("colors.background")), Some(Lock::Theme));
    }

    #[test]
    fn same_ignores_hex_case_and_number_kind() {
        assert!(same(&json!("#ABCDEF"), &json!("#abcdef")));
        assert!(same(&json!(1), &json!(1.0)));
        assert!(same(&json!({"b": 1, "a": 2}), &json!({"a": 2, "b": 1})));
        assert!(!same(&json!("x"), &json!("X")));
    }

    #[test]
    fn describes_values() {
        let s = schema();
        assert_eq!(describe(s, find("window.opacity"), &Value::Null), "Rio decides");
        assert_eq!(describe(s, find("theme"), &json!("")), "empty");
        assert_eq!(describe(s, find("window.opacity-cells"), &json!(false)), "Off");
        assert_eq!(format_number(0.95), "0.95");
        assert_eq!(format_number(800.0), "800");
    }
}
