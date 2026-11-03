//! ConfigStore against Ghostty's real app definition, on temp copies (never the live config).

use serde_json::{Value, json};
use settings_core::{ConfigStore, SaveError};
use std::fs;

fn changes(list: &[(&str, Value)]) -> Vec<(String, Value)> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn preview_keeps_comments_and_other_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.ghostty");
    let text = "# mine\nfont-size = 13\n  theme   =  Nord  \n";
    fs::write(&path, text).unwrap();
    let app = ghostty_config::app();
    let store = ConfigStore::new(&app, &path);
    let state = store.state();
    assert_eq!(state.values["font-size"], json!(13.0));
    assert_eq!(state.values["theme"], json!("Nord"));

    // `apply` (the preview) doesn't run the reload; `save` would ask a running Ghostty to reload.
    let result = store
        .apply(
            Some(&state.version),
            "",
            &changes(&[("font-size", json!(14)), ("cursor-style", json!("bar"))]),
        )
;
    if ghostty_config::ghostty_binary().is_none() {
        // Without Ghostty nothing can be checked, so nothing can be saved.
        let Err(SaveError::Invalid(errors)) = result else { panic!("must not pass unchecked") };
        assert_eq!(errors[""], ghostty_config::NO_GHOSTTY);
        return;
    }
    let result = result.unwrap();
    assert_eq!(
        result.after,
        "# mine\nfont-size = 14\n  theme   =  Nord  \n\ncursor-style = bar\n"
    );
}

#[test]
fn ghostty_rejects_what_the_writer_lets_through() {
    if ghostty_config::ghostty_binary().is_none() {
        eprintln!("skipped: Ghostty isn't installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.ghostty");
    fs::write(&path, "font-size = 13\n").unwrap();
    let app = ghostty_config::app();
    let store = ConfigStore::new(&app, &path);
    let Err(SaveError::Invalid(errors)) =
        store.save(None, "", &changes(&[("window-padding-x", json!("nope"))]))
    else {
        panic!("Ghostty should reject window-padding-x = nope")
    };
    assert!(
        errors["window-padding-x"].starts_with("Ghostty: "),
        "{errors:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "font-size = 13\n");
}
