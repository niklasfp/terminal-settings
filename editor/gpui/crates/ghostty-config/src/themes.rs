//! Ghostty ships its themes (key = value files) with the app, and also reads `<config dir>/themes`.
//! Everything listed is installed; the files give the picker its preview colors.

use crate::values::hex_color;
use settings_core::{ThemeInfo, theme_list, themes_dir};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The user's themes, then bundled ones they don't override; each sorted by name, ignoring case.
pub fn list(config_path: &Path, binary: Option<&Path>) -> Vec<ThemeInfo> {
    static BUNDLED: OnceLock<Vec<ThemeInfo>> = OnceLock::new();
    let bundled = BUNDLED.get_or_init(|| {
        bundled_dirs(binary)
            .iter()
            .flat_map(|d| read_dir(d))
            .collect()
    });
    list_from(config_path, bundled)
}

fn list_from(config_path: &Path, bundled: &[ThemeInfo]) -> Vec<ThemeInfo> {
    theme_list(read_dir(&themes_dir(config_path)), bundled.iter().cloned())
}

/// macOS: Ghostty.app/Contents/Resources/ghostty/themes; elsewhere: `<prefix>/share/ghostty/themes`.
fn bundled_dirs(binary: Option<&Path>) -> Vec<PathBuf> {
    let Some(prefix) = binary
        .and_then(|b| std::path::absolute(b).ok())
        .and_then(|b| b.parent().and_then(Path::parent).map(Path::to_path_buf))
    else {
        return Vec::new();
    };
    vec![
        prefix.join("Resources").join("ghostty").join("themes"),
        prefix.join("share").join("ghostty").join("themes"),
    ]
}

fn read_dir(dir: &Path) -> Vec<ThemeInfo> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let bytes = fs::read(e.path()).ok()?;
            Some(read_theme(
                e.file_name().to_string_lossy().into_owned(),
                &String::from_utf8_lossy(&bytes),
            ))
        })
        .collect()
}

fn read_theme(name: String, text: &str) -> ThemeInfo {
    let mut theme = ThemeInfo {
        name,
        installed: true,
        background: None,
        foreground: None,
        ansi: vec![None; 8],
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "palette" => {
                // palette = N=#rrggbb, for the 8 normal colors.
                if let Some((index, color)) = value.split_once('=')
                    && let Ok(i @ 0..=7) = index.trim().parse::<usize>()
                    && index.trim().len() == 1
                    && let Some(hex) = hex_color(color.trim())
                {
                    theme.ansi[i] = Some(hex);
                }
            }
            "background" => theme.background = hex_color(value.trim()).or(theme.background),
            "foreground" => theme.foreground = hex_color(value.trim()).or(theme.foreground),
            _ => {}
        }
    }
    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_base_and_palette_colors() {
        let t = read_theme(
            "Mine".into(),
            "# c\nbackground = 282A36\nforeground=#f8f8f2\npalette = 1=#FF5555\npalette = 12=#000000\npalette = 9 = #111111\nbackground = nope\n",
        );
        assert!(t.installed);
        assert_eq!(t.background.as_deref(), Some("#282a36"));
        assert_eq!(t.foreground.as_deref(), Some("#f8f8f2"));
        assert_eq!(t.ansi.len(), 8);
        assert_eq!(t.ansi[1].as_deref(), Some("#ff5555"));
        assert!(
            t.ansi
                .iter()
                .enumerate()
                .all(|(i, c)| i == 1 || c.is_none())
        );
    }

    #[test]
    fn user_themes_come_first_and_override_bundled_ones() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("themes");
        fs::create_dir_all(&themes).unwrap();
        fs::write(themes.join("zed"), "background = #000000\n").unwrap();
        fs::write(themes.join("Dracula"), "background = #111111\n").unwrap();
        fs::write(themes.join(".hidden"), "").unwrap();
        fs::create_dir(themes.join("folder")).unwrap();
        let bundled = vec![
            read_theme("Nord".into(), ""),
            read_theme("Dracula".into(), "background = #282a36\n"),
            read_theme("alabaster".into(), ""),
        ];
        let list = list_from(&dir.path().join("config.ghostty"), &bundled);
        let names: Vec<&str> = list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Dracula", "zed", "alabaster", "Nord"]);
        assert_eq!(list[0].background.as_deref(), Some("#111111"));
    }

    #[test]
    fn bundled_themes_have_preview_colors_when_ghostty_is_installed() {
        let bin = Path::new("/Applications/Ghostty.app/Contents/MacOS/ghostty");
        if !bin.is_file() {
            eprintln!("skipped: Ghostty isn't installed");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let themes = list(&dir.path().join("config"), Some(bin));
        assert!(themes.len() > 500, "{} themes", themes.len());
        let dracula: Vec<&ThemeInfo> = themes.iter().filter(|t| t.name == "Dracula").collect();
        assert_eq!(dracula.len(), 1);
        assert!(dracula[0].installed);
        assert_eq!(dracula[0].background.as_deref(), Some("#282a36"));
        assert_eq!(dracula[0].ansi[1].as_deref(), Some("#ff5555"));
    }
}
