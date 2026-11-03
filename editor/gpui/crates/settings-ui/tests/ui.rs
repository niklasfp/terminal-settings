//! UI integration tests: the real settings window (with Rio's app definition) in a headless GPUI
//! window, driven by clicks and typing, checked against the config file on disk.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px, size};
use settings_ui::view::SettingsWindow;
use settings_core::AppDefinition;
use std::path::{Path, PathBuf};

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    view: Entity<SettingsWindow>,
    window: AnyWindowHandle,
}

fn open(cx: &mut TestAppContext, config: &str) -> Fixture {
    open_with(cx, rio_config::app(), "config.toml", config)
}

/// Ghostty's editor, minus its after-save reload (which would make a running Ghostty reload).
fn open_ghostty(cx: &mut TestAppContext, config: &str) -> Fixture {
    let mut app = ghostty_config::app();
    app.after_save = || None;
    open_with(cx, app, "config.ghostty", config)
}

fn open_with(cx: &mut TestAppContext, app: AppDefinition, file: &str, config: &str) -> Fixture {
    open_with_files(cx, app, file, config, &[])
}

/// Like `open_with`, with more files next to the config (e.g. "themes/Mine.toml").
fn open_with_files(cx: &mut TestAppContext, app: AppDefinition, file: &str, config: &str, files: &[(&str, &str)]) -> Fixture {
    open_sized(cx, app, file, config, files, 1000.)
}

/// Like `open_with_files`, in a window `height` px tall (1280 wide).
fn open_sized(cx: &mut TestAppContext, app: AppDefinition, file: &str, config: &str, files: &[(&str, &str)], height: f32) -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(file);
    std::fs::write(&path, config).expect("write config");
    for (name, text) in files {
        let file = dir.path().join(name);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("create dir");
        std::fs::write(file, text).expect("write file");
    }
    let app: &'static AppDefinition = Box::leak(Box::new(app));

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_ui::view::bind_keys(cx);
    });
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(Default::default(), size(px(1280.), px(height))))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| SettingsWindow::new(app, path.clone(), window, cx)),
        )
        .expect("open window")
    });
    Fixture { _dir: dir, path, view, window }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read config")
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

    /// Filters the window down to matching settings, so rows below the fold become clickable.
    fn search(&self, cx: &mut TestAppContext, query: &str) {
        self.ui(cx, |window, cx| {
            window.press("/", cx);
            window.input(query, cx);
        });
    }

    fn save(&self, cx: &mut TestAppContext) {
        self.ui(cx, |window, cx| window.click("save", cx));
        self.ui(cx, |window, cx| window.click("confirm-save", cx));
    }
}

#[gpui_kit::test]
fn dragging_a_slider_records_the_value(cx: &mut TestAppContext) {
    let f = open(cx, "");
    f.search(cx, "window.opacity");
    f.ui(cx, |window, cx| {
        let height = window.find("window.opacity:slider").bounds().size.height;
        window.click_at("window.opacity:slider", gpui_kit::point(px(0.5), height / 2.), cx);
    });
    assert_eq!(f.dirty(cx), 1, "the slider change is pending");
    f.ui(cx, |window, _| assert_eq!(window.find("window.opacity:input").value(), Some("0")));

    f.save(cx);
    assert_eq!(read(&f.path), "[window]\nopacity = 0.0\n");
}

#[gpui_kit::test]
fn picking_a_color_records_the_value(cx: &mut TestAppContext) {
    let f = open(cx, "");
    f.search(cx, "colors.background");
    f.ui(cx, |window, cx| window.click("colors.background:picker", cx));
    // The swatches aren't observable, so pick through the popover's own hex field.
    f.ui(cx, |window, cx| {
        let field = window
            .within("colors.background:picker")
            .find_all_by_role(gpui_kit::Role::TextInput)
            .into_iter()
            .next()
            .expect("the picker's hex field");
        let id = field.path().last().cloned().expect("field id");
        let mut picker = window.within("colors.background:picker");
        picker.click(id, cx);
        picker.press("secondary-a", cx);
        picker.input("#112233", cx);
        picker.press("enter", cx);
    });
    assert_eq!(f.dirty(cx), 1, "the picked color is pending");
    f.ui(cx, |window, _| assert_eq!(window.find("colors.background:input").value(), Some("#112233")));

    f.save(cx);
    assert_eq!(read(&f.path), "[colors]\nbackground = \"#112233\"\n");
}

