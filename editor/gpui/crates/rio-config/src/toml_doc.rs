//! A config.toml held as text. Edits splice new text into the exact span of the value they
//! change, so everything else in the file (comments, ordering, spacing) stays byte-identical.
//! Every edit is re-parsed and rejected if the result is invalid TOML.

use serde_json::{Map, Value as Json};
use std::fmt;
use std::ops::Range;
use toml_edit::{Item, Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TomlEditError(pub String);

impl fmt::Display for TomlEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

type Result<T> = std::result::Result<T, TomlEditError>;

/// One `key = value` line, with its full dotted path (array-of-tables segments as `name[i]`).
#[derive(Debug, Clone)]
struct Entry {
    path: Vec<String>,
    /// Start of the line the key is on.
    line_start: usize,
    value_span: Range<usize>,
    value: Json,
    /// Index into `tables`, or None for root keys.
    table: Option<usize>,
}

/// One `[name]` or `[[name]]` header.
#[derive(Debug, Clone)]
struct TableHeader {
    path: Vec<String>,
    is_array: bool,
    header: Range<usize>,
}

/// What a parse produced: every header and key-value, in document order.
#[derive(Debug, Default)]
struct Index {
    entries: Vec<Entry>,
    tables: Vec<TableHeader>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigDocument {
    text: String,
    entries: Vec<Entry>,
    tables: Vec<TableHeader>,
}

impl ConfigDocument {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let index = index(text)
            .map_err(|e| TomlEditError(format!("config.toml is not valid TOML: {e}")))?;
        Ok(Self {
            text: text.to_string(),
            entries: index.entries,
            tables: index.tables,
        })
    }

    #[cfg(test)]
    fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn into_text(self) -> String {
        self.text
    }

    /// All values in the file, flattened to dotted keys. Inline tables are flattened too.
    pub(crate) fn values(&self) -> Map<String, Json> {
        let mut result = Map::new();
        for e in &self.entries {
            let key = e.path.join(".");
            result.insert(key.clone(), e.value.clone());
            if let Json::Object(obj) = &e.value {
                flatten(&mut result, &key, obj);
            }
        }
        result
    }

    fn find(&self, path: &[String]) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// Sets `path` to the TOML literal `literal`.
    pub(crate) fn set(&mut self, path: &[String], literal: &str) -> Result<()> {
        if let Some(existing) = self.find(path) {
            let span = existing.value_span.clone();
            let text = splice(&self.text, span.start, span.end, literal);
            return self.apply(text);
        }
        if let Some(e) = self
            .entries
            .iter()
            .find(|e| e.path.len() < path.len() && path.starts_with(&e.path))
        {
            return Err(TomlEditError(format!(
                "'{}' is written as an inline table in config.toml. Edit '{}' in the file directly.",
                e.path.join("."),
                path.join(".")
            )));
        }
        let text = self.insert(path, literal);
        self.apply(text)
    }

    /// Removes the line that sets `path`. Returns false when it isn't set.
    pub(crate) fn remove(&mut self, path: &[String]) -> Result<bool> {
        let Some(existing) = self.find(path) else {
            return Ok(false);
        };
        // A key-value always starts its line; take the whole line, trailing comment and newline too.
        let start = existing.line_start;
        let from = start.max(existing.value_span.end.saturating_sub(1));
        let end = self.text[from..]
            .find('\n')
            .map_or(self.text.len(), |i| from + i + 1);
        let text = splice(&self.text, start, end, "");
        self.apply(text)?;
        Ok(true)
    }

    /// The raw TOML of every `[[name]]` block (with their sub-tables), or "" when there are none.
    pub(crate) fn get_table_array(&self, name: &[String]) -> Result<String> {
        Ok(match self.table_array_range(name)? {
            None => String::new(),
            Some((start, end)) => format!("{}\n", self.text[start..end].trim_matches('\n')),
        })
    }

    /// Replaces every `[[name]]` block with `raw`; empty removes them.
    pub(crate) fn set_table_array(&mut self, name: &[String], raw: &str) -> Result<()> {
        let raw = raw.replace("\r\n", "\n");
        let raw = raw.trim_matches('\n');
        let check = index(raw).map_err(|e| TomlEditError(format!("Invalid TOML: {e}")))?;
        let dotted = name.join(".");
        if check.tables.iter().any(|t| !t.path.starts_with(name)) {
            return Err(TomlEditError(format!(
                "Only [[{dotted}]] blocks and their sub-tables belong here."
            )));
        }
        if check.entries.iter().any(|e| e.table.is_none()) {
            return Err(TomlEditError(format!("Start with a [[{dotted}]] header.")));
        }

        let mut block = if raw.is_empty() {
            String::new()
        } else {
            format!("{raw}\n")
        };
        let Some((start, end)) = self.table_array_range(name)? else {
            if block.is_empty() {
                return Ok(());
            }
            let text = with_trailing_newline(&self.text);
            let sep = if text.is_empty() { "" } else { "\n" };
            return self.apply(format!("{text}{sep}{block}"));
        };
        if !block.is_empty() {
            // Keep the blank lines that separated the old blocks from what follows.
            let old = &self.text[start..end];
            block = format!("{raw}{}", &old[old.trim_end_matches('\n').len()..]);
            if !block.ends_with('\n') {
                block.push('\n');
            }
        }
        let text = splice(&self.text, start, end, &block);
        self.apply(text)
    }

    fn table_array_range(&self, name: &[String]) -> Result<Option<(usize, usize)>> {
        // The contiguous run of tables from the first [[name]] whose header starts with name.
        let Some(first) = self
            .tables
            .iter()
            .position(|t| t.is_array && t.path == name)
        else {
            return Ok(None);
        };
        let mut last = first;
        while last + 1 < self.tables.len() && self.tables[last + 1].path.starts_with(name) {
            last += 1;
        }
        if self.tables[last + 1..]
            .iter()
            .any(|t| t.is_array && t.path == name)
        {
            return Err(TomlEditError(format!(
                "[[{}]] blocks are split up in the file. Edit them in the file directly.",
                name.join(".")
            )));
        }
        let start = self.line_start(self.tables[first].header.start);
        let end = match self.tables.get(last + 1) {
            Some(next) => self.line_start(next.header.start),
            None => self.text.len(),
        };
        Ok(Some((start, end)))
    }

    fn line_start(&self, offset: usize) -> usize {
        self.text[..offset].rfind('\n').map_or(0, |i| i + 1)
    }

    fn end_of_line(&self, offset: usize) -> usize {
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i + 1)
    }

    fn insert(&self, path: &[String], literal: &str) -> String {
        // Prefer the deepest existing [table] whose name is a prefix of the key.
        let best = self
            .tables
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.is_array && t.path.len() < path.len() && path.starts_with(&t.path))
            .fold(None::<(usize, &TableHeader)>, |best, (i, t)| match best {
                Some((_, b)) if b.path.len() >= t.path.len() => best,
                _ => Some((i, t)),
            });
        if let Some((index, table)) = best {
            let line = format!("{} = {literal}", format_key(&path[table.path.len()..]));
            return self.insert_into_table(index, &line);
        }

        let parent = &path[..path.len() - 1];
        let root_dotted = !parent.is_empty()
            && self
                .entries
                .iter()
                .any(|e| e.table.is_none() && e.path.len() > 1 && e.path[0] == path[0]);
        if parent.is_empty() || root_dotted {
            return self.insert_at_root(&format!("{} = {literal}", format_key(path)));
        }

        let text = with_trailing_newline(&self.text);
        let sep = if text.is_empty() || text.ends_with("\n\n") {
            ""
        } else {
            "\n"
        };
        format!(
            "{text}{sep}[{}]\n{} = {literal}\n",
            format_key(parent),
            format_key(&path[path.len() - 1..])
        )
    }

    fn insert_into_table(&self, table: usize, line: &str) -> String {
        let last_value_end = self
            .entries
            .iter()
            .filter(|e| e.table == Some(table))
            .map(|e| e.value_span.end)
            .max();
        let offset = match last_value_end {
            Some(end) => self.end_of_line(end),
            None => self.end_of_line(self.tables[table].header.end),
        };
        self.insert_line(offset, line)
    }

    fn insert_at_root(&self, line: &str) -> String {
        let last_root = self
            .entries
            .iter()
            .filter(|e| e.table.is_none())
            .map(|e| e.value_span.end)
            .max();
        if let Some(end) = last_root {
            return self.insert_line(self.end_of_line(end), line);
        }
        if self.tables.is_empty() {
            return format!("{}{line}\n", with_trailing_newline(&self.text));
        }
        // Root keys must come before the first table: put them after the file's leading comment block.
        let text = &self.text;
        let mut pos = 0;
        while pos < text.len() {
            let nl = text[pos..].find('\n').map(|i| pos + i);
            let line_text = text[pos..nl.unwrap_or(text.len())].trim();
            if !line_text.is_empty() && !line_text.starts_with('#') {
                break;
            }
            match nl {
                Some(nl) => pos = nl + 1,
                None => {
                    pos = text.len();
                    break;
                }
            }
        }
        let blank_after = pos < text.len() && text.as_bytes()[pos] != b'\n';
        let insert = format!("{line}\n{}", if blank_after { "\n" } else { "" });
        splice(text, pos, pos, &insert)
    }

    fn insert_line(&self, offset: usize, line: &str) -> String {
        // offset is just after a newline (or end of text without one).
        if offset == self.text.len() && !self.text.is_empty() && !self.text.ends_with('\n') {
            return format!("{}\n{line}\n", self.text);
        }
        splice(&self.text, offset, offset, &format!("{line}\n"))
    }

    fn apply(&mut self, text: String) -> Result<()> {
        let index = index(&text)
            .map_err(|e| TomlEditError(format!("Edit would make config.toml invalid: {e}")))?;
        self.text = text;
        self.entries = index.entries;
        self.tables = index.tables;
        Ok(())
    }
}

