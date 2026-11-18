//! The browser playground's side of the window. There the config is text in memory: Open (or a
//! file dropped on the page) reads a file, Save downloads the result or copies it, and a banner
//! says where the file has to go. The browser itself (file picker, drops, download) comes in
//! through `Playground`, so this module has no web dependencies and the same window runs (and is
//! tested) natively.

use crate::state::{AsyncMsg, SettingsWindow};
use rune::{Color, El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit, ScrollBar};
use serde_json::Value;
use settings_core::{DiffOp, Document};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker as TaskWaker};

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

/// What the window needs from the browser. Without it the window edits files on disk.
#[derive(Clone, Copy)]
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

/// How the saved text leaves the browser.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Handoff {
    Download,
    Copy,
}

/// What happens once the user confirms discarding unsaved edits.
pub enum PendingOpen {
    PickFile,
    Dropped(OpenedFile),
}

/// The banner text: nothing reaches the terminal until the saved file is put in place.
pub const BANNER: &str =
    "Playground: nothing is saved to your terminal. Open or drop a config file to edit it; Save downloads or copies the result for you to put in place.";

impl SettingsWindow {
    /// Attaches browser playground callbacks and polls any already-queued dropped files.
    pub fn with_playground(mut self, playground: Playground) -> Self {
        self.playground = Some(playground);
        self.poll_drops();
        self
    }

    /// Asks the browser for a file and edits it, after confirming that unsaved edits may go.
    pub fn open_file(&mut self) {
        if self.busy || self.playground.is_none() {
            return;
        }
        if self.editor.dirty_count() > 0 {
            self.discard_prompt = Some(PendingOpen::PickFile);
            return;
        }
        self.pick_file_now();
    }

    /// A file dropped on the page: edits it, after confirming that unsaved edits may go.
    pub fn open_dropped(&mut self, file: OpenedFile) {
        if self.busy {
            return;
        }
        if self.editor.dirty_count() > 0 {
            self.discard_prompt = Some(PendingOpen::Dropped(file));
            return;
        }
        self.open_bytes(file);
    }

    /// Proceeds with the pending file open after the user confirms discarding unsaved changes.
    pub fn confirm_discard(&mut self) {
        match self.discard_prompt.take() {
            Some(PendingOpen::PickFile) => self.pick_file_now(),
            Some(PendingOpen::Dropped(file)) => self.open_bytes(file),
            None => {}
        }
    }

    fn pick_file_now(&mut self) {
        let Some(open) = self.playground.as_ref().map(|p| p.open_file) else { return };
        let fut = open();
        self.spawn_playground_task(Box::pin(async move { AsyncMsg::FilePicked(fut.await) }));
    }

    /// Edits an opened file's text, dropping unsaved edits. Text that isn't UTF-8 is refused.
    pub fn open_bytes(&mut self, file: OpenedFile) {
        match String::from_utf8(file.bytes) {
            Ok(text) => {
                self.save_preview = None;
                self.discard_prompt = None;
                self.edits.clear();
                self.combos.clear();
                self.record_rows.clear();
                self.errors.clear();
                self.recheck.clear();
                self.document = Document::memory(file.name, text);
                self.reload();
            }
            Err(_) => {
                self.toast = Some(format!(
                    "{} isn't UTF-8 text, so it can't be edited here.",
                    file.name
                ));
            }
        }
    }

    /// Saves by handing the text to the browser, starting during the click that confirmed it
    /// (browsers only allow a download or a clipboard write then). A copy the browser refuses
    /// saves nothing, so the edits stay.
    pub(crate) fn hand_off(&mut self, how: Handoff) {
        let Some(preview) = self.save_preview.take() else { return };
        let Some(playground) = &self.playground else { return };
        let (download, copy) = (playground.download, playground.copy);
        let name = self.file_name();
        match how {
            Handoff::Download => {
                let bom = matches!(self.document, Document::Memory { bom: true, .. });
                let text = if bom {
                    format!("\u{FEFF}{}", preview.after)
                } else {
                    preview.after
                };
                download(&name, text.as_bytes());
                self.save_in_memory(preview.changes, None);
            }
            Handoff::Copy => {
                let fut = copy(&preview.after);
                let changes = preview.changes;
                self.spawn_playground_task(Box::pin(async move {
                    AsyncMsg::CopyFinished {
                        ok: fut.await,
                        changes,
                        name,
                    }
                }));
            }
        }
    }

    pub(crate) fn save_in_memory(&mut self, changes: Vec<(String, Value)>, note: Option<String>) {
        let (version, scope, mut store) = (
            self.editor.state.version.clone(),
            self.editor.scope.clone(),
            self.store(),
        );
        let result = store.save(Some(&version), &scope, &changes).map(|r| (r, store.state()));
        let doc = store.document;
        self.finish_save(result, &changes, doc, note);
    }

