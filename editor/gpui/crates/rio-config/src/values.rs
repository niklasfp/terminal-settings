//! Validates a value coming from the UI against its setting and turns it into a TOML literal.

use crate::toml_doc::{float, format_key, quote};
use serde_json::Value;
use settings_core::values::{self, items, number};
use settings_core::{Setting, SettingType};

const BLUR_STYLES: [&str; 2] = ["macos-glass-regular", "macos-glass-clear"];

type Result<T> = std::result::Result<T, String>;

/// The TOML literal for `value`, or a user-facing error. `Null` is rejected: removing a setting
/// isn't a value.
pub fn to_toml(setting: &Setting, value: &Value) -> Result<String> {
    if value.is_null() {
        return Err("A value is required. Use reset to remove it instead.".into());
    }
    literal(setting.ty, value, Some(setting))
}

fn literal(ty: SettingType, value: &Value, setting: Option<&Setting>) -> Result<String> {
    match ty {
        SettingType::Bool => Ok(bool_literal(values::boolean(value)?)),
        SettingType::Int => Ok(values::int(setting, value)?.to_string()),
        SettingType::Float => Ok(float(values::float(setting, value)?)),

        SettingType::String | SettingType::Path | SettingType::Font | SettingType::Theme => {
            Ok(quote(text(value)?))
        }

        SettingType::Enum => {
            let s = text(value)?;
            if let Some(values) = setting.and_then(|s| s.values.as_ref())
                && !values.iter().any(|v| v == s)
            {
                return Err(format!("Must be one of: {}.", values.join(", ")));
            }
            Ok(quote(s))
        }

        SettingType::Color => {
            let s = text(value)?.trim();
            if !is_hex_color(s) {
                return Err("Use a hex color like #1e1e2e or #1e1e2ecc.".into());
            }
            let lower = s.to_ascii_lowercase();
            Ok(quote(&if lower.starts_with('#') {
                lower
            } else {
                format!("#{lower}")
            }))
        }

        SettingType::Strings => {
            let items = items(value)?
                .iter()
                .map(|i| text(i).map(quote))
                .collect::<Result<Vec<_>>>()?;
            Ok(array(&items))
        }

        SettingType::Flags => {
            let items = items(value)?.iter().map(text).collect::<Result<Vec<_>>>()?;
            if let Some(allowed) = setting.and_then(|s| s.values.as_ref())
                && let Some(bad) = items.iter().find(|i| !allowed.iter().any(|a| a == *i))
            {
                return Err(format!("Unknown value '{bad}'."));
            }
            Ok(array(&items.into_iter().map(quote).collect::<Vec<_>>()))
        }

        SettingType::Margin => {
            let nums = items(value)?
                .iter()
                .map(number)
                .collect::<Result<Vec<_>>>()?;
            if !matches!(nums.len(), 1 | 2 | 4) {
                return Err("Use 1, 2 or 4 numbers (CSS-style).".into());
            }
            if nums.iter().any(|n| *n < 0.0) {
                return Err("Margins can't be negative.".into());
            }
            Ok(array(&nums.into_iter().map(num).collect::<Vec<_>>()))
        }

        SettingType::Pair => {
            let nums = items(value)?
                .iter()
                .map(number)
                .collect::<Result<Vec<_>>>()?;
            if nums.len() != 2 || nums.iter().any(|n| *n < 0.0 || *n != n.floor()) {
                return Err("Use two whole numbers.".into());
            }
            Ok(array(
                &nums
                    .into_iter()
                    .map(|n| (n as i64).to_string())
                    .collect::<Vec<_>>(),
            ))
        }

        SettingType::Blur => {
            if let Value::Bool(b) = value {
                return Ok(bool_literal(*b));
            }
            let style = text(value)?;
            if !BLUR_STYLES.contains(&style) {
                return Err(format!(
                    "Must be true, false or one of: {}.",
                    BLUR_STYLES.join(", ")
                ));
            }
            Ok(quote(style))
        }

        SettingType::FontStyle => {
            if *value == Value::Bool(false) {
                return Ok("false".into());
            }
            let name = text(value)?.trim();
            Ok(quote(if name.is_empty() { "default" } else { name }))
        }

        SettingType::Records => {
            let fields = setting
                .and_then(|s| s.fields.as_deref())
                .unwrap_or_default();
            let mut rows = Vec::new();
            for item in items(value)? {
                let Value::Object(obj) = item else {
                    return Err("Each entry must be an object.".into());
                };
                let mut parts = Vec::new();
                for f in fields {
                    let v = match obj.get(&f.key) {
                        None | Some(Value::Null) => continue,
                        Some(Value::String(s)) if s.is_empty() => continue,
                        Some(v) => v,
                    };
                    parts.push(format!(
                        "{} = {}",
                        format_key(std::slice::from_ref(&f.key)),
                        literal(f.ty, v, None)?
                    ));
                }
                if !parts.is_empty() {
                    rows.push(format!("{{ {} }}", parts.join(", ")));
                }
            }
            // Multi-line arrays are TOML 1.0; multi-line inline tables are not, so each entry stays on one line.
            Ok(if rows.is_empty() {
                "[]".into()
            } else {
                format!("[\n  {},\n]", rows.join(",\n  "))
            })
        }

        SettingType::Raw | SettingType::Fonts => {
            Err(format!("'{ty:?}' values can't be set this way."))
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    let hex = s.strip_prefix('#').unwrap_or(s);
    matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whole numbers without a decimal point, others as TOML floats.
fn num(d: f64) -> String {
    if d == d.floor() && d.abs() < 1e15 { (d as i64).to_string() } else { float(d) }
}

fn bool_literal(b: bool) -> String {
    if b { "true".into() } else { "false".into() }
}

fn array(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or_else(|| "Expected text.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;
    use serde_json::json;

    fn toml(key: &str, json: &str) -> Result<String> {
        let schema = schema();
        let setting = schema
            .find(key)
            .unwrap_or_else(|| panic!("{key} is in the schema"));
        to_toml(setting, &serde_json::from_str(json).expect("test JSON"))
    }

    #[test]
    fn converts_to_toml() {
        let cases = [
            ("window.opacity", "0.5", "0.5"),
            ("window.opacity", "1", "1.0"),
            ("fonts.size", "14", "14.0"),
            ("window.width", "1200", "1200"),
            ("cursor.blinking", "true", "true"),
            ("cursor.shape", "\"beam\"", "\"beam\""),
            ("colors.background", "\"1E1E2E\"", "\"#1e1e2e\""),
            ("colors.background", "\"#1e1e2ecc\"", "\"#1e1e2ecc\""),
            ("env-vars", r#"["A=1","B=\"x\""]"#, r#"["A=1", "B=\"x\""]"#),
            ("margin", "[10, 20]", "[10, 20]"),
            ("effects.trail-cursor-decay", "[100, 400]", "[100, 400]"),
            (
                "window.blur",
                "\"macos-glass-clear\"",
                "\"macos-glass-clear\"",
            ),
            ("window.blur", "false", "false"),
            ("fonts.regular.style", "false", "false"),
            ("fonts.regular.style", "\"\"", "\"default\""),
            (
                "keyboard.forward-to-ime-modifier-mask",
                r#"["shift","alt"]"#,
                r#"["shift", "alt"]"#,
            ),
        ];
        for (key, json, expected) in cases {
            assert_eq!(toml(key, json).as_deref(), Ok(expected), "{key} = {json}");
        }
    }

    #[test]
    fn rejects_invalid_values() {
        let cases = [
            ("window.opacity", "1.5"),
            ("cursor.blinking-interval", "100"),
            ("window.width", "100.5"),
            ("cursor.shape", "\"triangle\""),
            ("colors.background", "\"red\""),
            ("margin", "[1, 2, 3]"),
            ("window.blur", "\"frosted\""),
            ("keyboard.forward-to-ime-modifier-mask", "[\"hyper\"]"),
            ("cursor.blinking", "\"yes\""),
        ];
        for (key, json) in cases {
            assert!(
                toml(key, json).is_err(),
                "{key} = {json} should be rejected"
            );
        }
    }

    #[test]
    fn error_messages_read_well() {
        assert_eq!(
            toml("window.opacity", "1.5").unwrap_err(),
            "Must be at most 1."
        );
        assert_eq!(
            toml("window.width", "100.5").unwrap_err(),
            "Must be a whole number."
        );
        assert_eq!(
            toml("cursor.blinking", "\"yes\"").unwrap_err(),
            "Expected true or false."
        );
        assert!(
            toml("cursor.shape", "\"triangle\"")
                .unwrap_err()
                .starts_with("Must be one of: block")
        );
    }

    #[test]
    fn records_become_one_inline_table_per_line_and_skip_empty_fields() {
        let toml = toml(
            "bindings.keys",
            r#"[{"key":"t","with":"super","action":"CreateTab","esc":""},{"key":"w","with":"super","action":"CloseTab"}]"#,
        );
        assert_eq!(
            toml.as_deref(),
            Ok(
                "[\n  { key = \"t\", with = \"super\", action = \"CreateTab\" },\n  { key = \"w\", with = \"super\", action = \"CloseTab\" },\n]"
            )
        );
    }

    #[test]
    fn control_characters_use_unicode_escapes_not_toml_1_1_escapes() {
        assert_eq!(
            toml("title.content", r#""a\u001bb""#).as_deref(),
            Ok(r#""a\u001Bb""#)
        );
    }

    #[test]
    fn every_generated_default_is_accepted_by_its_own_validator() {
        for s in &schema().settings {
            if s.ty == SettingType::Raw || s.default.is_null() {
                continue;
            }
            if let Err(e) = to_toml(s, &s.default) {
                panic!("default of {} ({}) is rejected: {e}", s.key, s.default);
            }
        }
    }

    #[test]
    fn null_is_not_a_value() {
        let schema = schema();
        let s = schema.find("window.opacity").expect("in schema");
        assert!(to_toml(s, &json!(null)).is_err());
    }
}
