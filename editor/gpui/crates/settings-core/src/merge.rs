//! What every terminal's schema merge shares: diagnostics, the merged-once cache, label
//! humanizing and the lenient JSON accessors.

use crate::Schema;
use serde_json::{Map, Value};
use std::fmt;
use std::sync::OnceLock;

/// Something wrong or drifting between a terminal's generated source and the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDiagnostic {
    /// e.g. "RIO001", "GHO003".
    pub id: &'static str,
    pub is_error: bool,
    pub message: String,
}

pub fn error(id: &'static str, message: String) -> SchemaDiagnostic {
    SchemaDiagnostic { id, is_error: true, message }
}

pub fn warning(id: &'static str, message: String) -> SchemaDiagnostic {
    SchemaDiagnostic { id, is_error: false, message }
}

impl fmt::Display for SchemaDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = if self.is_error { "error" } else { "warning" };
        write!(f, "{level} {}: {}", self.id, self.message)
    }
}

pub type Merged = (Schema, Vec<SchemaDiagnostic>);

/// A terminal's schema, merged from its embedded files the first time it's asked for.
pub struct MergedSchema {
    cell: OnceLock<Merged>,
    merge: fn() -> Merged,
}

impl MergedSchema {
    pub const fn new(merge: fn() -> Merged) -> Self {
        Self { cell: OnceLock::new(), merge }
    }

    fn get(&self) -> &Merged {
        self.cell.get_or_init(self.merge)
    }

    pub fn schema(&self) -> Schema {
        self.get().0.clone()
    }

    pub fn diagnostics(&self) -> Vec<SchemaDiagnostic> {
        self.get().1.clone()
    }
}

/// "font-family-bold" or "blinking_interval" → "Font family bold", "Blinking interval".
pub fn humanize(key: &str) -> String {
    let words: Vec<&str> = key.split(['-', '_']).filter(|w| !w.is_empty()).collect();
    let Some((first, rest)) = words.split_first() else { return key.to_string() };
    let mut chars = first.chars();
    let mut out: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    for w in rest {
        out.push(' ');
        out.push_str(w);
    }
    out
}

pub type Obj = Map<String, Value>;

pub fn obj<'a>(o: &'a Obj, key: &str) -> Option<&'a Obj> {
    o.get(key)?.as_object()
}

pub fn string(o: &Obj, key: &str) -> Option<String> {
    o.get(key)?.as_str().map(str::to_string)
}

pub fn number(o: &Obj, key: &str) -> Option<f64> {
    o.get(key)?.as_f64()
}

/// False when missing or not a bool.
pub fn boolean(o: &Obj, key: &str) -> bool {
    o.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The string items of an array (others skipped); None when the key isn't an array.
pub fn strings(o: &Obj, key: &str) -> Option<Vec<String>> {
    Some(o.get(key)?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
}

/// The object items of an array (others skipped); none when the key isn't an array.
pub fn objects<'a>(o: &'a Obj, key: &str) -> impl Iterator<Item = &'a Obj> {
    o.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object)
}

#[cfg(test)]
mod tests {
    use super::humanize;

    #[test]
    fn humanize_capitalises_the_first_word_only() {
        assert_eq!(humanize("font-family-bold"), "Font family bold");
        assert_eq!(humanize("blinking_interval"), "Blinking interval");
        assert_eq!(humanize("--"), "--");
    }
}
