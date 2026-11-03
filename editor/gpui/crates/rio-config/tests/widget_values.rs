//! Every value a widget can produce must be something Rio itself accepts (ported from
//! RioWidgetValuesTests.cs). Needs the validator built by `cargo build --release` in
//! tools/rio/schema-dump, which parses a file with Rio's own Config; skipped when it isn't built.

use rio_config::RioTomlFormat;
use serde_json::{Value, json};
use settings_core::{Change, ConfigFormat, Setting, SettingType};
use std::path::PathBuf;
use std::process::Command;

fn validator() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tools/rio/schema-dump/target/release/schema-dump")
}

/// Rio's error, or None when Rio accepts the file.
fn rio_rejects(toml: &str) -> Option<String> {
    let file = tempfile::Builder::new()
        .suffix(".toml")
        .tempfile()
        .expect("temp file");
    std::fs::write(file.path(), toml).expect("write temp file");
    let out = Command::new(validator())
        .arg("validate")
        .arg(file.path())
        .output()
        .expect("run validator");
    if out.status.success() {
        None
    } else {
        Some(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn write(changes: &[(&Setting, Value)]) -> String {
    let changes: Vec<Change<'_>> = changes
        .iter()
        .map(|(s, v)| Change {
            setting: s,
            value: v.clone(),
        })
        .collect();
    RioTomlFormat
        .apply("", &changes, "")
        .unwrap_or_else(|e| panic!("apply failed: {e:?}"))
}

#[test]
fn every_widget_value_is_accepted_by_rio() {
    if !validator().exists() {
        eprintln!("skipped: {} isn't built", validator().display());
        return;
    }
    let schema = rio_config::schema();
    let settings: Vec<&Setting> = schema
        .settings
        .iter()
        .filter(|s| s.ty != SettingType::Raw)
        .collect();

    // All defaults together.
    let defaults: Vec<(&Setting, Value)> = settings
        .iter()
        .filter(|s| !s.default.is_null())
        .map(|s| (*s, s.default.clone()))
        .collect();
    let toml = write(&defaults);
    assert_eq!(rio_rejects(&toml), None, "defaults:\n{toml}");

    // Each choice / flag / on-off value on its own (TOML keys can't repeat).
    for s in settings {
        let values: Vec<Value> = match s.ty {
            SettingType::Enum => s.values.iter().flatten().map(|v| json!(v)).collect(),
            SettingType::Flags => s.values.iter().flatten().map(|v| json!([v])).collect(),
            SettingType::Bool => vec![json!(true), json!(false)],
            SettingType::Blur => vec![
                json!(false),
                json!(true),
                json!("macos-glass-regular"),
                json!("macos-glass-clear"),
            ],
            SettingType::FontStyle => vec![json!(false), json!("Bold")],
            SettingType::Margin => vec![json!([4]), json!([4, 8]), json!([1, 2, 3, 4])],
            _ => vec![],
        };
        for v in values {
            let toml = write(&[(s, v.clone())]);
            assert_eq!(rio_rejects(&toml), None, "{} = {v}:\n{toml}", s.key);
        }
    }
}
