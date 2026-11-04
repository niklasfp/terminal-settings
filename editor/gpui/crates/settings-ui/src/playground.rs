//! The browser playground's side of the window. There the config is text in memory: Open reads a
//! file the user picks, Save downloads the result, and a banner says where the file has to go.
//! The browser itself (file picker, download) comes in through `Playground`, so this module has no
//! web dependencies and the same window runs (and is tested) natively.

use crate::view::SettingsWindow;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;
use settings_core::Document;
use std::future::Future;
use std::pin::Pin;

/// A file the user opened: its name and bytes.
pub struct OpenedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A file being picked: resolves to the file, or `None` when the user cancels.
pub type PickedFile = Pin<Box<dyn Future<Output = Option<OpenedFile>>>>;

/// What the window needs from the browser. Set as a global; without it the window edits files.
pub struct Playground {
    /// What a new config is called (and downloaded as) until a file is opened, e.g. "config.toml".
    pub file_name: &'static str,
    /// Where the terminal reads its config, as (platform, path), for the banner.
    pub locations: &'static [(&'static str, &'static str)],
    /// Shows the browser's file picker. `None` when the user cancels.
    pub open_file: fn() -> PickedFile,
    /// Hands `bytes` to the browser as a download called `name`.
    pub download: fn(&str, &[u8]),
}

impl Global for Playground {}

/// The banner text: nothing reaches the terminal until the downloaded file is put in place.
pub const BANNER: &str = "Playground: nothing is saved to your terminal. Download the file and put it in place.";

impl SettingsWindow {
    pub(crate) fn playground<'a>(&self, cx: &'a App) -> Option<&'a Playground> {
        cx.try_global::<Playground>()
    }

    /// Asks the browser for a file and edits it, after confirming that unsaved edits may go.
    pub(crate) fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.playground(cx).map(|p| p.open_file) else { return };
        let n = self.dirty_count();
        let discard = (n > 0).then(|| {
            let message = format!("Discard {n} unsaved change{}?", if n == 1 { "" } else { "s" });
            window.prompt(PromptLevel::Warning, &message, Some("Opening another file drops them."), &["Discard", "Cancel"], cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Some(answer) = discard
                && answer.await != Ok(0)
            {
                return;
            }
            let Some(file) = open().await else { return };
            this.update_in(cx, |this, window, cx| this.open_bytes(file, window, cx)).ok();
        })
        .detach();
    }

    /// Edits an opened file's text, dropping unsaved edits. Text that isn't UTF-8 is refused.
    pub fn open_bytes(&mut self, file: OpenedFile, window: &mut Window, cx: &mut Context<Self>) {
        match String::from_utf8(file.bytes) {
            Ok(text) => {
                self.document = Document::memory(file.name, text);
                self.reload(window, cx);
            }
            Err(_) => window.push_notification(
                Notification::error(format!("{} isn't UTF-8 text, so it can't be edited here.", file.name)),
                cx,
            ),
        }
    }

    /// After a save in memory: the browser downloads the new text.
    pub(crate) fn download(&self, cx: &App) {
        let Some(download) = self.playground(cx).map(|p| p.download) else { return };
        if let (Some(name), Some(contents)) = (self.document.file_name(), self.document.contents()) {
            download(&name, contents.as_bytes());
        }
    }
}

impl SettingsWindow {
    /// The always-visible strip under the header: nothing is saved, and where the file goes.
    pub(crate) fn render_playground_banner(&self, cx: &App) -> Option<AnyElement> {
        let playground = self.playground(cx)?;
        let places: Vec<String> = playground.locations.iter().map(|(platform, path)| format!("{platform}: {path}")).collect();
        // A banner-style alert shows only its message, so the whole text goes there.
        let message = format!("{BANNER} {} reads it from {}.", self.app.terminal, places.join(" · "));
        Some(
            gpui_kit::component::alert::Alert::warning("playground", message)
                .banner()
                .into_any_element(),
        )
    }
}
