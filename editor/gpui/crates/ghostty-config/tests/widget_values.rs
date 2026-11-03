//! Every value a widget can produce must be something Ghostty itself accepts: every default, every
//! choice of every enum, every single flag, both bools and every color keyword, written through
//! the writer and checked by `ghostty +validate-config`. Skipped when Ghostty isn't installed.

use serde_json::{Value, json};
use settings_core::SettingType;
use std::process::Command;

#[test]
fn every_default_is_accepted_by_its_own_writer() {
    for s in &ghostty_config::schema().settings {
        if s.default.is_null() {
            continue;
        }
        if let Err(e) = ghostty_config::to_lines(s, &s.default) {
            panic!("default of {} ({}): {e}", s.key, s.default);
        }
    }
}

#[test]
fn every_widget_value_is_accepted_by_ghostty() {
    let Some(bin) = ghostty_config::ghostty_binary() else {
        eprintln!("skipped: Ghostty isn't installed");
        return;
    };
    let mut lines = Vec::new();
    for s in &ghostty_config::schema().settings {
        let mut candidates: Vec<Value> = Vec::new();
        if !s.default.is_null() {
            candidates.push(s.default.clone());
        }
        let values = s.values.clone().unwrap_or_default();
        match s.ty {
            SettingType::Enum => candidates.extend(values.iter().map(|v| json!(v))),
            SettingType::Flags => candidates.extend(values.iter().map(|f| json!([f]))),
            SettingType::Bool => candidates.extend([json!(true), json!(false)]),
            SettingType::Color => candidates.extend(values.iter().map(|k| json!(k))),
            _ => {}
        }
        for value in candidates {
            let written = ghostty_config::to_lines(s, &value)
                .unwrap_or_else(|e| panic!("{} = {value}: {e}", s.key));
            lines.extend(written.into_iter().map(|v| format!("{} = {v}", s.key)));
        }
    }
    let file = tempfile::Builder::new()
        .prefix("ghostty-widgets-")
        .suffix(".ghostty")
        .tempfile()
        .unwrap();
    std::fs::write(file.path(), lines.join("\n") + "\n").unwrap();
    let out = Command::new(&bin)
        .arg("+validate-config")
        .arg(format!("--config-file={}", file.path().display()))
        .output()
        .expect("run ghostty");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        said.trim().is_empty() && out.status.success(),
        "{} lines; Ghostty said:\n{said}",
        lines.len()
    );
}
