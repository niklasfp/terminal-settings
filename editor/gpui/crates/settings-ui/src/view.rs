//! The settings window: header (file, scope, appearance, save), sidebar of groups, one page of
//! settings at a time (or search results across all groups), and the save-with-diff dialog.

use crate::assets::group_icon;
use crate::controls::Control;
use crate::dropdowns::{FontItem, ThemeItem};
use crate::menus::{self, Appearance};
use crate::model::{self, ANSI, Editor, Lock};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, Theme, ThemeMode,
    WindowExt as _, h_flex, v_flex,
    alert::Alert,
    button::{Button, ButtonGroup, ButtonVariants as _},
    group_box::{GroupBox, GroupBoxVariants as _},
    input::{Input, InputEvent, InputState},
    kbd::Kbd,
    notification::Notification,
    scroll::ScrollableElement as _,
    select::{Select, SelectEvent, SelectState},
    sidebar::{SidebarItem, SidebarMenu, SidebarMenuItem},
    tag::Tag,
};
use gpui_kit::*;
use gpui_kit::base::Selectable as _;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::Value;
use settings_core::{AppDefinition, DiffOp, Document, DocumentStore, FieldErrors, SaveError, Setting, SettingType, line_diff};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

actions!(
    settings_window,
    [
        Save,
        FocusSearch,
        Quit,
        About,
        CloseWindow,
        Minimize,
        Zoom,
        Hide,
        HideOthers,
        ShowAll,
        AppearanceSystem,
        AppearanceLight,
        AppearanceDark
    ]
);

const CONTEXT: &str = "SettingsWindow";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // ⌘ on macOS, Ctrl elsewhere (KeyBinding::new can't parse `secondary-`); Ctrl+S saves everywhere.
        // A browser may run on any of them, so the web build takes ⌘S too.
        KeyBinding::new(if cfg!(any(target_os = "macos", target_family = "wasm")) { "cmd-s" } else { "ctrl-s" }, Save, Some(CONTEXT)),
        KeyBinding::new("ctrl-s", Save, Some(CONTEXT)),
        KeyBinding::new("/", FocusSearch, Some("SettingsWindow && !Input")),
        KeyBinding::new(if cfg!(target_os = "macos") { "cmd-f" } else { "ctrl-f" }, FocusSearch, Some(CONTEXT)),
    ]);
    // A browser may be on a Mac, so the web build also takes ⌘F (it already has Ctrl+F).
    #[cfg(target_family = "wasm")]
    cx.bind_keys([KeyBinding::new("cmd-f", FocusSearch, Some(CONTEXT))]);
}

enum Banner {
    /// The file changed on disk while there were unsaved edits.
    Conflict,
    /// The file can't be parsed.
    ParseError(String),
}

const PLATFORMS: [(&str, &str); 4] = [("", "All platforms"), ("macos", "macOS"), ("linux", "Linux"), ("windows", "Windows")];

fn platform_label(p: &str) -> &str {
    PLATFORMS.iter().find(|(id, _)| *id == p).map_or(p, |(_, l)| l)
}

/// The platform this runs on, for highlighting its badges; "" in a browser, which could be any.
fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_family = "wasm") {
        ""
    } else {
        "linux"
    }
}

/// An alert with a command button beside it.
fn with_action(alert: Alert, action: Button) -> impl IntoElement {
    h_flex().gap_3().items_center().child(alert.flex_1()).child(action)
}

/// "~/…" for paths under the home folder (a Unix convention; Windows shows the full path).
fn tidy_path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        return s;
    }
    match std::env::home_dir() {
        Some(home) if !home.as_os_str().is_empty() => match p.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => s,
        },
        _ => s,
    }
}

pub struct SettingsWindow {
    pub(crate) app: &'static AppDefinition,
    /// The file being edited, or text held in memory.
    pub(crate) document: Document,
    pub(crate) editor: Editor,
    pub(crate) controls: HashMap<String, Control>,
    /// Every font and theme dropdown lists these (shared, so each list is built once).
    pub(crate) fonts: Rc<Vec<FontItem>>,
    pub(crate) themes: Rc<Vec<ThemeItem>>,
    pub(crate) lists: HashMap<String, Rc<Vec<SharedString>>>,
    pub(crate) suggest: Option<crate::controls::SuggestState>,
    pub(crate) next_row_id: u64,
    pub(crate) subs: Vec<Subscription>,
    errors: FieldErrors,
    search: Entity<InputState>,
    search_text: HashMap<String, String>,
    active_group: String,
    scope_select: Entity<SelectState<Vec<SharedString>>>,
    appearance: Appearance,
    banner: Option<Banner>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// A preview or save is running in the background; edits are locked.
    pub(crate) busy: bool,
    /// Settings whose longer docs ("More") are open.
    expanded: std::collections::HashSet<String>,
    /// The latest theme choice per setting; a download only applies if it's still the latest.
    pub(crate) theme_requests: HashMap<String, u64>,
    /// Watches a file for outside edits; none for text in memory.
    _poll: Option<Task<()>>,
}

