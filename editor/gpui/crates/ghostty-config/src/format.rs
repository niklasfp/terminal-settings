//! Ghostty's config format: one `key = value` per line, comments on their own lines, later lines
//! win, an empty value resets to the default, and some keys repeat. Edits replace whole lines and
//! leave every other line (comments included) untouched.

use crate::values;
use serde_json::{Map, Value};
use settings_core::{
    Change, ConfigFormat, FieldErrors, FormatError, ParsedConfig, Schema, Setting, SettingType,
};
use std::collections::HashSet;

/// Ghostty's line-based config (`config.ghostty`).
#[derive(Debug, Default, Clone, Copy)]
pub struct GhosttyFormat;

/// A `key = value` line: its index and the value as Ghostty reads it.
struct Line<'a> {
    index: usize,
    key: &'a str,
    value: &'a str,
}

/// The file's lines without their terminators ("" has none, a final newline adds no empty line).
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let unix = text.replace("\r\n", "\n");
    let mut lines: Vec<String> = unix.split('\n').map(str::to_string).collect();
    // Only the final newline ends the last line; blank lines before it are content and stay.
    if unix.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// The key and raw value of a `key = value` line; None for comments, blank and other lines.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let first = t.chars().next()?;
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return None;
    }
    let key_len = t
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .unwrap_or(t.len());
    let (key, rest) = t.split_at(key_len);
    let value = rest.trim_start().strip_prefix('=')?;
    Some((key, value))
}

fn scan(lines: &[String]) -> Vec<Line<'_>> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, l)| {
            key_value(l).map(|(key, raw)| Line {
                index,
                key,
                value: unquote(raw),
            })
        })
        .collect()
}

