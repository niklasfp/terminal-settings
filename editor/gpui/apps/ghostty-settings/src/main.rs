//! ghostty-settings: a native settings window for Ghostty's config file.

// A GUI program on Windows: no console window opens beside it. (Ghostty has no Windows version,
// so this app doesn't ship there; it still builds there for the workspace tests.)
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    settings_ui::run(
        ghostty_config::app(),
        settings_ui::Brand {
            theme_json: include_str!("../ghostty-theme.json"),
            light_theme: "Ghostty Light",
            dark_theme: "Ghostty Dark",
            logo: include_bytes!("../../../../brand/ghostty-logo.png"),
        },
    );
}
