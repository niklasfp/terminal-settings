//! Vector icons (`rune::Icon`, 16×16 viewBox) for sidebar section groups and row actions.

use rune::Icon;

pub static SLIDERS: Icon = Icon {
    name: "settings-sliders",
    view: 16.0,
    body: r#"<path d="M2.5 4.5H13.5M2.5 8H13.5M2.5 11.5H13.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><circle cx="5.5" cy="4.5" r="1.6" fill="currentColor"/><circle cx="10.5" cy="8" r="1.6" fill="currentColor"/><circle cx="6.5" cy="11.5" r="1.6" fill="currentColor"/>"#,
};

pub static WINDOW: Icon = Icon {
    name: "settings-window",
    view: 16.0,
    body: r#"<rect x="2" y="3" width="12" height="10" rx="1.8" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M2 6H14" stroke="currentColor" stroke-width="1.2"/><circle cx="4.2" cy="4.5" r="0.6" fill="currentColor"/><circle cx="6" cy="4.5" r="0.6" fill="currentColor"/>"#,
};

pub static TYPE: Icon = Icon {
    name: "settings-type",
    view: 16.0,
    body: r#"<path d="M3 3.5V5.5M13 3.5V5.5M3 3.5H13M8 3.5V12.5M6 12.5H10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub static PALETTE: Icon = Icon {
    name: "settings-palette",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="5.8" stroke="currentColor" stroke-width="1.3" fill="none"/><circle cx="6" cy="6" r="1" fill="currentColor"/><circle cx="9.5" cy="5.5" r="1" fill="currentColor"/><circle cx="10.8" cy="8.2" r="1" fill="currentColor"/><circle cx="6.2" cy="9.8" r="1" fill="currentColor"/>"#,
};

pub static DROPLET: Icon = Icon {
    name: "settings-droplet",
    view: 16.0,
    body: r#"<path d="M8 2.2C8 2.2 3.5 6.8 3.5 9.8A4.5 4.5 0 0 0 12.5 9.8C12.5 6.8 8 2.2 8 2.2Z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" fill="none"/>"#,
};

pub static CURSOR: Icon = Icon {
    name: "settings-cursor",
    view: 16.0,
    body: r#"<path d="M6 2.5H10M8 2.5V13.5M6 13.5H10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#,
};

pub static TABS: Icon = Icon {
    name: "settings-tabs",
    view: 16.0,
    body: r#"<rect x="2" y="3" width="12" height="10" rx="1.8" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M2 6.5H14M6.5 3V6.5M10.5 3V6.5" stroke="currentColor" stroke-width="1.2"/>"#,
};

pub static LAYOUT: Icon = Icon {
    name: "settings-layout",
    view: 16.0,
    body: r#"<rect x="2" y="3" width="12" height="10" rx="1.8" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M6.2 3V13" stroke="currentColor" stroke-width="1.2"/>"#,
};

pub static TERMINAL: Icon = Icon {
    name: "settings-terminal",
    view: 16.0,
    body: r#"<rect x="2" y="3" width="12" height="10" rx="1.8" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M4.5 6L6.8 8L4.5 10M8 10H11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub static KEYBOARD: Icon = Icon {
    name: "settings-keyboard",
    view: 16.0,
    body: r#"<rect x="1.8" y="4" width="12.4" height="8" rx="1.6" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M4.2 6.5H5.2M7.5 6.5H8.5M10.8 6.5H11.8M4.5 9.5H11.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

pub static COMMAND: Icon = Icon {
    name: "settings-command",
    view: 16.0,
    body: r#"<path d="M10 6V4.5A1.5 1.5 0 1 1 11.5 6H4.5A1.5 1.5 0 1 1 6 4.5V11.5A1.5 1.5 0 1 1 4.5 10H11.5A1.5 1.5 0 1 1 10 11.5V6Z" stroke="currentColor" stroke-width="1.3" fill="none"/>"#,
};

pub static LINK: Icon = Icon {
    name: "settings-link",
    view: 16.0,
    body: r#"<path d="M6.8 9.2L9.2 6.8M5.5 10.5L4.5 11.5A2.1 2.1 0 0 1 1.5 8.5L3.5 6.5A2.1 2.1 0 0 1 6.5 6.5M10.5 5.5L11.5 4.5A2.1 2.1 0 0 1 14.5 7.5L12.5 9.5A2.1 2.1 0 0 1 9.5 9.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" fill="none"/>"#,
};

pub static BELL: Icon = Icon {
    name: "settings-bell",
    view: 16.0,
    body: r#"<path d="M4.5 6.8A3.5 3.5 0 0 1 11.5 6.8C11.5 10 12.8 11 12.8 11H3.2C3.2 11 4.5 10 4.5 6.8ZM6.8 12.8A1.4 1.4 0 0 0 9.2 12.8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub static CPU: Icon = Icon {
    name: "settings-cpu",
    view: 16.0,
    body: r#"<rect x="4" y="4" width="8" height="8" rx="1.2" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M6.5 2V4M9.5 2V4M6.5 12V14M9.5 12V14M2 6.5H4M2 9.5H4M12 6.5H14M12 9.5H14" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

pub static BUG: Icon = Icon {
    name: "settings-bug",
    view: 16.0,
    body: r#"<rect x="5" y="4.5" width="6" height="8" rx="3" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M2.5 6.5H5M11 6.5H13.5M2.2 9H5M11 9H13.8M3 11.5L5 10.5M13 11.5L11 10.5M6.2 2.5L7 4.5M9.8 2.5L9 4.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

pub static UNDO: Icon = Icon {
    name: "settings-undo",
    view: 16.0,
    body: r#"<path d="M5.5 4L2.5 7L5.5 10M3 7H9.5A3.5 3.5 0 1 1 9.5 14H7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub static PLUS: Icon = Icon {
    name: "settings-plus",
    view: 16.0,
    body: r#"<path d="M8 3.5V12.5M3.5 8H12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>"#,
};

pub fn group_icon(id: &str) -> &'static Icon {
    match id {
        "sliders" => &SLIDERS,
        "window" => &WINDOW,
        "type" => &TYPE,
        "palette" => &PALETTE,
        "droplet" => &DROPLET,
        "cursor" => &CURSOR,
        "tabs" => &TABS,
        "layout" => &LAYOUT,
        "terminal" => &TERMINAL,
        "keyboard" => &KEYBOARD,
        "command" => &COMMAND,
        "link" => &LINK,
        "bell" => &BELL,
        "cpu" => &CPU,
        "bug" => &BUG,
        _ => &SLIDERS,
    }
}