impl SettingsWindow {
    pub fn new(app: &'static AppDefinition, document: impl Into<Document>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let document = document.into();
        let state = DocumentStore::new(app, document.clone()).state();
        let schema = &app.schema;
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let scope_items: Vec<SharedString> = PLATFORMS
            .iter()
            .filter(|(id, _)| id.is_empty() || schema.scopes.iter().any(|s| s == id))
            .map(|(_, label)| SharedString::from(*label))
            .collect();
        let scope_select = cx.new(|cx| SelectState::new(scope_items, Some(Default::default()), window, cx));

        // The fonts installed here. A config held in memory (the browser playground) is for another
        // machine, so it gets no list and font fields take typed names.
        let installed = if document.is_file() { cx.text_system().all_font_names() } else { Vec::new() };
        let mut fonts: Vec<SharedString> = installed.into_iter().filter(|f| !f.starts_with('.')).map(Into::into).collect();
        fonts.sort_by_key(|f| f.to_lowercase());
        fonts.dedup();

        let poll = document.is_file().then(|| {
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(3)).await;
                    if this.update_in(cx, |this, window, cx| this.poll(window, cx)).is_err() {
                        break;
                    }
                }
            })
        });

        let appearance = cx
            .try_global::<crate::AppearanceFile>()
            .map(|f| Appearance::load(&f.0))
            .unwrap_or(Appearance::System);

        let mut this = Self {
            app,
            document,
            banner: state.parse_error.clone().map(Banner::ParseError),
            editor: Editor::new(schema, state),
            controls: HashMap::new(),
            fonts: Rc::new(fonts.into_iter().map(FontItem).collect()),
            themes: Rc::new(Vec::new()),
            lists: HashMap::new(),
            suggest: None,
            next_row_id: 0,
            subs: Vec::new(),
            errors: FieldErrors::new(),
            search,
            search_text: schema.settings.iter().map(|s| (s.key.clone(), model::search_text(schema, s))).collect(),
            active_group: schema.groups.first().map(|g| g.id.clone()).unwrap_or_default(),
            scope_select,
            appearance,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            busy: false,
            expanded: Default::default(),
            theme_requests: HashMap::new(),
            _poll: poll,
        };

        this.apply_appearance(appearance, window, cx);
        for s in &schema.settings {
            let control = this.create_control(s, window, cx);
            this.controls.insert(s.key.clone(), control);
        }
        this.sync_all(window, cx);
        this.load_themes(window, cx);
        this.load_lists(window, cx);

        let search_sub = cx.subscribe_in(&this.search, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                this.scroll.set_offset(point(px(0.), px(0.)));
                cx.notify();
            }
        });
        let scope_sub = cx.subscribe_in(&this.scope_select, window, |this, _, ev: &SelectEvent<Vec<SharedString>>, window, cx| {
            let SelectEvent::Confirm(Some(label)) = ev else { return };
            let scope = PLATFORMS.iter().find(|(_, l)| *l == label.as_ref()).map_or("", |(id, _)| *id);
            this.editor.set_scope(scope);
            this.sync_all(window, cx);
            cx.notify();
        });
        let appearance_sub = cx.observe_window_appearance(window, |this, window, cx| {
            if this.appearance == Appearance::System {
                Theme::sync_system_appearance(Some(window), cx);
            }
        });
        this.subs.extend([search_sub, scope_sub, appearance_sub]);
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.confirm_discard(false, window, cx)).unwrap_or(true)
        });
        this.focus.focus(window, cx);
        this
    }

    /// Unsaved changes.
    pub fn dirty_count(&self) -> usize {
        self.editor.dirty_count()
    }

    /// The error shown under a setting after a failed save, if any.
    pub fn error(&self, key: &str) -> Option<&str> {
        self.errors.get(key).map(String::as_str)
    }

    // ---------- edits ----------

    /// Records an edit from a widget. The widget already shows the value.
    pub(crate) fn edit(&mut self, key: &str, value: Value, _: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        self.editor.edit(s, value);
        self.errors.remove(key);
        cx.notify();
    }

    /// Records an edit made outside the widget (e.g. a chosen file) and updates the widget.
    pub(crate) fn set_and_sync(&mut self, key: &str, value: Value, window: &mut Window, cx: &mut Context<Self>) {
        self.edit(key, value, window, cx);
        self.sync(key, window, cx);
    }

    fn reset(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        if s.ty == SettingType::Theme {
            // A theme still downloading must not come back after the reset.
            self.supersede_theme_request(key);
        }
        self.editor.reset(s);
        self.errors.remove(key);
        self.sync(key, window, cx);
        cx.notify();
    }

    fn sync_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.editor.schema.settings.iter().map(|s| s.key.clone()).collect();
        for key in keys {
            self.sync(&key, window, cx);
        }
    }

    fn store(&self) -> DocumentStore<'static> {
        DocumentStore::new(self.app, self.document.clone())
    }

    /// Re-reads the file and shows it, dropping unsaved edits.
    pub(crate) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.store().state();
        self.banner = state.parse_error.clone().map(Banner::ParseError);
        self.editor.state = state;
        self.editor.discard();
        // A theme download finishing later must not bring a discarded choice back.
        self.theme_requests.values_mut().for_each(|n| *n += 1);
        self.errors.clear();
        self.sync_all(window, cx);
        cx.notify();
    }

    /// Picks up changes made to the file outside the editor.
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            // Our own save is about to change the file.
            return;
        }
        let state = self.store().state();
        if let Some(err) = &state.parse_error {
            self.banner = Some(Banner::ParseError(err.clone()));
            cx.notify();
            return;
        }
        if matches!(self.banner, Some(Banner::ParseError(_))) {
            self.banner = None;
            cx.notify();
        }
        if state.version == self.editor.state.version {
            return;
        }
        if self.editor.dirty_count() > 0 {
            self.banner = Some(Banner::Conflict);
            cx.notify();
            return;
        }
        self.editor.state = state;
        self.sync_all(window, cx);
        window.push_notification(format!("Reloaded changes from {}", self.file_name()), cx);
        cx.notify();
    }

    pub(crate) fn file_name(&self) -> String {
        self.document.file_name().unwrap_or_else(|| "the config file".into())
    }

    /// Asks before unsaved changes are thrown away by closing or quitting. Returns true when there
    /// is nothing to lose; otherwise asks, and closes (or quits) itself if the user agrees.
    fn confirm_discard(&mut self, quit: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let n = self.editor.dirty_count();
        if n == 0 {
            return true;
        }
        let message = format!("Discard {n} unsaved change{}?", if n == 1 { "" } else { "s" });
        let detail = format!("They haven't been saved to {}.", self.file_name());
        let answer = window.prompt(PromptLevel::Warning, &message, Some(&detail), &["Discard", "Cancel"], cx);
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.editor.discard();
                if quit {
                    cx.quit();
                } else {
                    window.remove_window();
                }
            })
            .ok();
        })
        .detach();
        false
    }

    // ---------- save ----------

    fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.start_save(window, cx);
    }

    /// Runs a store operation on a background thread: validation and the after-save step may run
    /// the terminal (seconds, in the worst case). Edits are locked until it finishes.
    fn in_background<R: Send + 'static>(
        &mut self,
        work: impl FnOnce(&mut DocumentStore<'static>) -> R + Send + 'static,
        done: impl FnOnce(&mut Self, R, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        cx.notify();
        let mut store = self.store();
        let started = self.document.clone();
        let task = cx.background_executor().spawn(async move {
            let result = work(&mut store);
            (result, store.document)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (result, document) = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                // Text in memory is the saved copy, so it changes with a save. A document opened
                // meanwhile wins.
                if this.document == started {
                    this.document = document;
                }
                done(this, result, window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        // A list entry still being typed counts: commit it now, not when the dialog takes focus.
        self.commit_pending_entries(window, cx);
        if self.editor.dirty_count() == 0 {
            return;
        }
        let changes = self.editor.changes();
        let version = self.editor.state.version.clone();
        let scope = self.editor.scope.clone();
        let pending = changes.clone();
        self.in_background(
            move |store| store.apply(Some(&version), &scope, &changes),
            move |this, result, window, cx| match result {
                Ok(result) if result.after == result.before => {
                    this.editor.discard();
                    this.sync_all(window, cx);
                    window.push_notification("Nothing to save", cx);
                }
                Ok(result) => this.open_save_dialog(&result.before, &result.after, pending, window, cx),
                Err(err) => this.show_save_error(err, window, cx),
            },
            window,
            cx,
        );
    }

    fn open_save_dialog(
        &mut self,
        before: &str,
        after: &str,
        changes: Vec<(String, Value)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changes = Rc::new(changes);
        let diff = Rc::new(line_diff(before, after, 2));
        let file = self.file_name();
        let this = cx.entity().downgrade();
        let download = self.playground(cx).is_some();
        let (title, action) = if download {
            (format!("Download {file} with these changes?"), "Download")
        } else {
            (format!("Save changes to {file}?"), "Save")
        };
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme();
            let lines = diff.iter().map(|d| {
                let (marker, bg, fg) = match d.op {
                    DiffOp::Add => ("+", theme.success.opacity(0.15), theme.foreground),
                    DiffOp::Remove => ("-", theme.danger.opacity(0.15), theme.foreground),
                    DiffOp::Context => (" ", theme.transparent, theme.muted_foreground),
                    DiffOp::Gap => ("", theme.transparent, theme.muted_foreground),
                };
                let number = match d.op {
                    DiffOp::Gap => "⋯".to_string(),
                    DiffOp::Remove => String::new(),
                    _ => d.line.to_string(),
                };
                h_flex()
                    .bg(bg)
                    .text_color(fg)
                    .child(div().w_10().flex_shrink_0().pr_2().text_right().text_color(theme.muted_foreground).child(number))
                    .child(div().whitespace_nowrap().child(if d.op == DiffOp::Gap { String::new() } else { format!("{marker} {}", d.text) }))
            });
            let save = this.clone();
            let changes = changes.clone();
            dialog
                .title(title.clone())
                .w(px(720.))
                .child(
                    v_flex()
                        .gap_3()
                        .child(div().text_sm().text_color(theme.muted_foreground).child("Only these lines change. Everything else in the file stays as it is."))
                        .child(
                            div()
                                .id("diff")
                                .max_h_96()
                                .overflow_y_scrollbar()
                                .rounded(theme.radius)
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.secondary)
                                .py_2()
                                .font_family(theme.mono_font_family.clone())
                                .text_sm()
                                .children(lines),
                        ),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("cancel").outline().label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                        .child(Button::new("confirm-save").primary().label(action).on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let changes = (*changes).clone();
                            save.update(cx, |this, cx| this.confirm_save(changes, window, cx)).ok();
                        })),
                )
        });
    }

    /// Saves exactly the changes the dialog showed.
    fn confirm_save(&mut self, changes: Vec<(String, Value)>, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let version = self.editor.state.version.clone();
        let scope = self.editor.scope.clone();
        let saved = changes.clone();
        self.in_background(
            move |store| store.save(Some(&version), &scope, &changes).map(|result| (result, store.state())),
            move |this, result, window, cx| match result {
                Ok((result, state)) => {
                    this.editor.state = state;
                    // Only what was written; anything edited since stays pending.
                    this.editor.clear_saved(&saved);
                    this.errors.clear();
                    this.banner = None;
                    this.sync_all(window, cx);
                    if result.after != result.before {
                        this.download(cx);
                    }
                    window.push_notification(Notification::success(result.note.unwrap_or_else(|| "Saved.".into())), cx);
                }
                Err(err) => this.show_save_error(err, window, cx),
            },
            window,
            cx,
        );
    }

    fn show_save_error(&mut self, err: SaveError, window: &mut Window, cx: &mut Context<Self>) {
        match err {
            SaveError::Conflict => self.banner = Some(Banner::Conflict),
            SaveError::Invalid(errors) => {
                let mut first: Option<String> = None;
                for (key, message) in errors {
                    if self.editor.schema.find(&key).is_some() {
                        first.get_or_insert(key.clone());
                        self.errors.insert(key, message);
                    } else {
                        window.push_notification(Notification::error(message), cx);
                    }
                }
                if let Some(key) = first {
                    self.reveal(&key, cx);
                }
            }
            SaveError::Parse(message) => self.banner = Some(Banner::ParseError(message)),
            SaveError::Io(message) => window.push_notification(Notification::error(message), cx),
        }
    }

    /// Shows the page that holds a setting.
    fn reveal(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(s) = self.editor.schema.find(key) {
            self.active_group = s.group.clone();
            self.scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    // ---------- appearance ----------

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    fn apply_appearance(&mut self, appearance: Appearance, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance = appearance;
        match appearance {
            Appearance::System => Theme::sync_system_appearance(Some(window), cx),
            Appearance::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            Appearance::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        menus::set(&self.app_name(), appearance, cx);
        cx.notify();
    }

    fn set_appearance(&mut self, appearance: Appearance, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_appearance(appearance, window, cx);
        if let Some(path) = cx.try_global::<crate::AppearanceFile>().map(|f| f.0.clone()) {
            appearance.save(&path);
        }
    }

    /// The name shown in the menu bar, e.g. "Rio Settings".
    pub fn app_name(&self) -> String {
        crate::app_name(self.app)
    }

    fn show_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.app_name();
        let lines = [
            format!("Version {}", crate::VERSION),
            format!("Settings schema from {} {}", self.app.terminal, self.editor.schema.revision.chars().take(7).collect::<String>()),
            format!("Editing {}", tidy_path(&self.document.path())),
        ];
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            dialog
                .title(name.clone())
                .child(v_flex().gap_1().text_sm().text_color(muted).children(lines.iter().cloned()))
        });
    }

    // ---------- rendering ----------

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    fn visible(&self, s: &Setting, query: &str) -> bool {
        query.is_empty() || model::matches_search(&self.search_text[&s.key], query)
    }

    fn render_header(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let playground = self.playground(cx).is_some();
        let theme = cx.theme();
        let dirty = self.editor.dirty_count();
        let state = &self.editor.state;
        let path = if state.target != state.path {
            format!("{} → {}", tidy_path(&state.path), tidy_path(&state.target))
        } else {
            tidy_path(&state.path)
        };
        let appearance = self.appearance;
        let _ = window;
        h_flex()
            .h_16()
            .flex_shrink_0()
            .px_5()
            .gap_4()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .when_some(cx.try_global::<crate::LogoPath>().map(|l| l.0), |row, path| row.child(img(ImageSource::Resource(Resource::Embedded(path.into()))).size_9()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().font_semibold().child(self.editor.schema.title.clone()))
                    .child(div().truncate().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(path)),
            )
            .when(!self.editor.schema.scopes.is_empty(), |row| {
                row.child(
                    h_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(theme.muted_foreground).child("Applies to"))
                        .child(
                            div()
                                .id("scope")
                                .w_40()
                                .flex_none()
                                .when(dirty > 0, |d| d.tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Save or undo your changes before switching").build(window, cx)))
                                .child(Select::new(&self.scope_select).small().disabled(dirty > 0)),
                        ),
                )
            })
            .child(
                ButtonGroup::new("appearance")
                    .small()
                    .child(Button::new("system").icon(gpui_kit::assets::IconName::Monitor).label("System").selected(appearance == Appearance::System))
                    .child(Button::new("light").icon(gpui_kit::assets::IconName::Sun).label("Light").selected(appearance == Appearance::Light))
                    .child(Button::new("dark").icon(gpui_kit::assets::IconName::Moon).label("Dark").selected(appearance == Appearance::Dark))
                    .on_click(cx.listener(|this, ix: &Vec<usize>, window, cx| {
                        let next = match ix.first() {
                            Some(1) => Appearance::Light,
                            Some(2) => Appearance::Dark,
                            _ => Appearance::System,
                        };
                        this.set_appearance(next, window, cx);
                    })),
            )
            .when(playground, |row| {
                row.child(
                    Button::new("open")
                        .outline()
                        .icon(gpui_kit::assets::IconName::FolderOpen)
                        .label("Open…")
                        .disabled(self.busy)
                        .tooltip("Edit a config file from your computer")
                        .on_click(cx.listener(|this, _, window, cx| this.open_file(window, cx))),
                )
            })
            .child(
                Button::new("save")
                    .primary()
                    .when(playground, |b| b.icon(gpui_kit::assets::IconName::Download))
                    .label(if playground { "Download" } else { "Save" })
                    .disabled(dirty == 0 || self.busy)
                    // The web fonts have no ⌘ glyph, and a browser may not be on a Mac anyway.
                    .tooltip(if playground { "Download the changed file" } else { "Save (⌘S)" })
                    // The count pill beside the label, in the button's own colors. It always takes its
                    // space (just transparent at 0), so the button and the header row never change width.
                    .child(
                        div()
                            .min_w_5()
                            .px_1()
                            .rounded_full()
                            .text_center()
                            .text_xs()
                            .font_semibold()
                            .bg(theme.primary_foreground.opacity(0.18))
                            .text_color(theme.primary_foreground)
                            .when(dirty == 0, |pill| pill.opacity(0.))
                            .child(dirty.to_string()),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.start_save(window, cx))),
            )
            .into_any_element()
    }

    fn render_sidebar(&self, query: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let schema = self.editor.schema;
        let searching = !query.is_empty();
        let theme = cx.theme().clone();
        let items = schema.groups.iter().filter_map(|g| {
            let count = if searching {
                schema.settings.iter().filter(|s| s.group == g.id && self.visible(s, query)).count()
            } else {
                self.editor.in_file_count(&g.id)
            };
            if searching && count == 0 {
                return None;
            }
            let id = g.id.clone();
            let active = !searching && self.active_group == g.id;
            Some(
                SidebarMenuItem::new(g.title.clone())
                    .icon(group_icon(&g.icon))
                    .active(active)
                    .when(count > 0, |item| {
                        // Browsing: how many of the section's settings your config sets. Searching: matches.
                        let what: SharedString = match (searching, count) {
                            (true, 1) => "1 matching setting".into(),
                            (true, n) => format!("{n} matching settings").into(),
                            (false, 1) => "1 setting here is in your config".into(),
                            (false, n) => format!("{n} settings here are in your config").into(),
                        };
                        let badge_id = ElementId::Name(format!("group-count:{}", g.id).into());
                        item.suffix(move |_, cx| {
                            let what = what.clone();
                            div()
                                .id(badge_id.clone())
                                .px_1p5()
                                .rounded_full()
                                .text_xs()
                                .bg(cx.theme().secondary)
                                .text_color(cx.theme().muted_foreground)
                                .child(count.to_string())
                                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(what.clone()).build(window, cx))
                        })
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.active_group = id.clone();
                        this.search.update(cx, |s, cx| s.set_value("", window, cx));
                        this.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    })),
            )
        });
        let revision: String = schema.revision.chars().take(7).collect();
        // Laid out here rather than with GPUI Kit's `Sidebar`: its menu list sizes to the menu, so on
        // a short window the sections ran over the footer instead of scrolling. Same look: the
        // theme's sidebar colors and the Sidebar's spacing.
        let menu = SidebarItem::render(SidebarMenu::new().children(items), "groups-menu", window, cx);
        v_flex()
            .id("groups")
            .w_64()
            .h_full()
            .flex_shrink_0()
            .bg(theme.tokens.sidebar)
            .text_color(theme.sidebar_foreground)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                div().pt_3().px_3().child(
                    Input::new(&self.search)
                        .id("search")
                        .small()
                        .prefix(IconName::Search)
                        .suffix(Kbd::new(Keystroke::parse("/").unwrap_or_default()).outline()),
                ),
            )
            .child(
                // The scroll area takes the space between search and footer (and gives its id to its
                // own scroll layers, hence the wrapper that holds "groups-scroll").
                div()
                    .id("groups-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .child(div().size_full().overflow_y_scrollbar().child(div().p_3().child(menu))),
            )
            .child(
                v_flex()
                    .id("groups-footer")
                    .test_support()
                    .flex_none()
                    .px_5()
                    .pb_3()
                    .pt_1()
                    .gap_0p5()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Schema from {} {revision}", schema.terminal))
                    .child(format!("{} {}", self.app.name, crate::VERSION)),
            )
            .into_any_element()
    }

    fn render_row(&self, s: &'static Setting, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let lock = self.editor.lock(s);
        let disabled = lock.is_some() || self.busy;
        let dirty = self.editor.is_dirty(&s.key);
        let set = self.editor.is_set(s);
        let in_file = self.editor.file_value(s).is_some();
        let error = self.errors.get(&s.key).cloned();
        let wide = matches!(s.ty, SettingType::Records | SettingType::Raw);
        let tooltip: Option<SharedString> = match lock {
            Some(Lock::Scope) => Some("This setting can't be overridden per platform".into()),
            Some(Lock::Theme) => Some(format!("{} ignores colors while a theme is set", self.app.terminal).into()),
            None => None,
        };
        let key = s.key.clone();
        let reset = if dirty || set {
            let undo = dirty;
            Button::new(ElementId::Name(format!("{}:reset", s.key).into()))
                .ghost()
                .small()
                .icon(if undo { gpui_kit::assets::IconName::Undo2 } else { gpui_kit::assets::IconName::CircleX })
                .tooltip(if undo { "Undo this change" } else { "Reset to default (removes the line from the file)" })
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, window, cx| this.reset(&key, window, cx)))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let current = current_platform();

        let label = v_flex()
            .id(ElementId::Name(format!("{}:label", s.key).into()))
            .test_support()
            .gap_1()
            .min_w_0()
            .flex_1()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(div().font_medium().child(s.label.clone()))
                    .when(in_file, |r| {
                        r.child(
                            div()
                                .id(ElementId::Name(format!("{}:set", s.key).into()))
                                .test_support()
                                .child(Tag::primary().xsmall().outline().child("In your config")),
                        )
                    })
                    .children(s.platforms.iter().flatten().map(|p| {
                        let tag = if p == current { Tag::primary() } else { Tag::secondary() };
                        tag.xsmall().outline().child(platform_label(p).to_string())
                    }))
                    .children(s.tags.iter().map(|t| {
                        let tag = if t.starts_with("Restart") { Tag::warning() } else { Tag::secondary() };
                        tag.xsmall().child(t.clone())
                    }))
                    .when(s.inferred, |r| r.child(Tag::warning().xsmall().child("new"))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().font_family(theme.mono_font_family.clone()).child(s.key.clone()))
                    .child("·")
                    .child(self.editor.default_note(s)),
            )
            .when_some(s.doc.clone(), |c, doc| c.child(div().text_sm().text_color(theme.muted_foreground).child(doc.replace('`', ""))))
            .when_some(s.built_ins_note.clone(), |c, note| {
                c.child(div().text_sm().text_color(theme.muted_foreground).child(format!("Lines here are added to {}'s {note}.", self.app.terminal)))
            })
            .when_some(s.doc_more.clone(), |c, more| {
                let key = s.key.clone();
                let open = self.expanded.contains(&s.key);
                c.child(
                    h_flex().child(
                        Button::new(ElementId::Name(format!("{}:more", s.key).into()))
                            .ghost()
                            .xsmall()
                            .label(if open { "Less" } else { "More" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.expanded.remove(&key) {
                                    this.expanded.insert(key.clone());
                                }
                                cx.notify();
                            })),
                    ),
                )
                .when(open, |c| {
                    c.child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(gpui_kit::component::text::TextView::markdown(ElementId::Name(format!("{}:doc", s.key).into()), more)),
                    )
                })
            })
            .when_some(error.clone(), |c, e| c.child(div().text_sm().text_color(theme.danger).child(e)));

        let mut control = self.render_control(s, disabled, cx);
        if s.pick.is_some() && !disabled {
            control = self.path_drop_target(s, control, cx);
        }

        let body = if wide {
            v_flex().gap_3().child(h_flex().gap_4().child(label).child(div().w_8().child(reset))).child(control)
        } else {
            v_flex().child(h_flex().gap_4().items_start().child(label).child(control).child(div().w_8().flex_shrink_0().child(reset)))
        };

        div()
            .id(ElementId::Name(format!("row:{}", s.key).into()))
            .test_support()
            .relative()
            .px_4()
            .py_3()
            .border_l_2()
            .border_color(if dirty { theme.primary } else if error.is_some() { theme.danger } else { theme.transparent })
            .when(disabled, |d| d.opacity(0.5))
            .when_some(tooltip, |d, t| d.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(t.clone()).build(window, cx)))
            .child(body)
    }

    fn render_group(&self, group_id: &str, query: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let schema = self.editor.schema;
        let group = schema.groups.iter().find(|g| g.id == group_id)?;
        let settings: Vec<&'static Setting> = schema.settings.iter().filter(|s| s.group == group.id && self.visible(s, query)).collect();
        if settings.is_empty() {
            return None;
        }
        // Subgroups in order of first appearance; settings without one come first.
        let mut subgroups: Vec<Option<String>> = Vec::new();
        if settings.iter().any(|s| s.subgroup.is_none()) {
            subgroups.push(None);
        }
        for s in &settings {
            if s.subgroup.is_some() && !subgroups.contains(&s.subgroup) {
                subgroups.push(s.subgroup.clone());
            }
        }
        let theme = cx.theme().clone();
        let cards: Vec<AnyElement> = subgroups
            .into_iter()
            .map(|sub| {
                let rows: Vec<_> = settings.iter().filter(|s| s.subgroup == sub).map(|s| self.render_row(s, cx).into_any_element()).collect();
                GroupBox::new()
                    .fill()
                    .when_some(sub, |g, title| g.title(title.to_uppercase()))
                    .child(v_flex().children(rows))
                    .into_any_element()
            })
            .collect();

        // The preview reads `colors.*` keys: only for terminals with a [colors] table that a theme locks (Rio).
        let colors_extras = (group.id == "colors" && query.is_empty() && self.editor.schema.theme_locks_colors).then(|| self.render_colors_extras(cx));
        Some(
            v_flex()
                .gap_4()
                .child(
                    h_flex()
                        .gap_3()
                        .child(div().text_color(theme.primary).child(gpui_kit::component::Icon::new(group_icon(&group.icon)).size_6()))
                        .child(div().text_2xl().font_semibold().child(group.title.clone())),
                )
                .when_some(group.doc.clone(), |c, doc| c.child(div().text_color(theme.muted_foreground).child(doc.replace('`', ""))))
                .children(colors_extras)
                .children(cards)
                .into_any_element(),
        )
    }

    /// The colors page's live terminal preview, plus the notice when a theme overrides colors.
    fn render_colors_extras(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let color = |name: &str| self.editor.color(name).and_then(|h| crate::controls::parse_hex(&h));
        let fg = color("foreground").unwrap_or(theme.foreground);
        let c = |name: &str| color(name).unwrap_or(fg);
        let span = |name: &str, text: &str| div().text_color(c(name)).child(text.to_string());
        let swatches = |prefix: &str| {
            h_flex().gap_1p5().children(ANSI.iter().map(|a| div().w_8().h_5().rounded(theme.radius).bg(c(&format!("{prefix}{a}")))))
        };
        let locked = self.editor.schema.theme_locks_colors && self.editor.theme_active();
        v_flex()
            .gap_3()
            .when(locked, |col| {
                col.child(with_action(
                    Alert::info(
                        "colors-locked",
                        format!("{} ignores these colors while a theme is set. Clear the theme to use them.", self.app.terminal),
                    ),
                    Button::new("clear-theme").small().outline().label("Clear theme").on_click(cx.listener(|this, _, window, cx| {
                        this.set_and_sync("theme", Value::Null, window, cx);
                        this.active_group = "theme".into();
                    })),
                ))
            })
            .child(
                // The preview paints the user's own terminal colors, not UI tokens.
                v_flex()
                    .gap_2()
                    .p_4()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(color("background").unwrap_or(theme.background))
                    .text_color(fg)
                    .font_family(theme.mono_font_family.clone())
                    .text_sm()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(span("green", "➜"))
                            .child(span("cyan", "~/project"))
                            .child(h_flex().child(span("blue", "git:(")).child(span("red", "main")).child(span("blue", ")")))
                            .child("ls -la"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(span("blue", "drwxr-xr-x"))
                            .child(span("yellow", "settings"))
                            .child(span("magenta", "config.toml"))
                            .child(
                                div()
                                    .px_0p5()
                                    .bg(color("selection-background").unwrap_or(theme.selection))
                                    .text_color(color("selection-foreground").unwrap_or(fg))
                                    .child("selected text"),
                            )
                            .child(div().w_2().h_4().bg(c("cursor"))),
                    )
                    .child(swatches(""))
                    .child(swatches("light-")),
            )
            .into_any_element()
    }

    fn render_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let file = self.file_name();
        match self.banner.as_ref()? {
            Banner::Conflict => Some(
                with_action(
                    Alert::warning("conflict", format!("{file} changed on disk (maybe you edited it in your editor).")),
                    Button::new("reload").small().outline().label("Reload and discard my changes").on_click(cx.listener(|this, _, window, cx| {
                        this.banner = None;
                        this.reload(window, cx);
                    })),
                )
                .into_any_element(),
            ),
            Banner::ParseError(message) => Some(
                Alert::error("parse-error", if self.document.is_file() {
                    format!("{message} Fix the file in your editor; this window updates when it's valid again.")
                } else {
                    format!("{message} Fix the file in your editor, then open it again.")
                })
                    .into_any_element(),
            ),
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query(cx);
        let theme = cx.theme().clone();
        let header = self.render_header(window, cx);
        let sidebar = self.render_sidebar(&query, window, cx);
        let banner = self.render_banner(cx);

        let pages: Vec<AnyElement> = if query.is_empty() {
            self.render_group(&self.active_group.clone(), "", cx).into_iter().collect()
        } else {
            self.editor.schema.groups.iter().filter_map(|g| self.render_group(&g.id, &query, cx)).collect()
        };
        let empty = pages.is_empty();
        let scope_note = (!self.editor.scope.is_empty()).then(|| {
            let scope = &self.editor.scope;
            Alert::info(
                "scope",
                format!(
                    "Editing {} overrides ([platform.{scope}] in {}). Dimmed settings can't be overridden per platform; switch “Applies to” back to All platforms to edit them.",
                    platform_label(scope),
                    self.file_name()
                ),
            )
        });
        let notices = self.editor.state.notices.clone();

        v_flex()
            .id("settings-window")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(|this, _: &About, window, cx| this.show_about(window, cx)))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                if this.confirm_discard(false, window, cx) {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &Zoom, window, _| window.zoom_window()))
            .on_action(cx.listener(|this, _: &AppearanceSystem, window, cx| this.set_appearance(Appearance::System, window, cx)))
            .on_action(cx.listener(|this, _: &AppearanceLight, window, cx| this.set_appearance(Appearance::Light, window, cx)))
            .on_action(cx.listener(|this, _: &AppearanceDark, window, cx| this.set_appearance(Appearance::Dark, window, cx)))
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                if this.confirm_discard(true, window, cx) {
                    cx.quit();
                }
            }))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| this.search.update(cx, |s, cx| s.focus(window, cx))))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(header)
            .children(self.render_playground_banner(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(sidebar)
                    .child(
                        // The scroll bar overlays the scroll area, so it sits in a positioned wrapper after it.
                        div()
                            .relative()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .child(
                                div().id("content").size_full().overflow_y_scroll().track_scroll(&self.scroll).child(
                                    v_flex()
                                        .max_w(rems(56.))
                                        .mx_auto()
                                        .px_8()
                                        .py_8()
                                        .gap_8()
                                        .children(banner)
                                        .children(scope_note)
                                        .children(notices.into_iter().map(|n| div().text_sm().text_color(theme.muted_foreground).child(n)))
                                        .children(pages)
                                        .when(empty, |c| {
                                            c.child(div().py_16().text_center().text_color(theme.muted_foreground).child("No settings match your search."))
                                        }),
                                ),
                            )
                            .vertical_scrollbar(&self.scroll),
                    ),
            )
    }
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
