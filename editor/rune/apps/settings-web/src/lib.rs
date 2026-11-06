//! settings-web-rune: the browser playground on Rune. One WebAssembly module runs either editor,
//! picked by the page's `?app=rio|ghostty`, over a config held in memory. Opening reads a file
//! the user picks or drops on the page; saving downloads it (or the editor copies it). Everything
//! here is the browser's side (`settings_rune::playground::Playground`). Rune bundles its fonts
//! (Inter and IBM Plex Mono) on the web, so unlike the GPUI playground nothing is fetched first.

#![cfg(target_arch = "wasm32")]

use settings_rune::playground::{Copied, OpenedFile, PickedFile, Playground, ThemeDownload};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(start)]
pub fn start() {
    // Rune draws over the whole page from here on.
    if let Some(status) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("status")) {
        status.remove();
    }
    listen_for_drops();
    if query_app().as_deref() == Some("ghostty") {
        settings_rune::run_web(
            ghostty_config::browser_app(),
            ghostty_settings_rune::BRAND,
            playground(ghostty_config::CONFIG_FILE_NAME, ghostty_config::CONFIG_LOCATIONS, None),
        );
    } else {
        settings_rune::run_web(
            rio_config::browser_app(),
            rio_settings_rune::BRAND,
            playground(rio_config::CONFIG_FILE_NAME, rio_config::CONFIG_LOCATIONS, Some(download_rio_theme)),
        );
    }
}

/// The body of a GET, or `Err(url)` when it fails.
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let fail = |_| url.to_string();
    let window = web_sys::window().ok_or_else(|| url.to_string())?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url)).await.map_err(fail)?.dyn_into().map_err(fail)?;
    if !response.ok() {
        return Err(url.to_string());
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(fail)?).await.map_err(fail)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

fn playground(
    file_name: &'static str,
    locations: &'static [(&'static str, &'static str)],
    install_theme: Option<fn(&str) -> ThemeDownload>,
) -> Playground {
    Playground { file_name, locations, open_file, next_drop: Some(next_drop), download, copy, install_theme }
}

/// Rio needs a theme's file next to the config: fetch it from the catalogue's pinned commit and
/// hand it to the user as `<name>.toml`.
fn download_rio_theme(name: &str) -> ThemeDownload {
    let name = name.to_string();
    Box::pin(async move {
        let url = rio_config::theme_url(&name)?;
        let bytes = fetch_bytes(&url).await.map_err(|_| format!("Couldn't download the theme '{name}'."))?;
        let file = format!("{name}.toml");
        download(&file, &bytes);
        Ok(file)
    })
}

/// `?app=` from the page's address.
fn query_app() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("app=").map(str::to_string))
}

/// Shows the browser's file picker. The picker opens right away (browsers only allow it during a
/// click); the future resolves once a file is chosen and read, or with `None` when it's cancelled.
fn open_file() -> PickedFile {
    let Some(picker) = Picker::show() else { return Box::pin(async { None }) };
    Box::pin(async move {
        let mut picker = picker;
        let picked = (&mut picker.chosen).await.unwrap_or(false);
        if !picked {
            return None;
        }
        let file = picker.input.files()?.get(0)?;
        let buffer = JsFuture::from(file.array_buffer()).await.ok()?;
        Some(OpenedFile { name: file.name(), bytes: js_sys::Uint8Array::new(&buffer).to_vec() })
        // `picker` drops here: the input leaves the page and its listeners are freed.
    })
}

/// A hidden `<input type=file>`, already clicked, and a channel that says whether a file was
/// chosen. Dropping it removes the input and frees its listeners.
struct Picker {
    input: web_sys::HtmlInputElement,
    chosen: futures_channel::oneshot::Receiver<bool>,
    _listeners: Vec<Closure<dyn FnMut()>>,
}

impl Picker {
    fn show() -> Option<Self> {
        let document = web_sys::window()?.document()?;
        let input: web_sys::HtmlInputElement = document.create_element("input").ok()?.dyn_into().ok()?;
        input.set_type("file");
        input.style().set_property("display", "none").ok()?;
        let (tx, chosen) = futures_channel::oneshot::channel();
        let tx = Rc::new(RefCell::new(Some(tx)));
        let mut listeners = Vec::new();
        for (event, picked) in [("change", true), ("cancel", false)] {
            let tx = tx.clone();
            let listener = Closure::<dyn FnMut()>::new(move || {
                if let Some(tx) = tx.borrow_mut().take() {
                    let _ = tx.send(picked);
                }
            });
            input.add_event_listener_with_callback(event, listener.as_ref().unchecked_ref()).ok()?;
            listeners.push(listener);
        }
        document.body()?.append_child(&input).ok()?;
        let picker = Self { input, chosen, _listeners: listeners };
        picker.input.click();
        Some(picker)
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.input.remove();
    }
}

thread_local! {
    /// Where the next file dropped on the page goes: the window waiting for it.
    static DROP_WAITER: RefCell<Option<futures_channel::oneshot::Sender<Option<OpenedFile>>>> = const { RefCell::new(None) };
}

/// Waits for the next file dropped on the page.
fn next_drop() -> PickedFile {
    let (tx, rx) = futures_channel::oneshot::channel();
    DROP_WAITER.with(|waiter| *waiter.borrow_mut() = Some(tx));
    Box::pin(async move { rx.await.ok().flatten() })
}

/// Files can be dropped anywhere on the page, for as long as it's open. Cancelling the browser's
/// own handling stops it opening the file in the tab.
fn listen_for_drops() {
    let Some(window) = web_sys::window() else { return };
    let over = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|event: web_sys::DragEvent| {
        event.prevent_default();
        if let Some(data) = event.data_transfer() {
            data.set_drop_effect("copy");
        }
    });
    let dropped = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|event: web_sys::DragEvent| {
        event.prevent_default();
        let Some(file) = event.data_transfer().and_then(|d| d.files()).and_then(|files| files.get(0)) else { return };
        let Some(tx) = DROP_WAITER.with(|waiter| waiter.borrow_mut().take()) else { return };
        wasm_bindgen_futures::spawn_local(async move {
            let bytes = JsFuture::from(file.array_buffer()).await.ok().map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec());
            let _ = tx.send(bytes.map(|bytes| OpenedFile { name: file.name(), bytes }));
        });
    });
    for (event, listener) in [("dragover", over), ("drop", dropped)] {
        let _ = window.add_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
        // The page's whole life.
        listener.forget();
    }
}

/// Copies `text` to the clipboard. Browsers only offer the clipboard on HTTPS (and localhost),
/// and may still refuse; either way the result says so.
fn copy(text: &str) -> Copied {
    let Some(navigator) = web_sys::window().map(|w| w.navigator()) else { return Box::pin(async { false }) };
    let available = js_sys::Reflect::get(navigator.as_ref(), &"clipboard".into()).is_ok_and(|c| !c.is_undefined() && !c.is_null());
    if !available {
        return Box::pin(async { false });
    }
    let written = JsFuture::from(navigator.clipboard().write_text(text));
    Box::pin(async move { written.await.is_ok() })
}

/// Downloads `bytes` as a file called `name`.
fn download(name: &str, bytes: &[u8]) {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    // A neutral type, so no browser adds an extension of its own to the name.
    options.set_type("application/octet-stream");
    let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
    let link = document.create_element("a").ok().and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok());
    if let (Some(link), Some(body)) = (link, document.body()) {
        link.set_href(&url);
        link.set_download(name);
        if body.append_child(&link).is_ok() {
            link.click();
            link.remove();
        }
    }
    // Revoked a moment later: revoking right away can cancel the download.
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
}
