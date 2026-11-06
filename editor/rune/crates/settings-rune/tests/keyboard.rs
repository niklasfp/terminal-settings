//! Dropdowns open from the keyboard: Tab to the trigger, then Enter (as well as Rune's own Space
//! and Down).

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use settings_core::{Document, Setting, SettingType};
use settings_rune::view::SettingsWindow;
use settings_rune::{Brand, Palette};

const BRAND: Brand = Brand {
    light: Palette { bg: "#ffffff", fg: "#000000", accent: "#3551f3", sidebar: "#eeeeee" },
    dark: Palette { bg: "#111111", fg: "#eeeeee", accent: "#3551f3", sidebar: "#000000" },
    logo: &[],
};

fn key(rt: &mut Runtime<SettingsWindow>, k: Key) {
    rt.key(KeyEvent { key: k, mods: Mods::default() });
    rt.frame(1.0 / 60.0);
}

/// Ghostty's playground editor on the page holding its first setting that `pick` accepts.
fn on_page_of(pick: impl Fn(&Setting) -> bool) -> Runtime<SettingsWindow> {
    let app = Box::leak(Box::new(ghostty_config::browser_app()));
    let state = SettingsWindow::new(app, BRAND, Document::memory("config", ""), None);
    let mut rt = Runtime::new(state, SettingsWindow::view, (1180.0, 820.0), Env { system_dark: true, scale: 2.0 });
    rt.window_focus(true);
    rt.frame(0.0);
    let group = app.schema.settings.iter().find(|x| pick(x)).expect("such a setting").group.clone();
    rt.update(|s, _| s.active_group = group.clone());
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

/// An enum drawn as a Select, not a segmented control.
fn is_select(s: &Setting) -> bool {
    s.ty == SettingType::Enum && !settings_rune::controls::is_segmented(s.values.as_deref().unwrap_or_default())
}

/// Whether pressing `k` after some number of Tabs (each count in a fresh window, so no key
/// changes anything along the way) opens what `opened` checks.
fn tab_then(pick: impl Fn(&Setting) -> bool + Copy, k: Key, opened: impl Fn(&Runtime<SettingsWindow>) -> bool) -> bool {
    (1..120).any(|tabs| {
        let mut rt = on_page_of(pick);
        for _ in 0..tabs {
            key(&mut rt, Key::Tab);
        }
        key(&mut rt, k.clone());
        opened(&rt)
    })
}

fn is_theme(s: &Setting) -> bool {
    s.ty == SettingType::Theme
}

#[test]
fn enter_space_and_down_open_the_theme_list() {
    for k in [Key::Enter, Key::Text(" ".into()), Key::Down] {
        assert!(
            tab_then(is_theme, k.clone(), |rt| rt.state.combos.get("theme").is_some_and(|c| c.open && c.search.focused)),
            "{k:?} opens the theme list with the caret in its search field"
        );
    }
}

#[test]
fn enter_opens_an_enum_dropdown() {
    assert!(tab_then(is_select, Key::Enter, |rt| rt.state.open_select.is_some()), "Enter on a Select's trigger opens it");
}
