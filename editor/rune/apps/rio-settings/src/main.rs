//! rio-settings on Rune (spike): the same `AppDefinition` the GPUI app runs, drawn by Rune.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    settings_rune::run(rio_config::app(), rio_settings_rune::BRAND);
}
