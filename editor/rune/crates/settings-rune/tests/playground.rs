//! The browser playground's window, run natively on Rune: a config held in memory, Save handing
//! the text to a fake browser download or clipboard, Open reading picked or dropped bytes, and
//! nothing that needs the host machine.

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use settings_core::{AppDefinition, Document};
use settings_rune::playground::{Copied, OpenedFile, PickedFile, Playground, ThemeDownload};
use settings_rune::view::SettingsWindow;
use settings_rune::{Brand, Palette};
use std::sync::Mutex;

const TEST_BRAND: Brand = Brand {
    light: Palette {
        bg: "#f4f4f5",
        fg: "#18181b",
        accent: "#c026d3",
        sidebar: "#e4e4e7",
    },
    dark: Palette {
        bg: "#1b1b1d",
        fg: "#e4e4e7",
        accent: "#e879f9",
        sidebar: "#18181b",
    },
    logo: &[],
};

static DOWNLOADS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static DROPS: Mutex<Vec<OpenedFile>> = Mutex::new(Vec::new());
static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);
static REFUSE_COPY: Mutex<bool> = Mutex::new(false);
static SERIAL: Mutex<()> = Mutex::new(());

fn no_file() -> PickedFile {
    Box::pin(async { None })
}

fn next_drop() -> PickedFile {
    match DROPS.lock().unwrap().pop() {
        Some(file) => Box::pin(async { Some(file) }),
        None => Box::pin(std::future::pending()),
    }
}

fn copy(text: &str) -> Copied {
    let allowed = !*REFUSE_COPY.lock().unwrap();
    if allowed {
        *CLIPBOARD.lock().unwrap() = Some(text.to_string());
    }
    Box::pin(async move { allowed })
}

fn record(name: &str, bytes: &[u8]) {
    DOWNLOADS
        .lock()
        .unwrap()
        .push((name.to_string(), String::from_utf8(bytes.to_vec()).unwrap()));
}

fn downloads() -> Vec<(String, String)> {
    DOWNLOADS.lock().unwrap().clone()
}

fn open_browser_app(
    app: AppDefinition,
    name: &str,
    text: &str,
    install_theme: Option<fn(&str) -> ThemeDownload>,
) -> Runtime<SettingsWindow> {
    DOWNLOADS.lock().unwrap().clear();
    *CLIPBOARD.lock().unwrap() = None;
    *REFUSE_COPY.lock().unwrap() = false;
    let app: &'static AppDefinition = Box::leak(Box::new(app));
    let doc = Document::memory(name, text);
    let playground = Playground {
        file_name: "config.toml",
        locations: &[("macOS", "~/.config/x/config")],
        open_file: no_file,
        next_drop: Some(next_drop),
        download: record,
        copy,
        install_theme,
    };
    let state = SettingsWindow::new(app, TEST_BRAND, doc, None).with_playground(playground);
    let mut rt = Runtime::new(
        state,
        SettingsWindow::view,
        (1280.0, 1000.0),
        Env { system_dark: true, scale: 2.0 },
    );
    rt.frame(0.0);
    rt
}

fn open(app: AppDefinition, name: &str, text: &str) -> Runtime<SettingsWindow> {
    open_browser_app(app.for_browser(), name, text, None)
}

fn click(rt: &mut Runtime<SettingsWindow>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("element '{key}' not found"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

fn settle_dialog(rt: &mut Runtime<SettingsWindow>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn download(rt: &mut Runtime<SettingsWindow>) {
    click(rt, "save");
    settle_dialog(rt);
    click(rt, "confirm-save");
}

fn search(rt: &mut Runtime<SettingsWindow>, query: &str) {
    rt.update(|s, _| s.search.replace_text(query));
    settle_dialog(rt);
}

#[test]
fn saving_downloads_the_changed_text_under_the_file_name() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(
        rio_config::app(),
        "mine.toml",
        "# my settings\r\nconfirm-before-quit = true # keep\r\n",
    );
    assert!(rt.find("playground").is_some(), "playground banner is rendered");
    click(&mut rt, "confirm-before-quit:switch");
    assert_eq!(rt.state.editor.dirty_count(), 1);
    download(&mut rt);
    assert_eq!(
        downloads(),
        [(
            "mine.toml".to_string(),
            "# my settings\r\nconfirm-before-quit = false # keep\r\n".to_string()
        )],
        "the original name and line endings, only the edited line changed"
    );
    assert_eq!(rt.state.editor.dirty_count(), 0, "the download counts as saved");

    // The next save starts from the downloaded text.
    click(&mut rt, "confirm-before-quit:switch");
    download(&mut rt);
    assert_eq!(
        downloads()[1].1,
        "# my settings\r\nconfirm-before-quit = true # keep\r\n"
    );
}

#[test]
fn without_a_font_list_a_font_is_typed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(rio_config::app(), "config.toml", "");
    search(&mut rt, "fonts.regular.family");
    assert!(
        rt.find("fonts.regular.family:fonts").is_none(),
        "no font dropdown"
    );
    click(&mut rt, "fonts.regular.family:input");
    rt.key(KeyEvent {
        key: Key::Command('a'),
        mods: Mods::default(),
    });
    rt.paste("Fira Code");
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.editor.dirty_count(), 1);
    download(&mut rt);
    assert_eq!(
        downloads(),
        [(
            "config.toml".to_string(),
            "[fonts.regular]\nfamily = \"Fira Code\"\n".to_string()
        )]
    );
}

