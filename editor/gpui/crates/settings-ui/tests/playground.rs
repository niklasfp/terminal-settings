//! The browser playground's window, run natively: a config held in memory, Save handing the text to
//! a fake browser download, Open reading picked bytes, and nothing that needs this machine.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px, size};
use settings_core::{AppDefinition, Document};
use settings_ui::playground::{Copied, OpenedFile, PickedFile, Playground, ThemeDownload};
use settings_ui::view::SettingsWindow;
use std::sync::Mutex;

/// What the fake browser was asked to download, as (name, text). Tests run one at a time per
/// thread, so each test takes the lock for its whole run.
static DOWNLOADS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static SERIAL: Mutex<()> = Mutex::new(());

fn no_file() -> PickedFile {
    Box::pin(async { None })
}

/// Files the fake page has had dropped on it, handed to the window one at a time.
static DROPS: Mutex<Vec<OpenedFile>> = Mutex::new(Vec::new());

/// The next dropped file, or waiting forever once there are none.
fn next_drop() -> PickedFile {
    match DROPS.lock().unwrap().pop() {
        Some(file) => Box::pin(async { Some(file) }),
        None => Box::pin(std::future::pending()),
    }
}

/// What the fake clipboard holds; `None` while copying is refused.
static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);
static REFUSE_COPY: Mutex<bool> = Mutex::new(false);

fn copy(text: &str) -> Copied {
    let allowed = !*REFUSE_COPY.lock().unwrap();
    if allowed {
        *CLIPBOARD.lock().unwrap() = Some(text.to_string());
    }
    Box::pin(async move { allowed })
}

fn record(name: &str, bytes: &[u8]) {
    DOWNLOADS.lock().unwrap().push((name.to_string(), String::from_utf8(bytes.to_vec()).unwrap()));
}

struct Fixture {
    view: Entity<SettingsWindow>,
    window: AnyWindowHandle,
}

impl Fixture {
    fn ui(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App)) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            f(window, cx);
        })
        .expect("window");
        cx.run_until_parked();
    }

    fn dirty(&self, cx: &mut TestAppContext) -> usize {
        cx.update(|cx| self.view.read(cx).dirty_count())
    }

    fn search(&self, cx: &mut TestAppContext, query: &str) {
        self.ui(cx, |window, cx| {
            window.press("/", cx);
            window.input(query, cx);
        });
    }

    fn download(&self, cx: &mut TestAppContext) {
        self.ui(cx, |window, cx| window.click("save", cx));
        self.ui(cx, |window, cx| window.click("confirm-save", cx));
    }
}

fn open(cx: &mut TestAppContext, app: AppDefinition, name: &str, text: &str) -> Fixture {
    open_browser_app(cx, app.for_browser(), name, text, None)
}

/// Like `open`, with an app already made for the browser and the browser's theme download.
fn open_browser_app(cx: &mut TestAppContext, app: AppDefinition, name: &str, text: &str, install_theme: Option<fn(&str) -> ThemeDownload>) -> Fixture {
    DOWNLOADS.lock().unwrap().clear();
    *CLIPBOARD.lock().unwrap() = None;
    *REFUSE_COPY.lock().unwrap() = false;
    let app: &'static AppDefinition = Box::leak(Box::new(app));
    let document = Document::memory(name, text);
    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_ui::view::bind_keys(cx);
        cx.set_global(Playground {
            file_name: "config.toml",
            locations: &[("macOS", "~/.config/x/config")],
            open_file: no_file,
            next_drop: Some(next_drop),
            download: record,
            copy,
            install_theme,
        });
    });
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(Default::default(), size(px(1280.), px(1000.))))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| SettingsWindow::new(app, document, window, cx)),
        )
        .expect("open window")
    });
    Fixture { view, window }
}

fn drop_file(f: &Fixture, cx: &mut TestAppContext, name: &str, text: &str) {
    cx.update_window(f.window, |_, window, cx| {
        f.view.update(cx, |view, cx| view.open_dropped(OpenedFile { name: name.into(), bytes: text.as_bytes().to_vec() }, window, cx))
    })
    .expect("window");
    cx.run_until_parked();
}

fn downloads() -> Vec<(String, String)> {
    DOWNLOADS.lock().unwrap().clone()
}

