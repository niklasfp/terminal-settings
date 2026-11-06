//! Rio's look in the Rune editor, shared by the desktop app and the browser playground.

use settings_rune::{Brand, Palette};

pub const BRAND: Brand = Brand {
    light: Palette { bg: "#f4f4f5", fg: "#18181b", accent: "#c026d3", sidebar: "#e4e4e7" },
    dark: Palette { bg: "#1b1b1d", fg: "#e4e4e7", accent: "#e879f9", sidebar: "#18181b" },
    logo: include_bytes!("../../../../brand/rio-logo.svg"),
};
