//! Ghostty's look in the Rune editor, shared by the desktop app and the browser playground.

use settings_rune::{Brand, Palette};

pub const BRAND: Brand = Brand {
    light: Palette { bg: "#ededf1", fg: "#111113", accent: "#3551f3", sidebar: "#e7e7eb" },
    dark: Palette { bg: "#0f0f11", fg: "#eaeaeb", accent: "#3551f3", sidebar: "#0b0b0d" },
    logo: include_bytes!("../../../../brand/ghostty-logo.png"),
};