    /// Polls `fut` once immediately (so synchronous test futures complete on the spot) and
    /// schedules pending futures on the browser microtask queue (`wasm32`) or `pending_tasks`.
    pub(crate) fn spawn_playground_task(&mut self, mut fut: Pin<Box<dyn Future<Output = AsyncMsg>>>) {
        let mut cx = Context::from_waker(TaskWaker::noop());
        if let Poll::Ready(msg) = fut.as_mut().poll(&mut cx) {
            self.handle_async_msg(msg);
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let tx = self.tx_async.clone();
            let waker = self.waker.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let msg = fut.await;
                let _ = tx.send(msg);
                if let Some(w) = waker {
                    w();
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending_tasks.push(fut);
        }
    }

    /// Polls any ready dropped files from `Playground::next_drop`.
    pub fn poll_drops(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(next_fn) = self.playground.as_ref().and_then(|p| p.next_drop) else {
                return false;
            };
            let mut cx = Context::from_waker(TaskWaker::noop());
            let mut changed = false;
            while let Poll::Ready(maybe_file) = next_fn().as_mut().poll(&mut cx) {
                if let Some(file) = maybe_file {
                    self.open_dropped(file);
                    changed = true;
                } else {
                    break;
                }
            }
            changed
        }
    }

    /// The always-visible strip under the header: nothing is saved, and where the file goes.
    pub(crate) fn render_playground_banner(&self, k: &Kit) -> Option<El> {
        let playground = self.playground.as_ref()?;
        let places: Vec<String> = playground
            .locations
            .iter()
            .map(|(platform, path)| format!("{platform}: {path}"))
            .collect();
        let message = format!("{BANNER} {} reads it from {}.", self.app.terminal, places.join(" · "));
        Some(
            row()
                .key("playground")
                .w_full()
                .px(20.0)
                .py(8.0)
                .fill(k.c.warning.bg)
                .border(1.0, k.c.warning.border)
                .center_y()
                .child(text(message).grow(1.0).font(k.ty.small).color(k.c.text_base).wrap()),
        )
    }

