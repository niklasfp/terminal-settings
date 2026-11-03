//! rio-settings: a native settings window for Rio's config.toml.

// A GUI program on Windows: no console window opens beside it. Output still reaches a pipe or a
// redirect (`rio-settings --version > file`), which is how CI checks it.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    settings_ui::run(
        rio_config::app(),
        settings_ui::Brand {
            theme_json: include_str!("../rio-theme.json"),
            light_theme: "Rio Light",
            dark_theme: "Rio Dark",
            logo: include_bytes!("../../../../brand/rio-logo.svg"),
        },
    );
}