#[test]
fn path_fields_offer_nothing_that_needs_this_machine() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(
        rio_config::app(),
        "config.toml",
        "[shell]\nprogram = \"/bin/zsh\"\n",
    );
    search(&mut rt, "shell.program");
    assert!(rt.find("shell.program:input").is_some());
    for part in ["browse", "reveal", "missing"] {
        assert!(
            rt.find(&format!("shell.program:{part}")).is_none(),
            "no {part} button"
        );
    }
}

#[test]
fn opening_a_file_shows_its_values_and_refuses_binary_files() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(rio_config::app(), "config.toml", "");
    assert!(rt.find("open").is_some(), "an Open button");
    rt.update(|s, _| {
        s.open_bytes(OpenedFile {
            name: "picked.toml".into(),
            bytes: b"confirm-before-quit = false\n".to_vec(),
        });
        s.open_bytes(OpenedFile {
            name: "image.png".into(),
            bytes: vec![0xff, 0xfe, 0x00],
        });
    });
    rt.frame(1.0 / 60.0);
    click(&mut rt, "confirm-before-quit:switch");
    download(&mut rt);
    assert_eq!(
        downloads(),
        [("picked.toml".to_string(), "confirm-before-quit = true\n".to_string())],
        "the opened file is edited; the binary one was refused"
    );
}

#[test]
fn opening_a_file_drops_half_typed_entries_and_a_stale_save_dialog() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(rio_config::app(), "config.toml", "");

    // A list entry typed for the old file doesn't come along.
    search(&mut rt, "shell.args");
    rt.update(|s, _| {
        s.edits
            .entry("shell.args:draft".into())
            .or_default()
            .replace_text("--half-typed");
    });
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| {
        s.open_bytes(OpenedFile {
            name: "first.toml".into(),
            bytes: b"# first\n".to_vec(),
        });
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state
            .edits
            .get("shell.args:draft")
            .map(|e| e.text.as_str())
            .unwrap_or(""),
        ""
    );

    // The old file's save dialog closes, so it can't save its edits into the new file.
    search(&mut rt, "confirm-before-quit");
    click(&mut rt, "confirm-before-quit:switch");
    click(&mut rt, "save");
    settle_dialog(&mut rt);
    assert!(rt.find("confirm-save").is_some(), "the dialog is open");
    rt.update(|s, _| {
        s.open_bytes(OpenedFile {
            name: "second.toml".into(),
            bytes: b"# second\n".to_vec(),
        });
    });
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("confirm-save").is_none(),
        "the old file's dialog is gone"
    );
    assert_eq!(rt.state.editor.dirty_count(), 0);

    click(&mut rt, "confirm-before-quit:switch");
    download(&mut rt);
    assert_eq!(
        downloads(),
        [(
            "second.toml".to_string(),
            "# second\nconfirm-before-quit = false\n".to_string()
        )]
    );
}

fn fake_theme_download(name: &str) -> ThemeDownload {
    let file = format!("{name}.toml");
    record(&file, b"[colors]\n");
    Box::pin(async move { Ok(file) })
}

fn pick_theme(rt: &mut Runtime<SettingsWindow>, name: &str) {
    search(rt, "theme");
    click(rt, "combo:theme-trigger");
    settle_dialog(rt);
    rt.paste(name);
    rt.frame(1.0 / 60.0);
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);
}

