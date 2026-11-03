//! Turns a UI value into Ghostty config values (one per line). Checks the obvious things here;
//! Ghostty's own `+validate-config` then checks the whole file before it's written.

use serde_json::Value;
use settings_core::values::{self, items};
use settings_core::{Setting, SettingType};

/// The config values for `value`, one per line, unquoted. `Err` is a user-facing message.
pub fn to_lines(s: &Setting, value: &Value) -> Result<Vec<String>, String> {
    let lines = match s.ty {
        SettingType::Bool => vec![values::boolean(value)?.to_string()],
        SettingType::Int => vec![values::int(Some(s), value)?.to_string()],
        SettingType::Float => vec![format_float(values::float(Some(s), value)?)],
        SettingType::Enum => {
            let v = text(value)?;
            if let Some(values) = &s.values
                && !values.contains(&v)
            {
                return Err(format!("Must be one of: {}.", values.join(", ")));
            }
            vec![v]
        }
        SettingType::Color => {
            let raw = text(value)?;
            let color = raw.trim();
            if let Some(hex) = hex_color(color) {
                vec![hex]
            } else if s
                .values
                .as_ref()
                .is_some_and(|v| v.iter().any(|k| k == color))
                || is_color_name(color)
            {
                // Keywords (cell-foreground, bright) and X11 names (red, dark slate gray); Ghostty checks names.
                vec![color.to_string()]
            } else {
                return Err("Use a hex color like #1e1e2e or a color name.".into());
            }
        }
        SettingType::Flags => {
            // Spell out every flag so the line means the same thing whatever the defaults become.
            let on = items(value)?
                .iter()
                .map(text)
                .collect::<Result<Vec<_>, _>>()?;
            let known = s.values.as_deref().unwrap_or_default();
            if let Some(bad) = on.iter().find(|f| !known.contains(f)) {
                return Err(format!("Unknown option '{bad}'."));
            }
            let spelled: Vec<String> = known
                .iter()
                .map(|f| {
                    if on.contains(f) {
                        f.clone()
                    } else {
                        format!("no-{f}")
                    }
                })
                .collect();
            vec![spelled.join(",")]
        }
        SettingType::FontStyle => match value {
            Value::Bool(false) => vec!["false".into()],
            _ => {
                let raw = text(value)?;
                let style = raw.trim();
                vec![if style.is_empty() { "default" } else { style }.to_string()]
            }
        },
        SettingType::Strings | SettingType::Fonts => items(value)?
            .iter()
            .map(text)
            .filter(|v| v.as_ref().map_or(true, |v| !v.is_empty()))
            .collect::<Result<Vec<_>, _>>()?,
        SettingType::Records => records(s, value)?,
        // Strings, paths, themes, durations and Ghostty's own formats (padding, modifiers, …):
        // passed through exactly (no trimming: some values start with spaces) and checked by Ghostty.
        _ => vec![text(value)?],
    };
    // A line break would split the value into lines of its own.
    if lines.iter().any(|l| l.contains(['\n', '\r'])) {
        return Err("Must be on one line.".into());
    }
    Ok(lines)
}

fn records(s: &Setting, value: &Value) -> Result<Vec<String>, String> {
    let fields = s.fields.as_deref().unwrap_or_default();
    let mut lines = Vec::new();
    for item in items(value)? {
        let (Value::Object(obj), [first, second, ..]) = (item, fields) else {
            return Err("Each entry needs two parts.".into());
        };
        let part = |key: &str| match obj.get(key) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(v)) => v.trim().to_string(),
            Some(other) => other.to_string().trim().to_string(),
        };
        let (a, b) = (part(&first.key), part(&second.key));
        if a.is_empty() && b.is_empty() {
            continue;
        }
        if a.is_empty() || b.is_empty() {
            return Err(format!(
                "Each entry needs both {} and {}.",
                first.key, second.key
            ));
        }
        lines.push(format!("{a}={b}"));
    }
    Ok(lines)
}

/// "#RRGGBB" or "RRGGBB" → "#rrggbb".
pub(crate) fn hex_color(v: &str) -> Option<String> {
    let digits = v.strip_prefix('#').unwrap_or(v);
    (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", digits.to_ascii_lowercase()))
}

/// Looks like an X11 color name: a letter, then letters, digits and spaces.
fn is_color_name(v: &str) -> bool {
    let mut chars = v.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == ' ')
}

/// The shortest text that reads back as the same number.
fn format_float(d: f64) -> String {
    format!("{d}")
}