fn splice(text: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() - (end - start) + replacement.len());
    out.push_str(&text[..start]);
    out.push_str(replacement);
    out.push_str(&text[end..]);
    out
}

fn with_trailing_newline(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

fn flatten(into: &mut Map<String, Json>, prefix: &str, obj: &Map<String, Json>) {
    for (k, v) in obj {
        let key = format!("{prefix}.{k}");
        into.insert(key.clone(), v.clone());
        if let Json::Object(child) = v {
            flatten(into, &key, child);
        }
    }
}

/// Keys as written in TOML: bare when possible, quoted otherwise.
pub(crate) fn format_key(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| if is_bare(p) { p.clone() } else { quote(p) })
        .collect::<Vec<_>>()
        .join(".")
}

fn is_bare(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A TOML basic string. Control characters use `\uXXXX`: `\e` is TOML 1.1 only, and Rio may parse TOML 1.0.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A TOML float: always with a decimal point (`1.0`), never with an exponent.
pub(crate) fn float(d: f64) -> String {
    if d.is_nan() {
        return "nan".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let r = format!("{d}");
    if r.contains('.') { r } else { format!("{r}.0") }
}

// ---- Parsing into the index ----

fn index(text: &str) -> std::result::Result<Index, String> {
    let doc = toml_edit::Document::parse(text).map_err(|e| describe(text, &e))?;
    let mut sections: Vec<Section> = Vec::new();
    let mut root = Vec::new();
    collect(
        text,
        doc.as_table(),
        &[],
        &[],
        None,
        &mut sections,
        &mut root,
    )?;

    // Document order, then array-of-tables numbering by header name.
    let mut order: Vec<usize> = (0..sections.len()).collect();
    order.sort_by_key(|&i| sections[i].0.header.start);
    let mut index = Index::default();
    let mut array_counts: Vec<(Vec<String>, usize)> = Vec::new();
    root.sort_by_key(|kv| kv.1);
    for (rel, line_start, value_span, value) in root {
        index.entries.push(Entry {
            path: rel,
            line_start,
            value_span,
            value,
            table: None,
        });
    }
    let mut slots: Vec<Option<Section>> = sections.into_iter().map(Some).collect();
    for i in order {
        let Some((header, mut kvs)) = slots[i].take() else {
            continue;
        };
        let mut prefix = header.path.clone();
        if header.is_array {
            let n = match array_counts.iter_mut().find(|(p, _)| *p == header.path) {
                Some((_, n)) => {
                    *n += 1;
                    *n - 1
                }
                None => {
                    array_counts.push((header.path.clone(), 1));
                    0
                }
            };
            if let Some(last) = prefix.last_mut() {
                *last = format!("{last}[{n}]");
            }
        }
        let table = index.tables.len();
        index.tables.push(header);
        kvs.sort_by_key(|kv| kv.1);
        for (rel, line_start, value_span, value) in kvs {
            let mut path = prefix.clone();
            path.extend(rel);
            index.entries.push(Entry {
                path,
                line_start,
                value_span,
                value,
                table: Some(table),
            });
        }
    }
    Ok(index)
}

type Kv = (Vec<String>, usize, Range<usize>, Json);
type Section = (TableHeader, Vec<Kv>);

/// Walks a table's items. `plain` is the table's path from the root (array names without
/// an index); `rel` is the dotted-key path inside the current section.
fn collect(
    text: &str,
    table: &Table,
    plain: &[String],
    rel: &[String],
    section: Option<usize>,
    sections: &mut Vec<Section>,
    root: &mut Vec<Kv>,
) -> std::result::Result<(), String> {
    for (key, item) in table.iter() {
        let plain_k = [plain, &[key.to_string()]].concat();
        let rel_k = [rel, &[key.to_string()]].concat();
        match item {
            Item::None => {}
            Item::Value(value) => {
                let key_start = table.key(key).and_then(|k| k.span()).map(|s| s.start);
                let (Some(key_start), Some(value_span)) = (key_start, value.span()) else {
                    return Err(format!("couldn't locate '{}' in the file", rel_k.join(".")));
                };
                let line_start = text[..key_start].rfind('\n').map_or(0, |i| i + 1);
                let kv = (rel_k, line_start, value_span, to_json(text, value));
                match section {
                    Some(s) => sections[s].1.push(kv),
                    None => root.push(kv),
                }
            }
            Item::Table(t) if t.is_dotted() => {
                collect(text, t, &plain_k, &rel_k, section, sections, root)?
            }
            Item::Table(t) if t.is_implicit() => {
                collect(text, t, &plain_k, &[], section, sections, root)?
            }
            Item::Table(t) => {
                let header = header(t, &plain_k, false)?;
                sections.push((header, Vec::new()));
                let s = sections.len() - 1;
                collect(text, t, &plain_k, &[], Some(s), sections, root)?;
            }
            Item::ArrayOfTables(array) => {
                for t in array.iter() {
                    let header = header(t, &plain_k, true)?;
                    sections.push((header, Vec::new()));
                    let s = sections.len() - 1;
                    collect(text, t, &plain_k, &[], Some(s), sections, root)?;
                }
            }
        }
    }
    Ok(())
}

fn header(t: &Table, path: &[String], is_array: bool) -> std::result::Result<TableHeader, String> {
    let header = t
        .span()
        .ok_or_else(|| format!("couldn't locate [{}] in the file", path.join(".")))?;
    Ok(TableHeader {
        path: path.to_vec(),
        is_array,
        header,
    })
}

fn to_json(text: &str, value: &Value) -> Json {
    match value {
        Value::String(s) => Json::String(s.value().clone()),
        Value::Integer(i) => Json::from(*i.value()),
        Value::Float(f) => Json::from(*f.value()),
        Value::Boolean(b) => Json::Bool(*b.value()),
        Value::Datetime(d) => {
            let raw = d.span().and_then(|s| text.get(s)).map(str::trim);
            Json::String(raw.map_or_else(|| d.value().to_string(), str::to_string))
        }
        Value::Array(a) => Json::Array(a.iter().map(|v| to_json(text, v)).collect()),
        Value::InlineTable(t) => {
            let mut obj = Map::new();
            for (k, v) in t.iter() {
                obj.insert(k.to_string(), to_json(text, v));
            }
            Json::Object(obj)
        }
    }
}

/// A one-line parse error: "line 3, column 8: invalid table header (expected `.`, `]`)".
fn describe(text: &str, error: &toml_edit::TomlError) -> String {
    let message = error.message().trim().replace('\n', "; ");
    match error.span() {
        Some(span) => {
            let offset = span.start.min(text.len());
            let before = text.get(..offset).unwrap_or(text);
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            format!("line {line}, column {column}: {message}")
        }
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# See the full configuration reference: https://rioterm.com/docs/config

theme = "dracula"   # my theme

[editor]
program = "/opt/homebrew/bin/zed"
args = []

# fonts below
[fonts]
family = "FiraCode Nerd Font Mono"

[[hints.rules]]
regex = "https?://\\S+"
hyperlinks = true

[hints.rules.action]
command = "open"

[window]
opacity = 0.9 # translucent"#;

    fn p(dotted: &str) -> Vec<String> {
        dotted.split('.').map(str::to_string).collect()
    }

    fn doc(text: &str) -> ConfigDocument {
        ConfigDocument::parse(text).expect("parses")
    }

    #[test]
    fn parse_then_no_edits_is_byte_identical() {
        assert_eq!(doc(SAMPLE).text(), SAMPLE);
    }

    #[test]
    fn replacing_a_value_keeps_comments_and_spacing() {
        let mut d = doc(SAMPLE);
        d.set(&p("window.opacity"), "0.75").unwrap();
        assert_eq!(
            d.text(),
            SAMPLE.replace(
                "opacity = 0.9 # translucent",
                "opacity = 0.75 # translucent"
            )
        );
    }

    #[test]
    fn replacing_root_value_keeps_trailing_comment() {
        let mut d = doc(SAMPLE);
        d.set(&p("theme"), "\"nord\"").unwrap();
        assert!(d.text().contains("theme = \"nord\"   # my theme"));
    }

    #[test]
    fn new_key_goes_into_existing_table_after_its_last_key() {
        let mut d = doc(SAMPLE);
        d.set(&p("fonts.size"), "16.0").unwrap();
        assert!(
            d.text()
                .contains("[fonts]\nfamily = \"FiraCode Nerd Font Mono\"\nsize = 16.0\n")
        );
    }

    #[test]
    fn nested_key_uses_dotted_key_in_nearest_parent_table() {
        let mut d = doc(SAMPLE);
        d.set(&p("fonts.regular.family"), "\"Iosevka\"").unwrap();
        assert!(d.text().contains("regular.family = \"Iosevka\""));
        assert_eq!(d.values()["fonts.regular.family"], "Iosevka");
    }

    #[test]
    fn new_table_is_appended_when_missing() {
        let mut d = doc(SAMPLE);
        d.set(&p("cursor.shape"), "\"beam\"").unwrap();
        assert!(
            d.text()
                .ends_with("opacity = 0.9 # translucent\n\n[cursor]\nshape = \"beam\"\n")
        );
    }

    #[test]
    fn new_root_key_goes_after_existing_root_keys() {
        let mut d = doc(SAMPLE);
        d.set(&p("copy-on-select"), "true").unwrap();
        assert!(
            d.text()
                .contains("theme = \"dracula\"   # my theme\ncopy-on-select = true\n")
        );
    }

    #[test]
    fn new_root_key_without_root_keys_goes_before_first_table() {
        let mut d = doc("# header\n\n[window]\nopacity = 1.0\n");
        d.set(&p("line-height"), "1.2").unwrap();
        assert_eq!(
            d.text(),
            "# header\n\nline-height = 1.2\n\n[window]\nopacity = 1.0\n"
        );
    }

    #[test]
    fn remove_deletes_the_whole_line_including_comment() {
        let mut d = doc(SAMPLE);
        assert!(d.remove(&p("window.opacity")).unwrap());
        assert!(!d.text().contains("opacity"));
        assert!(d.text().ends_with("[window]\n"));
        assert!(!d.remove(&p("window.opacity")).unwrap());
    }

    #[test]
    fn remove_takes_a_dotted_key_line_and_keeps_its_neighbours() {
        let mut d = doc("[fonts]\nregular.family = \"A\" # x\nregular.style = \"B\"\nsize = 1.0\n");
        assert!(d.remove(&p("fonts.regular.style")).unwrap());
        assert_eq!(
            d.text(),
            "[fonts]\nregular.family = \"A\" # x\nsize = 1.0\n"
        );
    }

    #[test]
    fn values_flattens_tables_and_arrays_of_tables() {
        let values = doc(SAMPLE).values();
        assert_eq!(values["editor.program"], "/opt/homebrew/bin/zed");
        assert_eq!(values["window.opacity"], 0.9);
        assert_eq!(values["hints.rules[0].hyperlinks"], true);
        assert_eq!(values["hints.rules.action.command"], "open");
    }

    #[test]
    fn inline_table_values_are_flattened_but_not_edited_in_place() {
        let mut d = doc("[fonts]\nregular = { family = \"Iosevka\", style = \"Light\" }\n");
        assert_eq!(d.values()["fonts.regular.style"], "Light");
        let err = d.set(&p("fonts.regular.style"), "\"Bold\"").unwrap_err();
        assert!(err.0.contains("inline table"), "{err}");
    }

    #[test]
    fn table_array_round_trips_and_can_be_replaced() {
        let mut d = doc(SAMPLE);
        let raw = d.get_table_array(&p("hints.rules")).unwrap();
        assert!(raw.starts_with("[[hints.rules]]"));
        assert!(raw.contains("[hints.rules.action]"));
        assert!(!raw.contains("[window]"));

        d.set_table_array(&p("hints.rules"), "[[hints.rules]]\nregex = \"x\"\n")
            .unwrap();
        assert!(
            d.text()
                .contains("[[hints.rules]]\nregex = \"x\"\n\n[window]")
        );
        assert!(!d.text().contains("hyperlinks"));

        d.set_table_array(&p("hints.rules"), "").unwrap();
        assert!(!d.text().contains("hints.rules"));
    }

    #[test]
    fn table_array_rejects_foreign_tables() {
        let mut d = doc(SAMPLE);
        assert!(
            d.set_table_array(&p("hints.rules"), "[window]\nopacity = 1.0\n")
                .is_err()
        );
        assert!(
            d.set_table_array(&p("hints.rules"), "regex = \"x\"\n")
                .is_err()
        );
    }

    #[test]
    fn split_table_arrays_are_refused() {
        let d = doc("[[hints.rules]]\na = 1\n[window]\nb = 1\n[[hints.rules]]\na = 2\n");
        let err = d.get_table_array(&p("hints.rules")).unwrap_err();
        assert!(err.0.contains("split up"), "{err}");
        let values = d.values();
        assert_eq!(values["hints.rules[1].a"], 2);
    }

    #[test]
    fn edits_producing_invalid_toml_are_rejected_and_text_is_unchanged() {
        let mut d = doc(SAMPLE);
        assert!(d.set(&p("window.opacity"), "\"unterminated").is_err());
        assert_eq!(d.text(), SAMPLE);
    }

    #[test]
    fn invalid_file_reports_parse_error() {
        let err = ConfigDocument::parse("[window\nopacity = ").unwrap_err();
        assert!(
            err.0.starts_with("config.toml is not valid TOML: line 1"),
            "{err}"
        );
    }

    #[test]
    fn empty_file_gets_table_without_leading_blank_line() {
        let mut d = doc("");
        d.set(&p("window.opacity"), "0.5").unwrap();
        assert_eq!(d.text(), "[window]\nopacity = 0.5\n");
    }

    #[test]
    fn file_without_trailing_newline_still_appends_cleanly() {
        let mut d = doc("[window]\nopacity = 1.0");
        d.set(&p("window.mode"), "\"Maximized\"").unwrap();
        assert_eq!(d.text(), "[window]\nopacity = 1.0\nmode = \"Maximized\"\n");
    }

    #[test]
    fn platform_override_goes_into_nested_table() {
        let mut d = doc(SAMPLE);
        d.set(&p("platform.macos.window.opacity"), "0.8").unwrap();
        assert!(
            d.text()
                .ends_with("[platform.macos.window]\nopacity = 0.8\n")
        );
        assert_eq!(d.values()["platform.macos.window.opacity"], 0.8);
    }

    #[test]
    fn root_dotted_keys_stay_dotted_at_the_root() {
        let mut d = doc("window.mode = \"Maximized\"\n\n[fonts]\nsize = 1.0\n");
        d.set(&p("window.opacity"), "0.5").unwrap();
        assert_eq!(
            d.text(),
            "window.mode = \"Maximized\"\nwindow.opacity = 0.5\n\n[fonts]\nsize = 1.0\n"
        );
    }

    #[test]
    fn multi_line_values_are_replaced_and_removed_whole() {
        let text = "[shell]\nargs = [\n  \"--login\",\n] # c\nprogram = \"zsh\"\n";
        let mut d = doc(text);
        d.set(&p("shell.args"), "[]").unwrap();
        assert_eq!(d.text(), "[shell]\nargs = [] # c\nprogram = \"zsh\"\n");
        let mut d = doc(text);
        d.remove(&p("shell.args")).unwrap();
        assert_eq!(d.text(), "[shell]\nprogram = \"zsh\"\n");
    }

    #[test]
    fn quoted_keys_and_non_ascii_text_keep_byte_offsets_right() {
        let mut d = doc("title = \"héllo ✓\"\n[\"a b\"]\nc = 1\n");
        assert_eq!(d.values()["a b.c"], 1);
        d.set(&p("title"), "\"x\"").unwrap();
        d.set(&["a b".to_string(), "d".to_string()], "2").unwrap();
        assert_eq!(d.text(), "title = \"x\"\n[\"a b\"]\nc = 1\nd = 2\n");
    }

    #[test]
    fn toml_1_1_input_is_read_but_never_written() {
        // toml_edit 0.25 parses TOML 1.1, so hand-written 1.1 files load; edits only emit 1.0.
        let mut d = doc("a = \"\\e\"\nb = { x = 1,\n  y = 2 }\n");
        assert_eq!(d.values()["a"], "\u{1b}");
        assert_eq!(d.values()["b.y"], 2);
        d.set(&p("a"), &quote("\u{1b}")).unwrap();
        assert!(d.text().starts_with("a = \"\\u001B\"\n"));
    }

    #[test]
    fn quote_uses_unicode_escapes_for_control_characters() {
        assert_eq!(quote("a\u{1b}b\"\\\n"), "\"a\\u001Bb\\\"\\\\\\n\"");
    }

    #[test]
    fn floats_always_have_a_decimal_point() {
        assert_eq!(float(1.0), "1.0");
        assert_eq!(float(0.5), "0.5");
        assert_eq!(float(1e20), "100000000000000000000.0");
        assert_eq!(float(-0.0), "-0.0");
    }
}