#[test]
fn a_rio_theme_is_picked_from_the_catalogue_and_its_file_downloaded() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open_browser_app(
        rio_config::browser_app(),
        "config.toml",
        "",
        Some(fake_theme_download),
    );
    pick_theme(&mut rt, "Dracula");
    assert_eq!(rt.state.editor.dirty_count(), 1, "the picked theme is pending");
    assert_eq!(
        downloads()[0].0,
        "Dracula.toml",
        "the theme file was handed to the browser"
    );
    download(&mut rt);
    assert_eq!(
        downloads()[1],
        ("config.toml".to_string(), "theme = \"Dracula\"\n".to_string())
    );
}

#[test]
fn a_ghostty_theme_is_picked_from_the_shipped_ones_without_a_download() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open_browser_app(
        ghostty_config::browser_app(),
        "config.ghostty",
        "font-size = 13\n",
        None,
    );
    pick_theme(&mut rt, "Dracula");
    assert_eq!(rt.state.editor.dirty_count(), 1);
    download(&mut rt);
    assert_eq!(
        downloads(),
        [(
            "config.ghostty".to_string(),
            "font-size = 13\n\ntheme = Dracula\n".to_string()
        )]
    );
}

#[test]
fn save_can_copy_the_file_instead_of_downloading_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(rio_config::app(), "mine.toml", "# mine\n");
    click(&mut rt, "confirm-before-quit:switch");
    click(&mut rt, "save");
    settle_dialog(&mut rt);
    click(&mut rt, "confirm-copy");
    assert_eq!(
        CLIPBOARD.lock().unwrap().as_deref(),
        Some("# mine\nconfirm-before-quit = false\n")
    );
    assert!(downloads().is_empty(), "nothing downloaded");
    assert_eq!(rt.state.editor.dirty_count(), 0, "the copy counts as saved");
}

#[test]
fn a_copy_the_browser_refuses_keeps_the_edits() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(rio_config::app(), "mine.toml", "# mine\n");
    *REFUSE_COPY.lock().unwrap() = true;
    click(&mut rt, "confirm-before-quit:switch");
    click(&mut rt, "save");
    settle_dialog(&mut rt);
    click(&mut rt, "confirm-copy");
    assert_eq!(rt.state.editor.dirty_count(), 1, "nothing was saved");
    // Downloading still works.
    download(&mut rt);
    assert_eq!(
        downloads(),
        [(
            "mine.toml".to_string(),
            "# mine\nconfirm-before-quit = false\n".to_string()
        )]
    );
}

#[test]
fn a_downloaded_file_keeps_its_byte_order_mark() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rt = open(rio_config::app(), "bom.toml", "\u{FEFF}# bom\n");
    click(&mut rt, "confirm-before-quit:switch");
    download(&mut rt);
    assert_eq!(
        downloads(),
        [(
            "bom.toml".to_string(),
            "\u{FEFF}# bom\nconfirm-before-quit = false\n".to_string()
        )]
    );
}

#[test]
fn a_file_dropped_on_the_page_is_opened() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    DROPS.lock().unwrap().push(OpenedFile {
        name: "dropped.toml".into(),
        bytes: b"# dropped\n".to_vec(),
    });
    let mut rt = open(rio_config::app(), "config.toml", "");
    click(&mut rt, "confirm-before-quit:switch");
    download(&mut rt);
    assert_eq!(
        downloads()[0],
        (
            "dropped.toml".to_string(),
            "# dropped\nconfirm-before-quit = false\n".to_string()
        )
    );

    // With unsaved edits, a drop asks first; Cancel keeps them.
    click(&mut rt, "confirm-before-quit:switch");
    rt.update(|s, _| {
        s.open_dropped(OpenedFile {
            name: "other.toml".into(),
            bytes: b"# other\n".to_vec(),
        });
    });
    rt.frame(1.0 / 60.0);
    settle_dialog(&mut rt);
    assert!(
        rt.find("discard-dialog-dialog").is_some() || rt.find("cancel-discard").is_some(),
        "asks before discarding"
    );
    click(&mut rt, "cancel-discard");
    assert_eq!(rt.state.editor.dirty_count(), 1, "the edit stays");

    rt.update(|s, _| {
        s.open_dropped(OpenedFile {
            name: "other.toml".into(),
            bytes: b"# other\n".to_vec(),
        });
    });
    rt.frame(1.0 / 60.0);
    settle_dialog(&mut rt);
    click(&mut rt, "confirm-discard");
    assert_eq!(rt.state.editor.dirty_count(), 0);
    click(&mut rt, "confirm-before-quit:switch");
    download(&mut rt);
    assert_eq!(
        downloads()[1],
        (
            "other.toml".to_string(),
            "# other\nconfirm-before-quit = false\n".to_string()
        )
    );
}