fn text(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(v.to_string()),
        _ => Err("Expected text.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setting(key: &str) -> Setting {
        crate::schema()
            .find(key)
            .unwrap_or_else(|| panic!("{key} in the schema"))
            .clone()
    }

    fn lines(key: &str, value: Value) -> Result<Vec<String>, String> {
        to_lines(&setting(key), &value)
    }

    #[test]
    fn bools_ints_and_floats() {
        assert_eq!(lines("font-thicken", json!(true)).unwrap(), ["true"]);
        assert!(lines("font-thicken", json!("yes")).is_err());
        assert_eq!(lines("font-thicken-strength", json!(10)).unwrap(), ["10"]);
        assert_eq!(lines("font-thicken-strength", json!("12")).unwrap(), ["12"]);
        assert_eq!(
            lines("font-thicken-strength", json!(1.5)).unwrap_err(),
            "Must be a whole number."
        );
        assert_eq!(
            lines("font-thicken-strength", json!(256)).unwrap_err(),
            "Must be at most 255."
        );
        assert_eq!(lines("font-size", json!(13.5)).unwrap(), ["13.5"]);
        assert_eq!(lines("font-size", json!(14)).unwrap(), ["14"]);
        assert_eq!(
            lines("font-size", json!(2)).unwrap_err(),
            "Must be at least 4."
        );
        assert_eq!(lines("background-opacity", json!(0.85)).unwrap(), ["0.85"]);
        assert!(lines("font-size", json!("big")).is_err());
    }

    #[test]
    fn enums_must_be_known() {
        assert_eq!(lines("cursor-style", json!("bar")).unwrap(), ["bar"]);
        assert!(
            lines("cursor-style", json!("triangle"))
                .unwrap_err()
                .starts_with("Must be one of: ")
        );
    }

    #[test]
    fn colors_take_hex_keywords_and_names() {
        assert_eq!(lines("background", json!("#1E1E2E")).unwrap(), ["#1e1e2e"]);
        assert_eq!(lines("background", json!(" 1e1e2e ")).unwrap(), ["#1e1e2e"]);
        assert_eq!(
            lines("cursor-color", json!("cell-foreground")).unwrap(),
            ["cell-foreground"]
        );
        assert_eq!(
            lines("background", json!("dark slate gray")).unwrap(),
            ["dark slate gray"]
        );
        assert!(lines("background", json!("#12")).is_err());
        assert!(lines("background", json!("rgb(1,2,3)")).is_err());
    }

    #[test]
    fn flags_spell_out_every_flag() {
        let s = setting("shell-integration-features");
        let known = s.values.clone().unwrap();
        let line = &to_lines(&s, &json!(["cursor"])).unwrap()[0];
        assert_eq!(line.split(',').count(), known.len());
        assert!(line.starts_with("cursor,no-"));
        assert_eq!(
            to_lines(&s, &json!(["bogus"])).unwrap_err(),
            "Unknown option 'bogus'."
        );
        assert!(to_lines(&s, &json!("cursor")).is_err());
    }

    #[test]
    fn font_style_false_and_default() {
        assert_eq!(lines("font-style", json!(false)).unwrap(), ["false"]);
        assert_eq!(lines("font-style", json!("  ")).unwrap(), ["default"]);
        assert_eq!(lines("font-style", json!(" Heavy ")).unwrap(), ["Heavy"]);
    }

    #[test]
    fn lists_drop_empty_items_and_reject_null() {
        assert_eq!(
            lines("font-family", json!(["A", "", "B"])).unwrap(),
            ["A", "B"]
        );
        assert_eq!(
            lines("font-family", json!(["A", null])).unwrap_err(),
            "Lists can't contain null."
        );
        assert_eq!(
            lines("font-family", json!("A")).unwrap_err(),
            "Expected a list."
        );
    }

    #[test]
    fn records_need_both_parts() {
        assert_eq!(
            lines(
                "keybind",
                json!([{"trigger": " super+t ", "action": "new_tab"}, {"trigger": "", "action": ""}])
            )
            .unwrap(),
            ["super+t=new_tab"]
        );
        assert_eq!(
            lines("keybind", json!([{"trigger": "super+t"}])).unwrap_err(),
            "Each entry needs both trigger and action."
        );
        assert_eq!(
            lines("keybind", json!(["super+t=new_tab"])).unwrap_err(),
            "Each entry needs two parts."
        );
        assert_eq!(
            lines("palette", json!([{"index": 1, "color": "#ff0000"}])).unwrap(),
            ["1=#ff0000"]
        );
    }

    #[test]
    fn text_passes_through_untrimmed_but_single_line() {
        assert_eq!(
            lines("selection-word-chars", json!(" \t'\"")).unwrap(),
            [" \t'\""]
        );
        assert_eq!(
            lines("title", json!("a\nkeybind = x=y")).unwrap_err(),
            "Must be on one line."
        );
        assert_eq!(lines("title", json!(5)).unwrap(), ["5"]);
        assert!(lines("title", json!({})).is_err());
    }
}