#[gpui_kit::test]
fn turning_a_font_style_off_and_on_again_keeps_it(cx: &mut TestAppContext) {
    let f = open(cx, "[fonts.regular]\nstyle = \"Bold\"\n");
    f.search(cx, "fonts.regular.style");
    f.ui(cx, |window, cx| window.click("fonts.regular.style:off", cx));
    assert_eq!(f.dirty(cx), 1);
    f.ui(cx, |window, cx| window.click("fonts.regular.style:off", cx));
    assert_eq!(f.dirty(cx), 0, "back to Bold, which is what the file has");
    f.ui(cx, |window, _| assert_eq!(window.find("fonts.regular.style:input").value(), Some("Bold")));
}

#[gpui_kit::test]
fn editing_a_record_back_to_its_file_value_is_not_a_change(cx: &mut TestAppContext) {
    // `bytes` is a real Rio binding field the schema doesn't describe.
    let f = open(cx, "[bindings]\nkeys = [\n  { key = \"t\", with = \"super\", bytes = [27] },\n]\n");
    f.search(cx, "bindings.keys");
    f.ui(cx, |window, cx| {
        window.click("bindings.keys:0:key", cx);
        window.press("secondary-a", cx);
        window.input("u", cx);
    });
    assert_eq!(f.dirty(cx), 1);
    f.ui(cx, |window, cx| {
        window.press("secondary-a", cx);
        window.input("t", cx);
    });
    assert_eq!(f.dirty(cx), 0);
}

#[gpui_kit::test]
fn reset_removes_a_value_set_in_the_file(cx: &mut TestAppContext) {
    let f = open(cx, "copy-on-select = true # mine\n# tail\n");
    f.ui(cx, |window, cx| window.click("copy-on-select:reset", cx));
    assert_eq!(f.dirty(cx), 1);
    f.save(cx);
    assert_eq!(read(&f.path), "# tail\n");
}

#[gpui_kit::test]
fn a_comma_ends_a_list_entry(cx: &mut TestAppContext) {
    let f = open(cx, "");
    f.search(cx, "shell.args");
    f.ui(cx, |window, cx| {
        window.click("shell.args:input", cx);
        window.input("-l,--norc,", cx);
    });
    f.save(cx);
    // Added to the default ["--login"].
    assert_eq!(read(&f.path), "[shell]\nargs = [\"--login\", \"-l\", \"--norc\"]\n");
}

#[gpui_kit::test]
fn toggling_a_switch_and_saving_changes_only_that_line(cx: &mut TestAppContext) {
    let f = open(cx, "# my settings\nconfirm-before-quit = true # keep\n\n[cursor]\nshape = \"block\"\n");
    assert_eq!(f.dirty(cx), 0);

    f.ui(cx, |window, cx| window.click("confirm-before-quit:switch", cx));
    assert_eq!(f.dirty(cx), 1);

    f.ui(cx, |window, cx| window.click("save", cx));
    f.ui(cx, |window, cx| window.click("confirm-save", cx));

    assert_eq!(read(&f.path), "# my settings\nconfirm-before-quit = false # keep\n\n[cursor]\nshape = \"block\"\n");
    assert_eq!(f.dirty(cx), 0);
    assert!(f.path.with_extension("toml.bak").exists(), "a backup is kept");
}

#[gpui_kit::test]
fn menu_commands_reach_the_window(cx: &mut TestAppContext) {
    use settings_ui::view::{AppearanceDark, AppearanceLight, CloseWindow};
    let f = open(cx, "");
    let dispatch = |cx: &mut TestAppContext, action: Box<dyn gpui_kit::Action>| {
        cx.update_window(f.window, |_, window, cx| window.dispatch_action(action, cx)).expect("window");
        cx.run_until_parked();
    };

    dispatch(cx, Box::new(AppearanceDark));
    assert!(cx.update(|cx| gpui_kit::component::Theme::global(cx).is_dark()));
    dispatch(cx, Box::new(AppearanceLight));
    assert!(!cx.update(|cx| gpui_kit::component::Theme::global(cx).is_dark()));

    // With an unsaved change, Close Window asks (a prompt) instead of closing.
    f.ui(cx, |window, cx| window.click("copy-on-select:switch", cx));
    dispatch(cx, Box::new(CloseWindow));
    assert_eq!(cx.update(|cx| cx.windows().len()), 1, "the window stays open");
    assert_eq!(f.dirty(cx), 1, "and keeps the change");
}

