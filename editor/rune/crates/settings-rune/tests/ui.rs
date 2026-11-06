//! Headless Rune UI tests for both terminal editors (`rio-settings` and `ghostty-settings`).

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use serde_json::json;
use settings_core::Document;
use settings_rune::view::SettingsWindow;
use settings_rune::{Brand, Palette};

const TEST_BRAND: Brand = Brand {
    light: Palette {
        bg: "#f4f4f5",
        fg: "#18181b",
        accent: "#c026d3",
        sidebar: "#e4e4e7",
    },
    dark: Palette {
        bg: "#1b1b1d",
        fg: "#e4e4e7",
        accent: "#e879f9",
        sidebar: "#18181b",
    },
    logo: &[],
};

fn click(rt: &mut Runtime<SettingsWindow>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("element '{key}' not found"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

#[test]
fn rio_renders_every_group_and_saves_through_diff_dialog() {
    let app = Box::leak(Box::new(rio_config::app()));
    let doc = Document::memory("config.toml", "[window]\nopacity = 0.9\n");
    let state = SettingsWindow::new(app, TEST_BRAND, doc, None);
    let mut rt = Runtime::new(
        state,
        SettingsWindow::view,
        (1180.0, 820.0),
        Env { system_dark: true, scale: 2.0 },
    );
    rt.frame(0.0);

    // Every group in Rio's schema renders cleanly in Rune.
    let group_ids: Vec<String> = app.schema.groups.iter().map(|g| g.id.clone()).collect();
    for id in group_ids {
        click(&mut rt, &format!("group:{id}"));
        assert_eq!(rt.state.active_group, id);
    }

    // Switch back to the window group, edit opacity, and click Save.
    click(&mut rt, "group:window");
    let opacity = app.schema.find("window.opacity").expect("window.opacity in Rio schema");
    assert_eq!(rt.state.editor.current_value(opacity), json!(0.9));
    rt.update(|s, _| s.set_and_sync("window.opacity", json!(0.75)));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.editor.dirty_count(), 1);

    click(&mut rt, "save");
    assert!(rt.state.save_preview.is_some(), "diff preview dialog opens");
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }

    click(&mut rt, "confirm-save");
    assert_eq!(rt.state.editor.dirty_count(), 0);
    assert_eq!(
        rt.state.document.contents().as_deref(),
        Some("[window]\nopacity = 0.75\n")
    );
}

#[test]
fn ghostty_renders_every_group_and_filters_by_search() {
    let app = Box::leak(Box::new(ghostty_config::app().for_browser()));
    let doc = Document::memory("config.ghostty", "font-size = 14\n");
    let state = SettingsWindow::new(app, TEST_BRAND, doc, None);
    let mut rt = Runtime::new(
        state,
        SettingsWindow::view,
        (1180.0, 820.0),
        Env { system_dark: false, scale: 2.0 },
    );
    rt.frame(0.0);

    let group_ids: Vec<String> = app.schema.groups.iter().map(|g| g.id.clone()).collect();
    for id in group_ids {
        click(&mut rt, &format!("group:{id}"));
        assert_eq!(rt.state.active_group, id);
    }

    // Searching filters across all groups.
    rt.update(|s, _| s.search.replace_text("font-size"));
    rt.frame(1.0 / 60.0);
    assert!(rt.find("row:font-size").is_some());
}

fn settle(rt: &mut Runtime<SettingsWindow>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn the_color_picker_picks_from_the_palette_and_the_hue_slider() {
    let app = Box::leak(Box::new(rio_config::app()));
    let doc = Document::memory("config.toml", "[colors]\nbackground = \"#1E1E2E\"\n");
    let state = SettingsWindow::new(app, TEST_BRAND, doc, None);
    let mut rt = Runtime::new(state, SettingsWindow::view, (1180.0, 820.0), Env { system_dark: true, scale: 2.0 });
    rt.frame(0.0);
    click(&mut rt, "group:colors");
    settle(&mut rt);

    click(&mut rt, "colors.background:swatch");
    settle(&mut rt);
    assert!(rt.state.color_picker.is_some(), "the swatch opens the picker");
    assert_eq!(rt.state.editor.dirty_count(), 0, "opening it changes nothing");

    // The fourth row's first swatch: a red.
    click(&mut rt, "swatch3-0");
    let bg = app.schema.find("colors.background").unwrap();
    let picked = rt.state.editor.current_value(bg);
    assert_eq!(rt.state.editor.dirty_count(), 1);
    assert!(picked.as_str().is_some_and(|h| h.starts_with('#') && h.len() == 7), "{picked}");
    assert_eq!(rt.state.edits["colors.background"].text, picked.as_str().unwrap(), "the hex field follows");

    // Sliders: dragging hue to the far right wraps back to red's hue; dragging to the middle is cyan.
    rt.update(|s, _| s.color_picker.as_mut().unwrap().tab = 1);
    settle(&mut rt);
    let (x, y) = rt.find("Hue-slider").expect("hue slider");
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    let hue = rt.state.color_picker.as_ref().unwrap().hsla.h;
    assert!((170.0..=190.0).contains(&hue), "the middle of the hue track is cyan, got {hue}");
    assert_ne!(rt.state.editor.current_value(bg), picked, "the hue drag changed the color");

    // The headless runtime has no AppSpec, so the window's key handler is called directly.
    let escape = KeyEvent { key: Key::Escape, mods: Mods::default() };
    rt.update(|s, tx| assert!(s.on_key(&escape, tx), "Escape is handled"));
    rt.frame(1.0 / 60.0);
    assert!(rt.state.color_picker.is_none(), "Escape closes the picker");
}
