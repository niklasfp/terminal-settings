//! The path field every file and folder setting uses, in both apps: the text stays editable, and
//! the field carries its own small buttons (like an IDE's path field) instead of a separate big one:
//! choose (native dialog), show in the file manager when the path exists, a warning when it
//! doesn't. Files and folders dragged onto the field from the file manager are taken too.

use crate::view::SettingsWindow;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, button::{Button, ButtonVariants as _}, h_flex, tooltip::Tooltip};
use gpui_kit::*;
use serde_json::Value;
use settings_core::{Setting, SettingType};
use std::path::PathBuf;

/// What a path setting points at, from the overlay's `pick`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    Folder,
    File,
    Image,
    Audio,
}

impl PathKind {
    pub fn of(s: &Setting) -> Option<Self> {
        Some(match s.pick.as_deref()? {
            "folder" => Self::Folder,
            "image" => Self::Image,
            "audio" => Self::Audio,
            _ => Self::File,
        })
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::File => "file",
            Self::Image => "image",
            Self::Audio => "sound",
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Self::Folder => Lucide::Folder,
            Self::File => Lucide::File,
            Self::Image => Lucide::FileImage,
            Self::Audio => Lucide::FileMusic,
        }
    }
}

/// The path a value names, with `~` expanded; `None` for values that aren't paths (e.g. Ghostty's
/// `working-directory = home`, or a filter name).
pub fn as_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("~/").or((text == "~").then_some("")) {
        return std::env::home_dir().map(|home| home.join(rest));
    }
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

/// What the system file manager is called, for "Show in …".
fn file_manager() -> &'static str {
    if cfg!(target_os = "macos") {
        "Finder"
    } else if cfg!(windows) {
        "Explorer"
    } else {
        "Files"
    }
}

/// "this Mac" / "this computer", for the not-found warning.
fn this_machine() -> &'static str {
    if cfg!(target_os = "macos") { "this Mac" } else { "this computer" }
}

impl SettingsWindow {
    /// The field's leading kind icon.
    pub(crate) fn path_prefix(kind: PathKind, cx: &App) -> impl IntoElement {
        Icon::new(kind.icon()).xsmall().text_color(cx.theme().muted_foreground)
    }

    /// The field's trailing buttons for its current `text`: a warning or "Show in Finder", then
    /// "Choose…".
    pub(crate) fn path_suffix(&self, s: &Setting, kind: PathKind, text: &str, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
        if !self.document.is_file() {
            // A config held in memory is for another machine: its paths can't be checked or chosen here.
            return div().into_any_element();
        }
        let key = s.key.clone();
        let id = |part: &str| ElementId::Name(format!("{}:{part}", s.key).into());
        let path = as_path(text);
        let status: Option<AnyElement> = match path {
            Some(path) if path.exists() => Some(
                Button::new(id("reveal"))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::FolderSearch)
                    .tooltip(format!("Show in {}", file_manager()))
                    .on_click(move |_, _, cx| cx.reveal_path(&path))
                    .into_any_element(),
            ),
            Some(_) => {
                let warning = format!("No such {} on {}", kind.noun(), this_machine());
                Some(
                    div()
                        .id(id("missing"))
                        .test_support()
                        .child(Icon::new(Lucide::TriangleAlert).xsmall().text_color(cx.theme().warning))
                        .tooltip(move |window, cx| Tooltip::new(warning.clone()).build(window, cx))
                        .into_any_element(),
                )
            }
            None => None,
        };
        h_flex()
            .gap_0p5()
            .children(status)
            .child(
                Button::new(id("browse"))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::FolderOpen)
                    .tooltip(format!("Choose {}…", kind.noun()))
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| this.browse(&key, kind, window, cx))),
            )
            .into_any_element()
    }

    /// Wraps a path setting's control so files and folders dropped from Finder land in it.
    pub(crate) fn path_drop_target(&self, s: &Setting, control: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        if !self.document.is_file() {
            return control;
        }
        let key = s.key.clone();
        let highlight = cx.theme().drop_target;
        div()
            .id(ElementId::Name(format!("{}:drop", s.key).into()))
            .rounded(cx.theme().radius)
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(highlight))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                this.use_paths(&key, paths.paths().to_vec(), window, cx);
            }))
            .child(control)
            .into_any_element()
    }

    /// Opens the native dialog for a file or folder.
    fn browse(&mut self, key: &str, kind: PathKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: kind != PathKind::Folder,
            directories: kind == PathKind::Folder,
            multiple: s.ty == SettingType::Strings,
            prompt: Some(format!("Choose {}", s.label.to_lowercase()).into()),
        });
        let key = key.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            this.update_in(cx, |this, window, cx| this.use_paths(&key, paths, window, cx)).ok();
        })
        .detach();
    }

    /// Puts chosen or dropped paths into the setting: a list gains them, a single path takes the first.
    pub(crate) fn use_paths(&mut self, key: &str, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let mut paths = paths.into_iter().map(|p| Value::String(p.display().to_string()));
        if s.ty == SettingType::Strings {
            let mut items = self.editor.current_value(s).as_array().cloned().unwrap_or_default();
            items.extend(paths);
            self.set_and_sync(key, Value::Array(items), window, cx);
        } else if let Some(path) = paths.next() {
            self.set_and_sync(key, path, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::as_path;

    #[test]
    fn only_absolute_and_home_paths_are_paths() {
        let absolute = std::env::temp_dir().join("x"); // "/tmp/x" isn't absolute on Windows
        assert_eq!(as_path(&absolute.display().to_string()), Some(absolute));
        assert!(as_path("~/x").is_some_and(|p| p.ends_with("x") && p.is_absolute()));
        assert_eq!(as_path("home"), None);
        assert_eq!(as_path("NewPixieCrt"), None);
        assert_eq!(as_path(""), None);
    }
}