    /// Modal dialog asking whether to discard unsaved edits before opening another file.
    pub(crate) fn render_discard_dialog(&self, k: &Kit, layer: &mut Vec<El>) {
        if self.discard_prompt.is_none() {
            return;
        }
        let n = self.editor.dirty_count();
        let title = format!("Discard {n} unsaved change{}?", if n == 1 { "" } else { "s" });
        let footer = row()
            .w_full()
            .justify_end()
            .gap(8.0)
            .child(
                Button::new("Cancel")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Regular)
                    .on_press(|app: &mut Self| app.discard_prompt = None)
                    .build(k)
                    .key("cancel-discard"),
            )
            .child(
                Button::new("Discard")
                    .variant(ButtonVariant::Danger)
                    .control_size(ControlSize::Regular)
                    .on_press(|app: &mut Self| app.confirm_discard())
                    .build(k)
                    .default_button()
                    .key("confirm-discard"),
            );
        Dialog::new("discard-dialog", title)
            .open(true)
            .fit()
            .width(440.0)
            .description("Opening another file drops them.")
            .child(col().w_full().px(16.0).pb(16.0).child(footer))
            .on_close(|app: &mut Self| app.discard_prompt = None)
            .build(k, layer);
    }

    pub(crate) fn parse_error_message(&self, message: &str) -> String {
        if self.document.is_file() {
            format!("{message} Fix the file in your editor; this window updates when it's valid again.")
        } else {
            format!("{message} Fix the file in your editor, then open it again.")
        }
    }

    pub(crate) fn handle_async_msg(&mut self, msg: AsyncMsg) {
        match msg {
            AsyncMsg::PreviewReady(res) => self.finish_preview(res),
            AsyncMsg::SaveDone(done) => self.finish_save(done.result, &done.saved, done.document, None),
            AsyncMsg::ThemesLoaded(list) => self.set_themes(list),
            AsyncMsg::ThemeInstalled { key, req_id, theme, result } => {
                if self.theme_requests.get(&key).copied() == Some(req_id) {
                    match result {
                        Ok(downloaded) => {
                            if let Some(t) = self.themes.iter_mut().find(|t| t.name == theme) {
                                t.installed = true;
                            }
                            if let Some(o) = self.theme_options.iter_mut().find(|o| o.id == theme) {
                                o.detail.clear();
                            }
                            self.set_and_sync(&key, Value::String(theme.clone()));
                            self.toast = Some(match downloaded {
                                Some(file) => format!(
                                    "Downloaded {file}. Put it in the themes folder next to {}.",
                                    self.file_name()
                                ),
                                None => format!("Installed theme “{theme}”."),
                            });
                        }
                        Err(e) => {
                            self.toast = Some(e);
                            self.sync_setting(&key);
                        }
                    }
                }
            }
            AsyncMsg::FilePicked(Some(file)) => self.open_bytes(file),
            AsyncMsg::FilePicked(None) => {}
            AsyncMsg::FileDropped(file) => self.open_dropped(file),
            AsyncMsg::CopyFinished { ok, changes, name } => {
                if ok {
                    let note = format!("Copied. Paste it into {name} where your terminal reads it.");
                    self.save_in_memory(changes, Some(note));
                } else {
                    self.toast = Some("The browser didn't allow copying. Download the file instead.".into());
                }
            }
        }
    }

    pub fn poll_async(&mut self, now_sec: f64) -> bool {
        let mut changed = self.poll_drops();
        #[cfg(not(target_arch = "wasm32"))]
        if !self.pending_tasks.is_empty() {
            let mut cx = Context::from_waker(TaskWaker::noop());
            let mut ready = Vec::new();
            let tasks = std::mem::take(&mut self.pending_tasks);
            for mut fut in tasks {
                match fut.as_mut().poll(&mut cx) {
                    Poll::Ready(msg) => ready.push(msg),
                    Poll::Pending => self.pending_tasks.push(fut),
                }
            }
            for msg in ready {
                changed = true;
                self.handle_async_msg(msg);
            }
        }
        let msgs: Vec<AsyncMsg> = self.rx_async.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect();
        for msg in msgs {
            changed = true;
            self.handle_async_msg(msg);
        }
        if self.document.is_file() && !self.busy && now_sec - self.last_poll_sec >= 3.0 {
            self.last_poll_sec = now_sec;
            if self.check_disk_changes() {
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn render_save_dialog(&self, k: &Kit, layer: &mut Vec<El>) {
        let Some(preview) = &self.save_preview else { return };
        let playground = self.playground.is_some();
        let title = format!("Save changes to {}?", self.file_name());
        let mut diff_rows = col().key("diff-rows").w_full().py(6.0);
        for (i, d) in preview.diff.iter().enumerate() {
            let (marker, bg, fg) = match d.op {
                DiffOp::Add => ("+", k.c.success.bg, k.c.text_base),
                DiffOp::Remove => ("-", k.c.danger.bg, k.c.text_base),
                DiffOp::Context => (" ", Color::TRANSPARENT, k.c.text_muted),
                DiffOp::Gap => ("", Color::TRANSPARENT, k.c.text_faint),
            };
            let number = match d.op {
                DiffOp::Gap => "⋯".to_string(),
                DiffOp::Remove => String::new(),
                _ => d.line.to_string(),
            };
            let body = if d.op == DiffOp::Gap { String::new() } else { format!("{marker} {}", d.text) };
            diff_rows = diff_rows.child(
                row()
                    .key(format!("diff-line-{i}"))
                    .w_full()
                    .px(10.0)
                    .py(2.0)
                    .gap(8.0)
                    .fill(bg)
                    .child(text(number).w(32.0).font(k.ty.mono).color(k.c.text_faint))
                    .child(text(body).font(k.ty.mono).color(fg)),
            );
        }
        let mut diff_box =
            col().key("diff-scroll").w_full().radius(k.radius.md).fill(k.c.bg_deep).border(1.0, k.c.border_base);
        if preview.diff.len() > 10 {
            diff_box = diff_box.h(240.0).scroll(ScrollSpec { stick_bottom: false }).child(diff_rows).scroll_bar(k);
        } else {
            diff_box = diff_box.child(diff_rows);
        }

        let mut footer = row()
            .w_full()
            .justify_end()
            .gap(8.0)
            .child(
                Button::new("Cancel")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Regular)
                    .on_press(|app: &mut Self| app.save_preview = None)
                    .build(k)
                    .key("cancel-save"),
            );
        if playground {
            footer = footer
                .child(
                    Button::new("Copy")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Regular)
                        .help("Copy the file to paste in place")
                        .on_press(|app: &mut Self| app.hand_off(Handoff::Copy))
                        .build(k)
                        .key("confirm-copy"),
                )
                .child(
                    Button::new("Download")
                        .variant(ButtonVariant::Contrast)
                        .control_size(ControlSize::Regular)
                        .on_press(|app: &mut Self| app.hand_off(Handoff::Download))
                        .build(k)
                        .default_button()
                        .key("confirm-save"),
                );
        } else {
            footer = footer.child(
                Button::new("Save")
                    .variant(ButtonVariant::Contrast)
                    .control_size(ControlSize::Regular)
                    .on_press(|app: &mut Self| app.confirm_save())
                    .build(k)
                    .default_button()
                    .key("confirm-save"),
            );
        }

        Dialog::new("save-dialog", title)
            .open(true)
            .fit()
            .width(680.0)
            .description("Only these lines change. Everything else in the file stays as it is.")
            .child(col().w_full().px(16.0).pb(16.0).gap(12.0).child(diff_box).child(footer))
            .on_close(|app: &mut Self| app.save_preview = None)
            .build(k, layer);
    }
}