#[gpui_kit::test]
fn the_save_button_keeps_its_size_when_changes_appear(cx: &mut TestAppContext) {
    let f = open(cx, "");
    let mut before = None;
    f.ui(cx, |window, _| before = Some(window.find("save").bounds()));
    f.ui(cx, |window, cx| window.click("copy-on-select:switch", cx));
    assert_eq!(f.dirty(cx), 1);
    f.ui(cx, |window, _| assert_eq!(Some(window.find("save").bounds()), before, "the header row must not shift"));
}

#[gpui_kit::test]
fn typing_a_number_marks_it_dirty_and_reset_undoes_it(cx: &mut TestAppContext) {
    let f = open(cx, "");
    f.ui(cx, |window, cx| {
        window.click("scrollback-history-limit:input", cx);
        window.press("secondary-a", cx);
        window.input("5000", cx);
    });
    assert_eq!(f.dirty(cx), 1);

    f.ui(cx, |window, cx| window.click("scrollback-history-limit:reset", cx));
    assert_eq!(f.dirty(cx), 0);
    f.ui(cx, |window, _| assert_eq!(window.find("scrollback-history-limit:input").value(), Some("10000")));
}

#[gpui_kit::test]
fn an_out_of_range_value_shows_an_error_instead_of_saving(cx: &mut TestAppContext) {
    let f = open(cx, "");
    // Line height sits below the fold; searching brings it up (and hides the rest).
    f.ui(cx, |window, cx| {
        window.press("/", cx);
        window.input("line height", cx);
    });
    f.ui(cx, |window, _| assert!(window.try_find("confirm-before-quit:switch").is_none(), "search hides other rows"));
    f.ui(cx, |window, cx| {
        window.click("line-height:input", cx);
        window.press("secondary-a", cx);
        window.input("0.5", cx);
    });
    f.ui(cx, |window, cx| window.click("save", cx));

    let error = cx.update(|cx| f.view.read(cx).error("line-height").map(str::to_string));
    assert_eq!(error.as_deref(), Some("Must be at least 1."));
    assert_eq!(read(&f.path), "", "nothing is written");
    f.ui(cx, |window, _| assert!(window.try_find("confirm-save").is_none(), "no save dialog"));
}

#[gpui_kit::test]
fn slash_focuses_search_but_types_normally_inside_a_field(cx: &mut TestAppContext) {
    let f = open(cx, "");
    f.ui(cx, |window, cx| {
        window.press("/", cx);
        window.input("a/b", cx);
    });
    f.ui(cx, |window, _| assert_eq!(window.find("search").value(), Some("a/b")));
}

#[gpui_kit::test]
fn a_file_changed_on_disk_is_not_overwritten(cx: &mut TestAppContext) {
    let f = open(cx, "copy-on-select = false\n");
    f.ui(cx, |window, cx| window.click("copy-on-select:switch", cx));
    std::fs::write(&f.path, "copy-on-select = false\n# edited elsewhere\n").expect("external edit");

    f.ui(cx, |window, cx| window.click("save", cx));
    f.ui(cx, |window, _| assert!(window.try_find("reload").is_some(), "conflict banner offers a reload"));
    assert_eq!(read(&f.path), "copy-on-select = false\n# edited elsewhere\n");

    f.ui(cx, |window, cx| window.click("reload", cx));
    assert_eq!(f.dirty(cx), 0);
}

// ---------- Ghostty ----------

fn ghostty_installed() -> bool {
    let found = ghostty_config::ghostty_binary().is_some();
    if !found {
        eprintln!("skipped: Ghostty isn't installed");
    }
    found
}

#[gpui_kit::test]
fn ghostty_saves_a_line_and_keeps_the_rest(cx: &mut TestAppContext) {
    if !ghostty_installed() {
        return;
    }
    let f = open_ghostty(cx, "# mine\nfont-size = 13\ntheme = Dracula\n");
    f.search(cx, "font-size");
    f.ui(cx, |window, cx| {
        window.click("font-size:input", cx);
        window.press("secondary-a", cx);
        window.input("15", cx);
    });
    assert_eq!(f.dirty(cx), 1);
    f.save(cx);
    assert_eq!(read(&f.path), "# mine\nfont-size = 15\ntheme = Dracula\n");
}

#[gpui_kit::test]
fn ghostty_rejections_show_on_the_setting(cx: &mut TestAppContext) {
    if !ghostty_installed() {
        return;
    }
    let f = open_ghostty(cx, "font-size = 13\n");
    f.search(cx, "window-padding-x");
    f.ui(cx, |window, cx| {
        window.click("window-padding-x:input", cx);
        window.press("secondary-a", cx);
        window.input("nope", cx);
    });
    f.ui(cx, |window, cx| window.click("save", cx));
    let error = cx.update(|cx| f.view.read(cx).error("window-padding-x").map(str::to_string));
    assert!(error.as_deref().is_some_and(|e| e.starts_with("Ghostty:")), "{error:?}");
    assert_eq!(read(&f.path), "font-size = 13\n", "nothing written");
}

