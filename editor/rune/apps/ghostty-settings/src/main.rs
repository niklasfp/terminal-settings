//! ghostty-settings on Rune (spike): the same `AppDefinition` the GPUI app runs, drawn by Rune.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    settings_rune::run(
        ghostty_config::app(),
        settings_rune::Brand {
            light: settings_rune::Palette {
                bg: "#ededf1",
                fg: "#111113",
                accent: "#3551f3",
                sidebar: "#e7e7eb",
            },
            dark: settings_rune::Palette {
                bg: "#0f0f11",
                fg: "#eaeaeb",
                accent: "#3551f3",
                sidebar: "#0b0b0d",
            },
            logo: include_bytes!("../../../../brand/ghostty-logo.png"),
        },
    );
}
