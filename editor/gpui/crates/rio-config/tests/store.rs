//! ConfigStore against Rio's real app definition (ported from ConfigStoreTests.cs).

use serde_json::{Value, json};
use settings_core::{ConfigStore, SaveError, version_of};
use std::fs;

fn changes(list: &[(&str, Value)]) -> Vec<(String, Value)> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[cfg(unix)]
#[test]
fn save_writes_through_symlink_and_keeps_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real").join("config.toml");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "# mine\n[window]\nopacity = 1.0\n").unwrap();
    let link = dir.path().join("config.toml");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let app = rio_config::app();
    let store = ConfigStore::new(&app, &link);
    assert_eq!(store.target_path(), target);
    let result = store
        .save(None, "", &changes(&[("window.opacity", json!(0.8))]))
        .unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# mine\n[window]\nopacity = 0.8\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("real").join("config.toml.bak")).unwrap(),
        "# mine\n[window]\nopacity = 1.0\n"
    );
    assert_eq!(
        result.version,
        version_of("# mine\n[window]\nopacity = 0.8\n")
    );
    assert_eq!(
        result.note.as_deref(),
        Some("Saved. Rio picks up the change right away.")
    );
}

#[test]
fn stale_version_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[window]\nopacity = 1.0\n").unwrap();
    let app = rio_config::app();
    let store = ConfigStore::new(&app, &path);
    let version = version_of(&fs::read_to_string(&path).unwrap());
    fs::write(&path, "[window]\nopacity = 0.5\n").unwrap();

    let result = store.apply(
        Some(&version),
        "",
        &changes(&[("window.opacity", json!(0.8))]),
    );
    assert!(matches!(result, Err(SaveError::Conflict)));
}

#[test]
fn invalid_values_are_reported_per_key_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "").unwrap();
    let app = rio_config::app();
    let store = ConfigStore::new(&app, &path);

    let result = store.save(
        None,
        "",
        &changes(&[
            ("window.opacity", json!(3)),
            ("cursor.shape", json!("beam")),
            ("nope", json!(1)),
        ]),
    );
    let Err(SaveError::Invalid(errors)) = result else {
        panic!("expected per-key errors, got {result:?}")
    };
    assert!(errors.contains_key("window.opacity"));
    assert!(errors.contains_key("nope"));
    assert!(!errors.contains_key("cursor.shape"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
}

#[test]
fn platform_scope_only_accepts_overridable_settings() {
    let dir = tempfile::tempdir().unwrap();
    let app = rio_config::app();
    let store = ConfigStore::new(&app, dir.path().join("config.toml"));
    let ok = store
        .apply(None, "macos", &changes(&[("window.opacity", json!(0.9))]))
        .unwrap();
    assert_eq!(ok.after, "[platform.macos.window]\nopacity = 0.9\n");

    let locked = store.apply(None, "macos", &changes(&[("cursor.shape", json!("beam"))]));
    assert!(matches!(locked, Err(SaveError::Invalid(_))));
}

#[test]
fn null_removes_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[window]\nopacity = 0.9\nmode = \"Maximized\"\n").unwrap();
    let app = rio_config::app();
    let r = ConfigStore::new(&app, &path)
        .apply(None, "", &changes(&[("window.opacity", Value::Null)]))
        .unwrap();
    assert_eq!(r.after, "[window]\nmode = \"Maximized\"\n");
}

#[test]
fn crlf_file_keeps_crlf_after_a_new_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "# mine\r\n[window]\r\nopacity = 1.0\r\n").unwrap();
    let app = rio_config::app();
    let r = ConfigStore::new(&app, &path)
        .apply(None, "", &changes(&[("cursor.shape", json!("beam"))]))
        .unwrap();
    assert_eq!(
        r.after,
        "# mine\r\n[window]\r\nopacity = 1.0\r\n\r\n[cursor]\r\nshape = \"beam\"\r\n"
    );
}

#[test]
fn state_reads_values_and_raw_blocks_or_reports_parse_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[window]\nopacity = 0.9\n\n[[hints.rules]]\nregex = \"x\"\n",
    )
    .unwrap();
    let app = rio_config::app();
    let store = ConfigStore::new(&app, &path);
    let state = store.state();
    assert!(state.exists && state.parse_error.is_none());
    assert_eq!(state.values["window.opacity"], json!(0.9));
    assert_eq!(state.raw["hints.rules"], "[[hints.rules]]\nregex = \"x\"\n");

    fs::write(&path, "[window\n").unwrap();
    let state = store.state();
    assert!(state.values.is_empty());
    assert!(
        state
            .parse_error
            .is_some_and(|e| e.starts_with("config.toml is not valid TOML"))
    );
    assert!(matches!(
        store.apply(None, "", &[]),
        Err(SaveError::Parse(_))
    ));
}

#[test]
fn raw_setting_replaces_table_array_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[window]\nopacity = 0.9\n").unwrap();
    let app = rio_config::app();
    let store = ConfigStore::new(&app, &path);
    let r = store
        .apply(
            None,
            "",
            &changes(&[("hints.rules", json!("[[hints.rules]]\nregex = \"y\"\n"))]),
        )
        .unwrap();
    assert_eq!(
        r.after,
        "[window]\nopacity = 0.9\n\n[[hints.rules]]\nregex = \"y\"\n"
    );
    let bad = store.apply(
        None,
        "",
        &changes(&[("hints.rules", json!("[window]\nopacity = 1.0\n"))]),
    );
    assert!(matches!(bad, Err(SaveError::Invalid(e)) if e.contains_key("hints.rules")));
}
