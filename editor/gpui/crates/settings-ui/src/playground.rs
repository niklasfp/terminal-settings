//! The browser playground's side of the window. There the config is text in memory: Open (or a
//! file dropped on the page) reads a file, Save downloads the result or copies it, and a banner
//! says where the file has to go. The browser itself (file picker, drops, download) comes in
//! through `Playground`, so this module has no web dependencies and the same window runs (and is
//! tested) natively.

use crate::view::SettingsWindow;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;
use serde_json::Value;
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

/// Text being copied to the clipboard: resolves to whether it was.
pub type Copied = Pin<Box<dyn Future<Output = bool>>>;

/// A theme file being downloaded: resolves to the downloaded file's name, or a user-facing error.
pub type ThemeDownload = Pin<Box<dyn Future<Output = Result<String, String>>>>;

/// What the window needs from the browser. Set as a global; without it the window edits files.
pub struct Playground {
    /// What a new config is called (and downloaded as) until a file is opened, e.g. "config.toml".
    pub file_name: &'static str,
    /// Where the terminal reads its config, as (platform, path), for the banner.
    pub locations: &'static [(&'static str, &'static str)],
    /// Shows the browser's file picker. `None` when the user cancels.
    pub open_file: fn() -> PickedFile,
    /// Waits for the next file dropped on the page (`None` when one couldn't be read). `None` when
    /// files can't be dropped.
    pub next_drop: Option<fn() -> PickedFile>,
    /// Hands `bytes` to the browser as a download called `name`.
    pub download: fn(&str, &[u8]),
    /// Puts text on the clipboard; resolves to whether the browser allowed it.
    pub copy: fn(&str) -> Copied,
    /// Downloads a theme the app lists as not installed (its file has to be put in place, like the
    /// config). `None` when every listed theme works by name.
    pub install_theme: Option<fn(&str) -> ThemeDownload>,
}

impl Global for Playground {}

/// How the saved text leaves the browser.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handoff {
    Download,
    Copy,
}

/// The banner text: nothing reaches the terminal until the saved file is put in place.
pub const BANNER: &str =
    "Playground: nothing is saved to your terminal. Open or drop a config file to edit it; Save downloads or copies the result for you to put in place.";

impl SettingsWindow {
    pub(crate) fn playground<'a>(&self, cx: &'a App) -> Option<&'a Playground> {
        cx.try_global::<Playground>()
    }

    /// Asks the browser for a file and edits it, after confirming that unsaved edits may go.
    pub(crate) fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.playground(cx).map(|p| p.open_file) else { return };
        // Browsers only show a file picker during a click, so with nothing to discard it opens
        // here, not in a task; after a Discard click it's still within that click's activation.
        self.discard_then(window, cx, move |_, window, cx| {
            let picked = open();
            cx.spawn_in(window, async move |this, cx| {
                let Some(file) = picked.await else { return };
                this.update_in(cx, |this, window, cx| this.open_bytes(file, window, cx)).ok();
            })
            .detach();
        });
    }

    /// A file dropped on the page: edits it, after confirming that unsaved edits may go.
    pub fn open_dropped(&mut self, file: OpenedFile, window: &mut Window, cx: &mut Context<Self>) {
        self.discard_then(window, cx, move |this, window, cx| this.open_bytes(file, window, cx));
    }

    /// Runs `then` now when there's nothing unsaved, or once the user agrees to discard it.
    fn discard_then(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.busy {
            return;
        }
        let n = self.dirty_count();
        if n == 0 {
            then(self, window, cx);
            return;
        }
        let message = format!("Discard {n} unsaved change{}?", if n == 1 { "" } else { "s" });
        let answer = window.prompt(PromptLevel::Warning, &message, Some("Opening another file drops them."), &["Discard", "Cancel"], cx);
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| then(this, window, cx)).ok();
        })
        .detach();
    }

    /// Listens for files dropped on the page while the window is open.
    pub(crate) fn watch_drops(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<()>> {
        let next = self.playground(cx)?.next_drop?;
        Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let Some(file) = next().await else { continue };
                if this.update_in(cx, |this, window, cx| this.open_dropped(file, window, cx)).is_err() {
                    break;
                }
            }
        }))
    }

    /// Edits an opened file's text, dropping unsaved edits. Text that isn't UTF-8 is refused.
    pub fn open_bytes(&mut self, file: OpenedFile, window: &mut Window, cx: &mut Context<Self>) {
        match String::from_utf8(file.bytes) {
            Ok(text) => {
                // A save dialog still open belongs to the previous file.
                window.close_dialog(cx);
                self.clear_pending_entries(window, cx);
                self.document = Document::memory(file.name, text);
                self.reload(window, cx);
            }
            Err(_) => window.push_notification(
                Notification::error(format!("{} isn't UTF-8 text, so it can't be edited here.", file.name)),
                cx,
            ),
        }
    }

    /// Saves by handing the text to the browser, starting during the click that confirmed it
    /// (browsers only allow a download or a clipboard write then). A copy the browser refuses
    /// saves nothing, so the edits stay.
    pub(crate) fn hand_off(&mut self, how: Handoff, changes: Vec<(String, Value)>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(playground) = self.playground(cx) else { return };
        let (download, copy) = (playground.download, playground.copy);
        let name = self.file_name();
        match how {
            Handoff::Download => {
                // The file keeps a byte-order mark it had.
                let bom = matches!(self.document, Document::Memory { bom: true, .. });
                let text = if bom { format!("\u{FEFF}{text}") } else { text.to_string() };
                download(&name, text.as_bytes());
                self.confirm_save(changes, None, window, cx);
            }
            Handoff::Copy => {
                let copied = copy(text);
                cx.spawn_in(window, async move |this, cx| {
                    let ok = copied.await;
                    this.update_in(cx, |this, window, cx| {
                        if ok {
                            let note = format!("Copied. Paste it into {name} where your terminal reads it.");
                            this.confirm_save(changes, Some(note), window, cx);
                        } else {
                            let message = "The browser didn't allow copying. Download the file instead.";
                            window.push_notification(Notification::error(message), cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
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
