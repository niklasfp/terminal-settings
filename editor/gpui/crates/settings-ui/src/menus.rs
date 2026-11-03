//! The native menu bar. GPUI has no default menus; the app sets them, and rebuilds them when the
//! appearance changes so its checkmark follows the header switch.

use crate::view::{
    About, AppearanceDark, AppearanceLight, AppearanceSystem, CloseWindow, FocusSearch, Hide, HideOthers, Minimize,
    Quit, Save, ShowAll, Zoom,
};
use gpui_kit::component::input;
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

/// App-wide shortcuts for the menu commands (window-level ones like ⌘S live in `view::bind_keys`).
/// ⌘ on macOS, Ctrl elsewhere (KeyBinding::new can't parse `secondary-`); hiding and minimizing
/// are macOS conventions.
pub fn bind_keys(cx: &mut App) {
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-m", Minimize, None),
        ]);
    } else {
        cx.bind_keys([KeyBinding::new("ctrl-q", Quit, None), KeyBinding::new("ctrl-w", CloseWindow, None)]);
    }
    // Fallbacks for when no settings window has focus to handle them.
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
}

pub fn set(app_name: &str, appearance: Appearance, cx: &mut App) {
    cx.set_menus(build(app_name, appearance));
}

fn build(app_name: &str, appearance: Appearance) -> Vec<Menu> {
    vec![
        Menu::new(app_name.to_string()).items([
            MenuItem::action(format!("About {app_name}"), About),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(format!("Hide {app_name}"), Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action(format!("Quit {app_name}"), Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Save…", Save),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find", FocusSearch),
        ]),
        Menu::new("View").items([MenuItem::submenu(Menu::new("Appearance").items([
            MenuItem::action("System", AppearanceSystem).checked(appearance == Appearance::System),
            MenuItem::action("Light", AppearanceLight).checked(appearance == Appearance::Light),
            MenuItem::action("Dark", AppearanceDark).checked(appearance == Appearance::Dark),
        ]))]),
        Menu::new("Window").items([MenuItem::action("Minimize", Minimize), MenuItem::action("Zoom", Zoom)]),
    ]
}