#[gpui_kit::test]
fn saving_downloads_the_changed_text_under_the_file_name(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "mine.toml", "# my settings\r\nconfirm-before-quit = true # keep\r\n");
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    assert_eq!(f.dirty(cx), 1);
    f.download(cx);
    assert_eq!(
        downloads(),
        [("mine.toml".to_string(), "# my settings\r\nconfirm-before-quit = false # keep\r\n".to_string())],
        "the original name and line endings, only the edited line changed"
    );
    assert_eq!(f.dirty(cx), 0, "the download counts as saved");

    // The next save starts from the downloaded text.
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.download(cx);
    assert_eq!(downloads()[1].1, "# my settings\r\nconfirm-before-quit = true # keep\r\n");
}

#[gpui_kit::test]
fn without_a_font_list_a_font_is_typed(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "config.toml", "");
    f.search(cx, "fonts.regular.family");
    f.ui(cx, |window, cx| {
        assert!(window.try_find("fonts.regular.family:fonts").is_none(), "no font dropdown");
        window.click("fonts.regular.family:input", cx);
        window.press("secondary-a", cx);
        window.input("Fira Code", cx);
    });
    assert_eq!(f.dirty(cx), 1);
    f.download(cx);
    assert_eq!(downloads(), [("config.toml".to_string(), "[fonts.regular]\nfamily = \"Fira Code\"\n".to_string())]);
}

#[gpui_kit::test]
fn path_fields_offer_nothing_that_needs_this_machine(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "config.toml", "[shell]\nprogram = \"/bin/zsh\"\n");
    f.search(cx, "shell.program");
    f.ui(cx, |window, _| {
        assert!(window.try_find("shell.program:input").is_some());
        for part in ["browse", "reveal", "missing"] {
            assert!(window.try_find(format!("shell.program:{part}")).is_none(), "no {part} button");
        }
    });
}

#[gpui_kit::test]
fn opening_a_file_shows_its_values_and_refuses_binary_files(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "config.toml", "");
    f.ui(cx, |window, _| assert!(window.try_find("open").is_some(), "an Open button"));
    cx.update_window(f.window, |_, window, cx| {
        f.view.update(cx, |view, cx| {
            view.open_bytes(OpenedFile { name: "picked.toml".into(), bytes: b"confirm-before-quit = false\n".to_vec() }, window, cx);
            view.open_bytes(OpenedFile { name: "image.png".into(), bytes: vec![0xff, 0xfe, 0x00] }, window, cx);
        })
    })
    .expect("window");
    cx.run_until_parked();
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.download(cx);
    assert_eq!(
        downloads(),
        [("picked.toml".to_string(), "confirm-before-quit = true\n".to_string())],
        "the opened file is edited; the binary one was refused"
    );
}


fn open_picked(f: &Fixture, cx: &mut TestAppContext, name: &str, text: &str) {
    cx.update_window(f.window, |_, window, cx| {
        f.view.update(cx, |view, cx| view.open_bytes(OpenedFile { name: name.into(), bytes: text.as_bytes().to_vec() }, window, cx))
    })
    .expect("window");
    cx.run_until_parked();
}

#[gpui_kit::test]
fn opening_a_file_drops_half_typed_entries_and_a_stale_save_dialog(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "config.toml", "");

    // A list entry typed for the old file doesn't come along.
    f.search(cx, "shell.args");
    f.ui(cx, |window, cx| {
        window.click("shell.args:input", cx);
        window.input("--half-typed", cx);
    });
    open_picked(&f, cx, "first.toml", "# first\n");
    f.ui(cx, |window, _| assert_eq!(window.find("shell.args:input").value(), Some("")));

    // The old file's save dialog closes, so it can't save its edits into the new file.
    f.ui(cx, |window, cx| {
        window.click("search", cx);
        window.press("secondary-a", cx);
        window.input("confirm-before-quit", cx);
    });
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.ui(cx, |window, cx| window.click("save", cx));
    f.ui(cx, |window, _| assert!(window.try_find("confirm-save").is_some(), "the dialog is open"));
    open_picked(&f, cx, "second.toml", "# second\n");
    f.ui(cx, |window, _| assert!(window.try_find("confirm-save").is_none(), "the old file's dialog is gone"));
    assert_eq!(f.dirty(cx), 0);

    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.download(cx);
    assert_eq!(downloads(), [("second.toml".to_string(), "# second\nconfirm-before-quit = false\n".to_string())]);
}

/// A fake theme download: records the theme as a downloaded `<name>.toml`.
fn fake_theme_download(name: &str) -> ThemeDownload {
    let file = format!("{name}.toml");
    record(&file, b"[colors]\n");
    Box::pin(async move { Ok(file) })
}