/// Ghostty trims spaces around values; a value in double quotes keeps its inner text exactly.
fn unquote(raw: &str) -> &str {
    let v = raw.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

fn quote(v: &str) -> String {
    if v.is_empty() || v != v.trim() || v.starts_with('"') {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

/// Edited as a list of values even when the key isn't marked repeatable.
fn is_list(s: &Setting) -> bool {
    matches!(
        s.ty,
        SettingType::Strings | SettingType::Fonts | SettingType::Records
    )
}

impl ConfigFormat for GhosttyFormat {
    fn parse(&self, text: &str, schema: &Schema) -> Result<ParsedConfig, String> {
        let lines = split_lines(text);
        let scanned = scan(&lines);
        let mut parsed = ParsedConfig::default();
        let mut keys: Vec<&str> = Vec::new();
        let mut unique = HashSet::new();
        for l in &scanned {
            if unique.insert(l.key) {
                keys.push(l.key);
            }
        }
        for key in keys {
            let Some(setting) = schema.find(key) else {
                parsed.notices.push(format!(
                    "Unknown key '{key}' in the file; it is left untouched."
                ));
                continue;
            };
            let group: Vec<&str> = scanned
                .iter()
                .filter(|l| l.key == key)
                .map(|l| l.value)
                .collect();
            // An empty value resets the key (and clears a repeatable list); only lines after it count.
            let effective = match group.iter().rposition(|v| v.is_empty()) {
                Some(reset) => &group[reset + 1..],
                None => &group[..],
            };
            if setting.repeatable {
                parsed
                    .values
                    .insert(key.to_string(), to_json_list(setting, effective));
            } else if let Some(last) = effective.last() {
                let value = if is_list(setting) {
                    // A single-valued key edited with a list widget (e.g. a comma-separated list).
                    to_json_list(setting, &[last])
                } else {
                    to_json(setting, last)
                };
                parsed.values.insert(key.to_string(), value);
            }
        }
        if parsed.values.contains_key("config-file") {
            parsed.notices.push(
                "This file includes other files with config-file. Values set there aren't shown here and may override these."
                    .into(),
            );
        }
        Ok(parsed)
    }

    fn apply(
        &self,
        text: &str,
        changes: &[Change<'_>],
        _scope: &str,
    ) -> Result<String, FormatError> {
        let mut lines = split_lines(text);
        let mut errors = FieldErrors::new();
        let mut appended = false;

        for change in changes {
            let setting = change.setting;
            let new_values = if change.value.is_null() {
                Vec::new()
            } else {
                match values::to_lines(setting, &change.value) {
                    Ok(v) => v,
                    Err(message) => {
                        errors.insert(setting.key.clone(), message);
                        continue;
                    }
                }
            };
            if !setting.repeatable && new_values.len() > 1 {
                errors.insert(
                    setting.key.clone(),
                    "This setting takes a single value.".into(),
                );
                continue;
            }

            let existing: Vec<usize> = scan(&lines)
                .iter()
                .filter(|l| l.key == setting.key)
                .map(|l| l.index)
                .collect();
            if !setting.repeatable
                && let ([.., last], [value]) = (existing.as_slice(), new_values.as_slice())
            {
                // Scalar: rewrite the line that wins (the last one), keep earlier lines as they are.
                lines[*last] = format!("{} = {}", setting.key, quote(value));
                continue;
            }

            // Reset, or a list: replace all of its lines at the position of the first one.
            let mut at = existing.first().copied().unwrap_or(lines.len());
            for &i in existing.iter().rev() {
                lines.remove(i);
            }
            if existing.is_empty() && !new_values.is_empty() {
                // Keys new to the file go at the end, as one block after a blank line.
                if !appended && lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    lines.push(String::new());
                    at = lines.len();
                }
                appended = true;
            }
            let at = at.min(lines.len());
            lines.splice(
                at..at,
                new_values
                    .iter()
                    .map(|v| format!("{} = {}", setting.key, quote(v))),
            );
        }

        if !errors.is_empty() {
            return Err(FormatError::Invalid(errors));
        }
        Ok(lines.iter().map(|l| format!("{l}\n")).collect())
    }
}

fn to_json_list(s: &Setting, lines: &[&str]) -> Value {
    if let (SettingType::Records, Some([first, second, ..])) = (s.ty, s.fields.as_deref()) {
        return Value::Array(
            lines
                .iter()
                .map(|line| {
                    let (a, b) = match line.split_once('=') {
                        Some((a, b)) => (a.trim(), b.trim()),
                        None => (*line, ""),
                    };
                    let mut obj = Map::new();
                    obj.insert(first.key.clone(), Value::String(a.to_string()));
                    obj.insert(second.key.clone(), Value::String(b.to_string()));
                    Value::Object(obj)
                })
                .collect(),
        );
    }
    Value::Array(
        lines
            .iter()
            .map(|l| Value::String((*l).to_string()))
            .collect(),
    )
}

fn to_json(s: &Setting, v: &str) -> Value {
    match s.ty {
        SettingType::Bool if v == "true" || v == "false" => Value::Bool(v == "true"),
        SettingType::Int => match v.trim().parse::<i64>() {
            Ok(n) => Value::from(n),
            Err(_) => Value::String(v.to_string()),
        },
        SettingType::Float => match v.trim().parse::<f64>() {
            Ok(d) if d.is_finite() && looks_numeric(v) => Value::from(d),
            _ => Value::String(v.to_string()),
        },
        SettingType::Flags => {
            // "a,no-b" only mentions some flags; the rest keep their defaults.
            let known = s.values.as_deref().unwrap_or_default();
            let mut on: HashSet<String> = s
                .default
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect();
            for part in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part {
                    "true" => on.extend(known.iter().cloned()),
                    "false" => on.clear(),
                    _ => match part.strip_prefix("no-") {
                        Some(flag) => {
                            on.remove(flag);
                        }
                        None => {
                            on.insert(part.to_string());
                        }
                    },
                }
            }
            Value::Array(
                known
                    .iter()
                    .filter(|f| on.contains(*f))
                    .map(|f| Value::String(f.clone()))
                    .collect(),
            )
        }
        SettingType::FontStyle if v == "false" => Value::Bool(false),
        SettingType::Color => match values::hex_color(v) {
            Some(hex) => Value::String(hex),
            None => Value::String(v.to_string()),
        },
        _ => Value::String(v.to_string()),
    }
}

/// Rust also reads "inf" and "nan" as floats; Ghostty's numbers are plain decimals.
fn looks_numeric(v: &str) -> bool {
    v.trim()
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SAMPLE: &str = "# Danish @ / [ / ] / | are typed with Option.
macos-option-as-alt = right

# SSH helpers
shell-integration-features = ssh-env,ssh-terminfo

font-family = JetBrains Mono
font-family = Symbols Nerd Font
keybind = super+t=new_tab
background-opacity = 0.9
background-opacity = 0.8

";

    fn schema() -> Schema {
        crate::schema()
    }

    fn parse(text: &str) -> ParsedConfig {
        GhosttyFormat.parse(text, &schema()).expect("parses")
    }

    fn apply_all(text: &str, changes: &[(&str, Value)]) -> Result<String, FormatError> {
        let schema = schema();
        let changes: Vec<Change<'_>> = changes
            .iter()
            .map(|(k, v)| Change {
                setting: schema
                    .find(k)
                    .unwrap_or_else(|| panic!("{k} in the schema")),
                value: v.clone(),
            })
            .collect();
        GhosttyFormat.apply(text, &changes, "")
    }

    fn apply(text: &str, key: &str, value: Value) -> String {
        apply_all(text, &[(key, value)]).unwrap_or_else(|e| panic!("apply failed: {e:?}"))
    }

    #[test]
    fn parse_reads_last_value_lists_flags_and_records() {
        let v = parse(SAMPLE).values;
        assert_eq!(v["background-opacity"], json!(0.8));
        assert_eq!(v["macos-option-as-alt"], json!("right"));
        assert_eq!(
            v["font-family"],
            json!(["JetBrains Mono", "Symbols Nerd Font"])
        );
        // Flags merge with Ghostty's defaults (cursor, title, path are on by default).
        let mut flags: Vec<&str> = v["shell-integration-features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        flags.sort_unstable();
        assert_eq!(
            flags,
            ["cursor", "path", "ssh-env", "ssh-terminfo", "title"]
        );
        assert_eq!(
            v["keybind"],
            json!([{"trigger": "super+t", "action": "new_tab"}])
        );
    }

    #[test]
    fn parse_reports_unknown_keys_and_includes() {
        let parsed = parse("bogus-key = 1\nconfig-file = ?extra.ghostty\nfont-size = x\n");
        assert_eq!(
            parsed.notices,
            [
                "Unknown key 'bogus-key' in the file; it is left untouched.",
                "This file includes other files with config-file. Values set there aren't shown here and may override these."
            ]
        );
        // A value the widget can't show stays text, so nothing is lost.
        assert_eq!(parsed.values["font-size"], json!("x"));
    }

    #[test]
    fn parse_normalises_colors_and_reads_quoted_values() {
        let v = parse("background = 1E1E2E\ntitle = \"  spaced  \"\nfont-thicken = true\n").values;
        assert_eq!(v["background"], json!("#1e1e2e"));
        assert_eq!(v["title"], json!("  spaced  "));
        assert_eq!(v["font-thicken"], json!(true));
    }

    #[test]
    fn crlf_and_indented_lines_parse() {
        let v = parse("  font-size=14\r\n# font-size = 20\r\n").values;
        assert_eq!(v["font-size"], json!(14.0));
    }

    #[test]
    fn scalar_edit_rewrites_the_winning_line_only() {
        let after = apply(SAMPLE, "background-opacity", json!(0.5));
        assert!(after.contains("background-opacity = 0.9\nbackground-opacity = 0.5\n"));
        assert!(after.contains("# Danish @ / [ / ] / | are typed with Option.\n"));
    }

    #[test]
    fn trailing_blank_lines_are_kept() {
        let s = schema();
        let setting = s.find("font-size").unwrap();
        let out = GhosttyFormat
            .apply("font-size = 13\n\n\n", &[Change { setting, value: serde_json::json!(14) }], "")
            .unwrap();
        assert_eq!(out, "font-size = 14\n\n\n");
    }

    #[test]
    fn new_key_is_appended_after_a_blank_line() {
        let after = apply("# mine\nfont-size = 13\n", "cursor-style", json!("bar"));
        assert_eq!(after, "# mine\nfont-size = 13\n\ncursor-style = bar\n");
    }

    #[test]
    fn several_new_keys_form_one_block() {
        let after = apply_all(
            "font-size = 13\n",
            &[("cursor-style", json!("bar")), ("theme", json!("Nord"))],
        )
        .unwrap();
        assert_eq!(
            after,
            "font-size = 13\n\ncursor-style = bar\ntheme = Nord\n"
        );
    }

    #[test]
    fn new_key_in_an_empty_file_has_no_blank_line() {
        assert_eq!(apply("", "font-size", json!(13)), "font-size = 13\n");
    }

    #[test]
    fn reset_removes_every_line_for_the_key() {
        let after = apply(SAMPLE, "background-opacity", Value::Null);
        assert!(!after.contains("background-opacity"));
        assert!(after.contains("keybind = super+t=new_tab\n"));
    }

    #[test]
    fn repeatable_list_is_replaced_in_place() {
        let after = apply(
            SAMPLE,
            "font-family",
            json!(["Iosevka", "Symbols Nerd Font"]),
        );
        assert!(after.contains(
            "\nfont-family = Iosevka\nfont-family = Symbols Nerd Font\nkeybind = super+t=new_tab\n"
        ));
    }

    #[test]
    fn emptying_a_list_removes_its_lines() {
        let after = apply(SAMPLE, "font-family", json!([]));
        assert!(!after.contains("font-family"));
    }

    #[test]
    fn flags_are_written_out_in_full() {
        let after = apply(
            SAMPLE,
            "shell-integration-features",
            json!(["cursor", "ssh-env"]),
        );
        assert!(after.contains(
            "shell-integration-features = cursor,no-sudo,no-title,ssh-env,no-ssh-terminfo,no-path\n"
        ));
    }

    #[test]
    fn records_round_trip_as_trigger_equals_action() {
        let after = apply(
            SAMPLE,
            "keybind",
            json!([{"trigger":"super+t","action":"new_tab"},{"trigger":"super+d","action":"new_split:right"}]),
        );
        assert!(after.contains("keybind = super+t=new_tab\nkeybind = super+d=new_split:right\n"));
    }

    #[test]
    fn values_with_leading_spaces_are_quoted_and_read_back_exactly() {
        let after = apply("", "selection-word-chars", json!(" \t'\""));
        assert_eq!(after, "selection-word-chars = \" \t'\"\"\n");
        assert_eq!(
            parse(&after).values["selection-word-chars"],
            json!(" \t'\"")
        );
    }

    #[test]
    fn empty_value_line_resets_earlier_lines() {
        let v = parse("font-family = A\nfont-family = \nfont-family = B\n").values;
        assert_eq!(v["font-family"], json!(["B"]));
        let v = parse("font-size = 14\nfont-size =\n").values;
        assert!(!v.contains_key("font-size"));
    }

    #[test]
    fn rejects_invalid_values() {
        for (key, value) in [
            ("background-opacity", json!(1.5)),
            ("cursor-style", json!("triangle")),
            ("background", json!("#12")),
            ("shell-integration-features", json!(["bogus"])),
            ("keybind", json!([{"trigger": "super+t"}])),
        ] {
            assert!(
                matches!(apply_all("", &[(key, value.clone())]), Err(FormatError::Invalid(e)) if e.contains_key(key)),
                "{key} = {value}"
            );
        }
    }

    #[test]
    fn errors_are_reported_together_and_nothing_is_written() {
        let Err(FormatError::Invalid(errors)) = apply_all(
            "font-size = 13\n",
            &[
                ("font-size", json!(1)),
                ("cursor-style", json!("bar")),
                ("background", json!("nope!")),
            ],
        ) else {
            panic!("expected errors")
        };
        assert_eq!(
            errors.keys().collect::<Vec<_>>(),
            ["background", "font-size"]
        );
    }

    #[test]
    fn single_valued_list_settings_take_one_line() {
        let after = apply(
            "macos-icon-screen-color = #000000\n",
            "macos-icon-screen-color",
            json!(["#111111,#222222"]),
        );
        assert_eq!(after, "macos-icon-screen-color = #111111,#222222\n");
        assert_eq!(
            parse(&after).values["macos-icon-screen-color"],
            json!(["#111111,#222222"])
        );
        assert!(matches!(
            apply_all(
                "",
                &[("macos-icon-screen-color", json!(["#111111", "#222222"]))]
            ),
            Err(FormatError::Invalid(_))
        ));
        assert_eq!(
            apply(
                "macos-icon-screen-color = #000000\n",
                "macos-icon-screen-color",
                json!([])
            ),
            ""
        );
    }

    #[test]
    fn untouched_lines_survive_byte_for_byte() {
        let text = "# c\n  font-size   =   13  \nunknown-thing = 1\n\n\ncursor-style = block\n";
        let after = apply(text, "cursor-style", json!("bar"));
        assert_eq!(
            after,
            "# c\n  font-size   =   13  \nunknown-thing = 1\n\n\ncursor-style = bar\n"
        );
    }

    #[test]
    fn format_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<GhosttyFormat>();
    }
}