#[gpui_kit::test]
fn a_list_entry_still_being_typed_is_saved(cx: &mut TestAppContext) {
    let f = open(cx, "");
    f.search(cx, "shell.args");
    f.ui(cx, |window, cx| {
        window.click("shell.args:input", cx);
        window.input("-l", cx); // no Enter
        window.press("secondary-s", cx);
    });
    f.ui(cx, |window, cx| window.click("confirm-save", cx));
    assert_eq!(read(&f.path), "[shell]\nargs = [\"--login\", \"-l\"]\n");
    assert_eq!(f.dirty(cx), 0, "nothing left pending");
}

#[gpui_kit::test]
fn ghostty_list_entries_keep_their_commas(cx: &mut TestAppContext) {
    if !ghostty_installed() {
        return;
    }
    let f = open_ghostty(cx, "");
    f.search(cx, "macos-icon-screen-color");
    f.ui(cx, |window, cx| {
        window.click("macos-icon-screen-color:input", cx);
        window.input("#111111,#222222", cx);
        window.press("enter", cx);
    });
    f.save(cx);
    assert_eq!(read(&f.path), "macos-icon-screen-color = #111111,#222222\n");
}

#[gpui_kit::test]
fn on_a_short_window_the_sections_scroll_above_the_footer(cx: &mut TestAppContext) {
    // 500 px is shorter than the full section list (Rio has 17 sections).
    let f = open_sized(cx, rio_config::app(), "config.toml", "", &[], 500.);
    f.ui(cx, |window, _| {
        let list = window.find("groups-scroll").bounds();
        let footer = window.find("groups-footer").bounds();
        assert!(list.bottom() <= footer.top(), "the section list ends above the footer: {list:?} {footer:?}");
        assert!(footer.bottom() <= px(500.), "the footer stays in the window: {footer:?}");
    });
}

#[gpui_kit::test]
fn a_dropdown_leaves_room_for_its_label(cx: &mut TestAppContext) {
    // A plain dropdown and a font dropdown.
    for key in ["background-image-position", "window-title-font-family"] {
        let f = open_ghostty(cx, "");
        f.search(cx, key);
        f.ui(cx, |window, _| {
            let row = window.find(format!("row:{key}")).bounds();
            let label = window.find(format!("{key}:label")).bounds();
            assert!(label.size.width > px(300.), "{key}: the label keeps its room in the row: {row:?} {label:?}");
        });
    }
}

#[gpui_kit::test]
fn many_flags_are_a_multi_select_dropdown(cx: &mut TestAppContext) {
    let f = open_ghostty(cx, "");
    f.search(cx, "shell-integration-features");
    f.ui(cx, |window, cx| window.click("shell-integration-features:flags", cx));
    // Untick the first flag, "cursor" (on by default).
    f.ui(cx, |window, cx| window.press("down", cx));
    f.ui(cx, |window, cx| window.press("enter", cx));
    f.ui(cx, |window, cx| window.press("escape", cx));
    assert_eq!(f.dirty(cx), 1, "the flag change is pending");
    if !ghostty_installed() {
        return;
    }
    f.save(cx);
    let saved = read(&f.path);
    assert!(saved.starts_with("shell-integration-features = no-cursor,"), "{saved}");
}

#[gpui_kit::test]
fn settings_from_the_file_are_labelled_but_unsaved_edits_are_not(cx: &mut TestAppContext) {
    let f = open(cx, "[fonts]\nsize = 15.0\n");
    f.search(cx, "fonts.");
    f.ui(cx, |window, _| {
        assert!(window.try_find("fonts.size:set").is_some(), "a value from the file is labelled");
        assert!(window.try_find("fonts.hinting:set").is_none(), "a default isn't");
    });
    f.ui(cx, |window, cx| window.click("fonts.hinting:switch", cx));
    assert_eq!(f.dirty(cx), 1);
    f.ui(cx, |window, _| {
        assert!(window.try_find("fonts.hinting:set").is_none(), "an unsaved edit isn't in the file yet");
    });
}

