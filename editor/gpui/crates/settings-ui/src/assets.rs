//! Embedded assets: the component icons, the extra Lucide icons the settings window uses, and the
//! running app's logo.

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        AppWindow,
        Bell,
        Bug,
        CircleX,
        Command,
        Cpu,
        Droplet,
        File,
        FileImage,
        FileMusic,
        Folder,
        FolderOpen,
        FolderSearch,
        Keyboard,
        LayoutPanelLeft,
        Link,
        Monitor,
        Moon,
        MousePointer2,
        Palette,
        PanelsTopLeft,
        Plus,
        SlidersHorizontal,
        SquareTerminal,
        Sun,
        TextCursorInput,
        TriangleAlert,
        Type,
        Undo2,
        Download
    ]
);

// The component icons. Native embeds them through gpui-kit. On the web gpui-kit would download
// them from a server at runtime, so the same default set is embedded here instead.
#[cfg(not(target_family = "wasm"))]
use gpui_kit::assets::Assets as ComponentIcons;

#[cfg(target_family = "wasm")]
macro_rules! component_icons {
    ($($variant:ident => $path:literal),* $(,)?) => {
        gpui_kit::assets::icon_assets!(ComponentIcons, [$($variant),*]);
    };
}

#[cfg(target_family = "wasm")]
gpui_kit::assets::__component_icon_names!(component_icons);

/// Where the logo is served from; the extension tells GPUI how to decode it.
pub fn logo_path(logo: &[u8]) -> &'static str {
    if is_svg(logo) { "brand/logo.svg" } else { "brand/logo.png" }
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]).to_string();
    head.trim_start().starts_with('<') && head.contains("<svg")
}

/// An SVG logo without its root `width`/`height`. Rio's shared file declares 3000×3000 for the web,
/// and GPUI rasterizes SVGs at their declared size times the display scale: 6000×6000 RGBA is
/// ~144 MB of RAM plus the same again on the GPU. Without them the viewBox sets the size.
fn sized_logo(logo: &'static [u8]) -> Cow<'static, [u8]> {
    if is_svg(logo) {
        Cow::Owned(strip_root_size(&String::from_utf8_lossy(logo)).into_bytes())
    } else {
        Cow::Borrowed(logo)
    }
}

fn strip_root_size(svg: &str) -> String {
    let Some(start) = svg.find("<svg") else { return svg.to_string() };
    let Some(len) = svg[start..].find('>') else { return svg.to_string() };
    let (head, rest) = svg.split_at(start + len);
    let mut tag = head[start..].to_string();
    for attr in ["width", "height"] {
        let needle = format!(" {attr}=\"");
        if let Some(at) = tag.find(&needle)
            && let Some(end) = tag[at + needle.len()..].find('"')
        {
            tag.replace_range(at..at + needle.len() + end + 1, "");
        }
    }
    format!("{}{tag}{rest}", &head[..start])
}

/// The asset source: the app's logo, the extra icons, then GPUI Kit's own assets.
pub struct AppAssets {
    pub logo: &'static [u8],
}

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == logo_path(self.logo) {
            return Ok(Some(sized_logo(self.logo)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        ComponentIcons.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ComponentIcons.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

/// The sidebar icon for an overlay group icon id.
pub fn group_icon(id: &str) -> gpui_kit::assets::IconName {
    use gpui_kit::assets::IconName as I;
    match id {
        "sliders" => I::SlidersHorizontal,
        "window" => I::AppWindow,
        "type" => I::Type,
        "palette" => I::Palette,
        "droplet" => I::Droplet,
        "cursor" => I::TextCursorInput,
        "tabs" => I::PanelsTopLeft,
        "layout" => I::LayoutPanelLeft,
        "terminal" => I::SquareTerminal,
        "keyboard" => I::Keyboard,
        "command" => I::Command,
        "link" => I::Link,
        "bell" => I::Bell,
        "cpu" => I::Cpu,
        "bug" => I::Bug,
        _ => I::SlidersHorizontal,
    }
}

#[cfg(test)]
mod tests {
    const RIO: &[u8] = include_bytes!("../../../../brand/rio-logo.svg");
    const GHOSTTY: &[u8] = include_bytes!("../../../../brand/ghostty-logo.png");

    #[test]
    fn logos_are_served_by_format() {
        assert_eq!(super::logo_path(RIO), "brand/logo.svg");
        assert_eq!(super::logo_path(GHOSTTY), "brand/logo.png");
        assert_eq!(super::sized_logo(GHOSTTY).as_ref(), GHOSTTY, "PNGs pass through");
    }

    #[test]
    fn svg_logo_loses_its_declared_size_but_keeps_the_view_box() {
        let svg = String::from_utf8(super::sized_logo(RIO).to_vec()).unwrap();
        let root = &svg[..svg.find('>').unwrap()];
        assert!(root.contains("viewBox=\"0 0 48 48\""), "{root}");
        assert!(!root.contains("width=") && !root.contains("height="), "{root}");
        assert!(svg.contains("<path"), "the body is untouched");
    }
}