fn pick_theme(f: &Fixture, cx: &mut TestAppContext, name: &str) {
    cx.run_until_parked(); // the theme list loads in the background
    f.search(cx, "theme");
    f.ui(cx, |window, cx| window.click("theme:theme", cx));
    // Type into the popover's search field.
    f.ui(cx, |window, cx| {
        let search = window.within("theme:theme").find_all_by_role(gpui_kit::Role::TextInput).into_iter().last().expect("the search field");
        let id = search.path().last().cloned().expect("field id");
        window.within("theme:theme").click(id, cx);
    });
    // The field takes text input once it has painted focused.
    f.ui(cx, |window, cx| window.input(name, cx));
    cx.executor().advance_clock(std::time::Duration::from_millis(300));
    f.ui(cx, |_, _| {});
    f.ui(cx, |window, cx| window.press("enter", cx));
}

#[gpui_kit::test]
fn a_rio_theme_is_picked_from_the_catalogue_and_its_file_downloaded(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open_browser_app(cx, rio_config::browser_app(), "config.toml", "", Some(fake_theme_download));
    pick_theme(&f, cx, "Dracula");
    assert_eq!(f.dirty(cx), 1, "the picked theme is pending");
    assert_eq!(downloads()[0].0, "Dracula.toml", "the theme file was handed to the browser");
    f.download(cx);
    assert_eq!(downloads()[1], ("config.toml".to_string(), "theme = \"Dracula\"\n".to_string()));
}

#[gpui_kit::test]
fn a_ghostty_theme_is_picked_from_the_shipped_ones_without_a_download(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open_browser_app(cx, ghostty_config::browser_app(), "config.ghostty", "font-size = 13\n", None);
    pick_theme(&f, cx, "Dracula");
    assert_eq!(f.dirty(cx), 1);
    f.download(cx);
    assert_eq!(downloads(), [("config.ghostty".to_string(), "font-size = 13\n\ntheme = Dracula\n".to_string())]);
}

#[gpui_kit::test]
fn save_can_copy_the_file_instead_of_downloading_it(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "mine.toml", "# mine\n");
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.ui(cx, |window, cx| window.click("save", cx));
    f.ui(cx, |window, cx| window.click("confirm-copy", cx));
    assert_eq!(CLIPBOARD.lock().unwrap().as_deref(), Some("# mine\nconfirm-before-quit = false\n"));
    assert!(downloads().is_empty(), "nothing downloaded");
    assert_eq!(f.dirty(cx), 0, "the copy counts as saved");
}

#[gpui_kit::test]
fn a_copy_the_browser_refuses_keeps_the_edits(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "mine.toml", "# mine\n");
    *REFUSE_COPY.lock().unwrap() = true;
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.ui(cx, |window, cx| window.click("save", cx));
    f.ui(cx, |window, cx| window.click("confirm-copy", cx));
    assert_eq!(f.dirty(cx), 1, "nothing was saved");
    // Downloading still works.
    f.download(cx);
    assert_eq!(downloads(), [("mine.toml".to_string(), "# mine\nconfirm-before-quit = false\n".to_string())]);
}

#[gpui_kit::test]
fn a_downloaded_file_keeps_its_byte_order_mark(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = open(cx, rio_config::app(), "bom.toml", "\u{FEFF}# bom\n");
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.download(cx);
    assert_eq!(downloads(), [("bom.toml".to_string(), "\u{FEFF}# bom\nconfirm-before-quit = false\n".to_string())]);
}

#[gpui_kit::test]
fn a_file_dropped_on_the_page_is_opened(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    DROPS.lock().unwrap().push(OpenedFile { name: "dropped.toml".into(), bytes: b"# dropped\n".to_vec() });
    let f = open(cx, rio_config::app(), "config.toml", "");
    cx.run_until_parked();
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.download(cx);
    assert_eq!(downloads()[0], ("dropped.toml".to_string(), "# dropped\nconfirm-before-quit = false\n".to_string()));

    // With unsaved edits, a drop asks first; Cancel keeps them.
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    drop_file(&f, cx, "other.toml", "# other\n");
    assert!(cx.has_pending_prompt(), "asks before discarding");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(f.dirty(cx), 1, "the edit stays");
    drop_file(&f, cx, "other.toml", "# other\n");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(f.dirty(cx), 0);
    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    f.download(cx);
    assert_eq!(downloads()[1], ("other.toml".to_string(), "# other\nconfirm-before-quit = false\n".to_string()));
}

