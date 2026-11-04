//! settings-web: the browser playground. One WebAssembly module runs either editor, picked by the
//! page's `?app=rio|ghostty`, over a config held in memory. Opening reads a file the user picks;
//! saving downloads it. Everything here is the browser's side (`settings_ui::Playground`).

#![cfg(target_family = "wasm")]

use settings_ui::playground::{OpenedFile, PickedFile, Playground, ThemeDownload};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// The fonts the editor draws with, fetched at startup rather than built in or deployed: IBM Plex
/// (SIL OFL), pinned to a release. IBM Plex Sans is also the family the web platform draws GPUI's
/// default UI font with. TTF, since GPUI reads neither WOFF nor WOFF2.
const FONT_URLS: [&str; 3] = [
    "https://cdn.jsdelivr.net/gh/IBM/plex@v6.4.0/IBM-Plex-Sans/fonts/complete/ttf/IBMPlexSans-Regular.ttf",
    "https://cdn.jsdelivr.net/gh/IBM/plex@v6.4.0/IBM-Plex-Sans/fonts/complete/ttf/IBMPlexSans-SemiBold.ttf",
    "https://cdn.jsdelivr.net/gh/IBM/plex@v6.4.0/IBM-Plex-Mono/fonts/complete/ttf/IBMPlexMono-Regular.ttf",
];

#[wasm_bindgen(start)]
pub async fn start() {
    // Panics go to the browser console, and so does GPUI's log.
    gpui_kit::platform::web_init();
    // GPUI can't draw a single letter without a font, so the editor waits for them.
    let fetches: Vec<_> = FONT_URLS.iter().map(|url| fetch_bytes(url)).collect();
    let files = match futures_util::future::join_all(fetches).await.into_iter().collect::<Result<Vec<_>, _>>() {
        Ok(files) => files,
        Err(url) => {
            show_status(&format!("The editor couldn't load its fonts from {url}. Check your connection or content blocker, then reload."));
            return;
        }
    };
    hide_status();
    let fonts = settings_ui::WebFonts { files, sans: "IBM Plex Sans", mono: "IBM Plex Mono" };
    if query_app().as_deref() == Some("ghostty") {
        settings_ui::run_web(
            ghostty_config::browser_app(),
            settings_ui::Brand {
                theme_json: include_str!("../../ghostty-settings/ghostty-theme.json"),
                light_theme: "Ghostty Light",
                dark_theme: "Ghostty Dark",
                logo: include_bytes!("../../../../brand/ghostty-logo.png"),
            },
            playground(ghostty_config::CONFIG_FILE_NAME, ghostty_config::CONFIG_LOCATIONS, None),
            fonts,
        );
    } else {
        settings_ui::run_web(
            rio_config::browser_app(),
            settings_ui::Brand {
                theme_json: include_str!("../../rio-settings/rio-theme.json"),
                light_theme: "Rio Light",
                dark_theme: "Rio Dark",
                logo: include_bytes!("../../../../brand/rio-logo.svg"),
            },
            playground(rio_config::CONFIG_FILE_NAME, rio_config::CONFIG_LOCATIONS, Some(download_rio_theme)),
            fonts,
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

/// Takes the page's "Loading…" line away once the editor draws over the page.
fn hide_status() {
    if let Some(status) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("status")) {
        status.remove();
    }
}

/// Replaces the page's "Loading…" line.
fn show_status(text: &str) {
    if let Some(status) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("status")) {
        status.set_text_content(Some(text));
    }
}

fn playground(
    file_name: &'static str,
    locations: &'static [(&'static str, &'static str)],
    install_theme: Option<fn(&str) -> ThemeDownload>,
) -> Playground {
    Playground { file_name, locations, open_file, download, install_theme }
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
        let input = &picker.input;
        if !picked {
            return None;
        }
        let file = input.files()?.get(0)?;
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