#[gpui_kit::test]
fn a_path_field_says_whether_its_path_exists(cx: &mut TestAppContext) {
    // TOML literal strings ('…') take Windows paths without escaping the backslashes.
    let existing = std::env::temp_dir();
    let real = open(cx, &format!("working-dir = '{}'\n", existing.display()));
    real.search(cx, "working-dir");
    real.ui(cx, |window, _| {
        assert!(window.try_find("working-dir:browse").is_some(), "choose sits in the field");
        assert!(window.try_find("working-dir:reveal").is_some(), "an existing folder can be shown in the file manager");
        assert!(window.try_find("working-dir:missing").is_none());
    });

    let missing = existing.join("no-such-folder-for-this-test");
    let gone = open(cx, &format!("working-dir = '{}'\n", missing.display()));
    gone.search(cx, "working-dir");
    gone.ui(cx, |window, _| {
        assert!(window.try_find("working-dir:missing").is_some(), "a missing folder is flagged");
        assert!(window.try_find("working-dir:reveal").is_none());
    });

    // A keyword isn't a path, so it's neither flagged nor revealed.
    let keyword = open_ghostty(cx, "working-directory = home\n");
    keyword.search(cx, "working-directory");
    keyword.ui(cx, |window, _| {
        assert!(window.try_find("working-directory:browse").is_some());
        assert!(window.try_find("working-directory:missing").is_none());
        assert!(window.try_find("working-directory:reveal").is_none());
    });
}

#[gpui_kit::test]
fn a_theme_is_picked_from_a_searchable_dropdown(cx: &mut TestAppContext) {
    let theme = "[colors]\nbackground = \"#101010\"\nforeground = \"#f0f0f0\"\n";
    let f = open_with_files(cx, rio_config::app(), "config.toml", "", &[("themes/Mine.toml", theme)]);
    cx.run_until_parked(); // the theme list loads in the background
    f.search(cx, "theme");
    f.ui(cx, |window, cx| window.click("theme:theme", cx));
    f.ui(cx, |window, cx| window.input("Mine", cx));
    f.ui(cx, |window, cx| window.press("enter", cx));
    assert_eq!(f.dirty(cx), 1, "the picked theme is pending");
    f.save(cx);
    assert_eq!(read(&f.path), "theme = \"Mine\"\n");
}

#[gpui_kit::test]
fn a_font_column_keeps_its_font_when_another_column_changes(cx: &mut TestAppContext) {
    let f = open(cx, "[fonts]\nsymbol-map = [{ start = \"E0A0\", end = \"E0D4\", font-family = \"Alpha Mono\" }]\n");
    f.search(cx, "fonts.symbol-map");
    f.ui(cx, |window, _| {
        window.find("fonts.symbol-map:0:font-family");
    });
    f.ui(cx, |window, cx| {
        window.click("fonts.symbol-map:0:end", cx);
        window.press("secondary-a", cx);
        window.input("E0D5", cx);
    });
    f.save(cx);
    let saved = read(&f.path);
    assert!(saved.contains("end = \"E0D5\"") && saved.contains("font-family = \"Alpha Mono\""), "{saved}");
}

#[gpui_kit::test]
fn a_font_is_picked_from_a_searchable_dropdown(cx: &mut TestAppContext) {
    let f = open(cx, "[fonts.regular]\nfamily = \"Alpha Mono\"\n");
    f.search(cx, "fonts.regular.family");
    f.ui(cx, |window, cx| window.click("fonts.regular.family:fonts", cx));
    // The built-in default stays listed even though it isn't an installed font.
    f.ui(cx, |window, cx| window.input("cascadia", cx));
    f.ui(cx, |window, cx| window.press("enter", cx));
    assert_eq!(f.dirty(cx), 1, "the picked font is pending");
    f.save(cx);
    assert_eq!(read(&f.path), "[fonts.regular]\nfamily = \"cascadiacode\"\n");
}

#[gpui_kit::test]
fn a_font_list_drops_a_font_unticked_in_its_dropdown(cx: &mut TestAppContext) {
    if !ghostty_installed() {
        return;
    }
    let f = open_ghostty(cx, "font-family = Alpha Mono\nfont-family = Beta Mono\nfont-size = 13\n");
    f.search(cx, "font-family");
    f.ui(cx, |window, cx| window.click("font-family:fonts", cx));
    f.ui(cx, |window, cx| window.input("Beta", cx));
    f.ui(cx, |window, cx| window.press("down", cx));
    f.ui(cx, |window, cx| window.press("enter", cx));
    f.ui(cx, |window, cx| window.press("escape", cx));
    assert_eq!(f.dirty(cx), 1, "the shorter list is pending");
    f.save(cx);
    assert_eq!(read(&f.path), "font-family = Alpha Mono\nfont-size = 13\n");
}
