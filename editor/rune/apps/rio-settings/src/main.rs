//! rio-settings on Rune (spike): the same `AppDefinition` the GPUI app runs, drawn by Rune.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    settings_rune::run(
        rio_config::app(),
        settings_rune::Brand {
            light: settings_rune::Palette {
                bg: "#f4f4f5",
                fg: "#18181b",
                accent: "#c026d3",
                sidebar: "#e4e4e7",
            },
            dark: settings_rune::Palette {
                bg: "#1b1b1d",
                fg: "#e4e4e7",
                accent: "#e879f9",
                sidebar: "#18181b",
            },
            logo: include_bytes!("../../../../brand/rio-logo.svg"),
        },
    );
}
