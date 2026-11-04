//! The browser playground's window, run natively: a config held in memory, Save handing the text to
//! a fake browser download, Open reading picked bytes, and nothing that needs this machine.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px, size};
use settings_core::{AppDefinition, Document};
use settings_ui::playground::{OpenedFile, PickedFile, Playground};
use settings_ui::view::SettingsWindow;
use std::sync::Mutex;

/// What the fake browser was asked to download, as (name, text). Tests run one at a time per
/// thread, so each test takes the lock for its whole run.
static DOWNLOADS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static SERIAL: Mutex<()> = Mutex::new(());

fn no_file() -> PickedFile {
    Box::pin(async { None })
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
    DOWNLOADS.lock().unwrap().clear();
    let app: &'static AppDefinition = Box::leak(Box::new(app.for_browser()));
    let document = Document::memory(name, text);
    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_ui::view::bind_keys(cx);
        cx.set_global(Playground {
            file_name: "config.toml",
            locations: &[("macOS", "~/.config/x/config")],
            open_file: no_file,
            download: record,
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
