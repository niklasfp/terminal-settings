//! Retained state, persistence, and background worker coordination for `SettingsWindow`.

use crate::controls::{margin_compress, margin_expand, number, parse_hex_color};
use crate::model::{self, Editor, format_number, plain, same};
use crate::playground::{OpenedFile, PendingOpen, Playground};
use crate::Brand;
use rune::{Color, Image, ImageCache, Key, KeyEvent, TextEdit, Tx, Waker};
use rune_kit::{ComboboxOption, ComboboxState, Kit, PaletteSeed};
use serde_json::{Map, Value};
use settings_core::{
    AppDefinition, ApplyResult, ConfigState, DiffLine, Document, DocumentStore, FieldErrors,
    SaveError, Setting, SettingType, ThemeInfo, line_diff,
};
use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::future::Future;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path).ok().as_deref().map(str::trim) {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    pub fn save(self, path: &Path) {
        let s = match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, s);
    }
}

pub enum Banner {
    Conflict,
    ParseError(String),
}

pub struct SavePreview {
    pub before: String,
    pub after: String,
    pub diff: Vec<DiffLine>,
    pub changes: Vec<(String, Value)>,
}

pub type PreviewPayload = (String, String, Vec<(String, Value)>);
pub type SaveOutcome = (ApplyResult, ConfigState);

pub struct SaveDonePayload {
    pub result: Result<SaveOutcome, SaveError>,
    pub saved: Vec<(String, Value)>,
    pub document: Document,
}

pub enum AsyncMsg {
    PreviewReady(Result<PreviewPayload, SaveError>),
    SaveDone(Box<SaveDonePayload>),
    ThemesLoaded(Vec<ThemeInfo>),
    ThemeInstalled {
        key: String,
        req_id: u64,
        theme: String,
        result: Result<Option<String>, String>,
    },
    FilePicked(Option<OpenedFile>),
    FileDropped(OpenedFile),
    CopyFinished {
        ok: bool,
        changes: Vec<(String, Value)>,
        name: String,
    },
}

pub struct SettingsWindow {
    pub app: &'static AppDefinition,
    pub brand: Brand,
    pub document: Document,
    pub playground: Option<Playground>,
    pub discard_prompt: Option<PendingOpen>,
    pub editor: Editor,
    pub edits: HashMap<String, TextEdit>,
    pub combos: HashMap<String, ComboboxState>,
    pub record_rows: HashMap<String, Vec<u64>>,
    pub next_row_id: u64,
    pub open_select: Option<String>,
    pub color_picker: Option<crate::color_picker::ColorPickerState>,
    pub themes: Vec<ThemeInfo>,
    pub theme_options: Vec<ComboboxOption>,
    pub theme_requests: HashMap<String, u64>,
    pub errors: FieldErrors,
    pub recheck: HashSet<String>,
    pub search: TextEdit,
    pub search_text: HashMap<String, String>,
    pub active_group: String,
    pub appearance: Appearance,
    pub appearance_file: Option<PathBuf>,
    pub banner: Option<Banner>,
    pub busy: bool,
    pub expanded: HashSet<String>,
    pub save_preview: Option<SavePreview>,
    pub toast: Option<String>,
    pub logo: Option<Image>,
    pub tx_async: Sender<AsyncMsg>,
    pub rx_async: Arc<Mutex<Receiver<AsyncMsg>>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub pending_tasks: Vec<Pin<Box<dyn Future<Output = AsyncMsg>>>>,
    pub waker: Option<Waker>,
    pub last_poll_sec: f64,
}

impl SettingsWindow {
    pub fn new(
        app: &'static AppDefinition,
        brand: Brand,
        document: impl Into<Document>,
        appearance_file: Option<PathBuf>,
    ) -> Self {
        let document = document.into();
        let state = DocumentStore::new(app, document.clone()).state();
        let schema = &app.schema;
        let appearance = appearance_file.as_deref().map(Appearance::load).unwrap_or_default();
        let (tx_async, rx_async) = mpsc::channel();
        let mut this = Self {
            app,
            brand,
            banner: state.parse_error.clone().map(Banner::ParseError),
            editor: Editor::new(schema, state),
            document,
            playground: None,
            discard_prompt: None,
            edits: HashMap::new(),
            combos: HashMap::new(),
            record_rows: HashMap::new(),
            next_row_id: 0,
            open_select: None,
            color_picker: None,
            themes: Vec::new(),
            theme_options: Vec::new(),
            theme_requests: HashMap::new(),
            errors: FieldErrors::new(),
            recheck: HashSet::new(),
            search: TextEdit::new(""),
            search_text: schema
                .settings
                .iter()
                .map(|s| (s.key.clone(), model::search_text(schema, s)))
                .collect(),
            active_group: schema.groups.first().map(|g| g.id.clone()).unwrap_or_default(),
            appearance,
            appearance_file,
            busy: false,
            expanded: HashSet::new(),
            save_preview: None,
            toast: None,
            logo: decode_logo(brand.logo, None),
            tx_async,
            rx_async: Arc::new(Mutex::new(rx_async)),
            #[cfg(not(target_arch = "wasm32"))]
            pending_tasks: Vec::new(),
            waker: None,
            last_poll_sec: 0.0,
        };
        this.sync_all();
        if let Some(themes_fn) = app.themes {
            this.set_themes(themes_fn(&this.document.path()));
        }
        this
    }

    pub fn setup(&mut self, waker: Waker) {
        self.waker = Some(waker.clone());
        #[cfg(target_arch = "wasm32")]
        {
            self.logo = decode_logo(self.brand.logo, Some(waker.clone()));
            if let Some(next_fn) = self.playground.as_ref().and_then(|p| p.next_drop) {
                let tx = self.tx_async.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    loop {
                        if let Some(file) = next_fn().await {
                            if tx.send(AsyncMsg::FileDropped(file)).is_err() {
                                break;
                            }
                            waker();
                        }
                    }
                });
            }
        }
    }

    pub fn set_themes(&mut self, themes: Vec<ThemeInfo>) {
        self.theme_options = themes
            .iter()
            .map(|t| {
                let mut o = ComboboxOption::new(t.name.clone(), t.name.clone());
                if !t.installed {
                    o.detail = "Download".into();
                }
                o
            })
            .collect();
        self.themes = themes;
    }

    pub fn sync_all(&mut self) {
        let keys: Vec<String> = self.editor.schema.settings.iter().map(|s| s.key.clone()).collect();
        for key in keys {
            self.sync_setting(&key);
        }
    }

    pub fn store(&self) -> DocumentStore<'static> {
        DocumentStore::new(self.app, self.document.clone())
    }

    pub fn file_name(&self) -> String {
        self.document.file_name().unwrap_or_else(|| "the config file".into())
    }

    pub fn edit(&mut self, key: &str, value: Value) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let error = if self.recheck.contains(key) { self.value_error(s, &value) } else { None };
        self.editor.edit(s, value);
        match error {
            Some(message) => {
                self.errors.insert(key.to_string(), message);
            }
            None => {
                self.errors.remove(key);
            }
        }
    }

    pub fn set_and_sync(&mut self, key: &str, value: Value) {
        self.edit(key, value);
        self.sync_setting(key);
    }

    fn value_error(&self, s: &'static Setting, value: &Value) -> Option<String> {
        let change = settings_core::Change { setting: s, value: value.clone() };
        match self.app.format.apply("", &[change], "") {
            Err(settings_core::FormatError::Invalid(mut errors)) => errors.remove(&s.key),
            _ => None,
        }
    }

    pub fn reset(&mut self, key: &str) {
        let Some(s) = self.editor.schema.find(key) else { return };
        if s.ty == SettingType::Theme {
            *self.theme_requests.entry(key.to_string()).or_default() += 1;
        }
        self.editor.reset(s);
        self.errors.remove(key);
        self.recheck.remove(key);
        self.sync_setting(key);
    }

    pub fn reload(&mut self) {
        let state = self.store().state();
        self.banner = state.parse_error.clone().map(Banner::ParseError);
        self.editor.state = state;
        self.editor.discard();
        self.theme_requests.values_mut().for_each(|n| *n += 1);
        self.errors.clear();
        self.recheck.clear();
        self.sync_all();
    }

    pub fn choose_theme(&mut self, key: &str, chosen: &str) {
        if let Some(combo) = self.combos.get_mut(key) {
            combo.close();
        }
        let needs_install = self.themes.iter().find(|t| t.name == chosen).is_some_and(|t| !t.installed);
        if needs_install {
            if let Some(download) = self.playground.as_ref().and_then(|p| p.install_theme) {
                let req = self.theme_requests.entry(key.to_string()).or_default();
                *req += 1;
                let (req_id, theme, key_owned) = (*req, chosen.to_string(), key.to_string());
                self.toast = Some(format!("Downloading “{chosen}”…"));
                let fut = download(chosen);
                self.spawn_playground_task(Box::pin(async move {
                    AsyncMsg::ThemeInstalled {
                        key: key_owned,
                        req_id,
                        theme,
                        result: fut.await.map(Some),
                    }
                }));
                return;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(install) = self.app.install_theme
                && self.document.is_file()
            {
                let req = self.theme_requests.entry(key.to_string()).or_default();
                *req += 1;
                let (req_id, path, theme, key_owned) = (*req, self.document.path(), chosen.to_string(), key.to_string());
                let (tx, waker) = (self.tx_async.clone(), self.waker.clone());
                self.toast = Some(format!("Downloading theme “{chosen}”…"));
                std::thread::spawn(move || {
                    let result = install(&path, &theme).map(|()| None);
                    let _ = tx.send(AsyncMsg::ThemeInstalled { key: key_owned, req_id, theme, result });
                    if let Some(w) = waker {
                        w();
                    }
                });
                return;
            }
        }
        self.set_and_sync(key, Value::String(chosen.to_string()));
    }

    /// Runs `apply` and opens the diff confirmation dialog.
    pub fn start_save(&mut self) {
        if self.busy {
            return;
        }
        self.commit_pending_entries();
        if self.editor.dirty_count() == 0 {
            return;
        }
        let (changes, version, scope, store) = (
            self.editor.changes(),
            self.editor.state.version.clone(),
            self.editor.scope.clone(),
            self.store(),
        );
        #[cfg(not(target_arch = "wasm32"))]
        if self.document.is_file()
            && let Some(waker) = self.waker.clone()
        {
            self.busy = true;
            let tx = self.tx_async.clone();
            std::thread::spawn(move || {
                let res = store.apply(Some(&version), &scope, &changes).map(|r| (r.before, r.after, changes));
                let _ = tx.send(AsyncMsg::PreviewReady(res));
                waker();
            });
            return;
        }
        let res = store.apply(Some(&version), &scope, &changes).map(|r| (r.before, r.after, changes));
        self.finish_preview(res);
    }

    pub(crate) fn finish_preview(&mut self, res: Result<PreviewPayload, SaveError>) {
        self.busy = false;
        match res {
            Ok((before, after, _)) if after == before => {
                self.editor.discard();
                self.sync_all();
                self.toast = Some("Nothing to save".into());
            }
            Ok((before, after, changes)) => {
                let diff = line_diff(&before, &after, 2);
                self.save_preview = Some(SavePreview { before, after, diff, changes });
            }
            Err(err) => self.show_save_error(err),
        }
    }

    pub fn confirm_save(&mut self) {
        let Some(preview) = self.save_preview.take() else { return };
        if self.busy {
            return;
        }
        let changes = preview.changes;
        let saved = changes.clone();
        let (version, scope, mut store) = (
            self.editor.state.version.clone(),
            self.editor.scope.clone(),
            self.store(),
        );
        #[cfg(not(target_arch = "wasm32"))]
        if self.document.is_file()
            && let Some(waker) = self.waker.clone()
        {
            self.busy = true;
            let tx = self.tx_async.clone();
            std::thread::spawn(move || {
                let result = store.save(Some(&version), &scope, &changes).map(|r| (r, store.state()));
                let _ = tx.send(AsyncMsg::SaveDone(Box::new(SaveDonePayload {
                    result,
                    saved,
                    document: store.document,
                })));
                waker();
            });
            return;
        }
        let result = store.save(Some(&version), &scope, &changes).map(|r| (r, store.state()));
        let doc = store.document;
        self.finish_save(result, &saved, doc, None);
    }

    pub(crate) fn finish_save(
        &mut self,
        result: Result<SaveOutcome, SaveError>,
        saved: &[(String, Value)],
        document: Document,
        note: Option<String>,
    ) {
        self.busy = false;
        self.document = document;
        match result {
            Ok((res, state)) => {
                self.editor.state = state;
                self.editor.clear_saved(saved);
                self.errors.clear();
                self.recheck.clear();
                self.banner = None;
                self.sync_all();
                self.toast = Some(note.or(res.note).unwrap_or_else(|| "Saved.".into()));
            }
            Err(err) => self.show_save_error(err),
        }
    }

    fn show_save_error(&mut self, err: SaveError) {
        match err {
            SaveError::Conflict => self.banner = Some(Banner::Conflict),
            SaveError::Invalid(errors) => {
                let mut first: Option<String> = None;
                for (key, message) in errors {
                    if self.editor.schema.find(&key).is_some() {
                        first.get_or_insert(key.clone());
                        self.recheck.insert(key.clone());
                        self.errors.insert(key, message);
                    } else {
                        self.toast = Some(message);
                    }
                }
                if let Some(key) = first
                    && let Some(s) = self.editor.schema.find(&key)
                {
                    self.active_group = s.group.clone();
                }
            }
            SaveError::Parse(message) => self.banner = Some(Banner::ParseError(message)),
            SaveError::Io(message) => self.toast = Some(message),
        }
    }

    pub(crate) fn check_disk_changes(&mut self) -> bool {
        let state = self.store().state();
        if let Some(err) = &state.parse_error {
            self.banner = Some(Banner::ParseError(err.clone()));
            return true;
        }
        let mut changed = false;
        if matches!(self.banner, Some(Banner::ParseError(_))) {
            self.banner = None;
            changed = true;
        }
        if state.version == self.editor.state.version {
            return changed;
        }
        if self.editor.dirty_count() > 0 {
            self.banner = Some(Banner::Conflict);
            return true;
        }
        self.editor.state = state;
        self.sync_all();
        self.toast = Some(format!("Reloaded changes from {}", self.file_name()));
        true
    }

    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if matches!(ev.key, Key::Escape) && self.color_picker.take().is_some() {
            return true;
        }
        if ev.is_command('s')
            || (ev.mods.ctrl && matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case("s")))
        {
            self.start_save();
            return true;
        }
        if ev.is_command('f')
            || (ev.mods.ctrl && matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case("f")))
            || (tx.focused().is_none() && matches!(&ev.key, Key::Text(t) if t == "/"))
        {
            self.search.focused = true;
            return true;
        }
        false
    }

    pub(crate) fn query(&self) -> String {
        self.search.text.trim().to_lowercase()
    }

    pub(crate) fn visible(&self, s: &Setting, query: &str) -> bool {
        query.is_empty() || model::matches_search(&self.search_text[&s.key], query)
    }

    pub(crate) fn kit(&self, system_dark: bool) -> (Kit, Color) {
        let dark = match self.appearance {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        };
        let p = if dark { &self.brand.dark } else { &self.brand.light };
        let bg = parse_hex_color(p.bg).unwrap_or(Color::BLACK);
        let fg = parse_hex_color(p.fg).unwrap_or(Color::WHITE);
        let accent = parse_hex_color(p.accent).unwrap_or(bg);
        let sidebar = parse_hex_color(p.sidebar).unwrap_or(bg);
        (Kit::from_palette(PaletteSeed { bg, fg, accent }), sidebar)
    }

    pub(crate) fn put_edit(&mut self, edit_key: String, text: String) {
        let entry = self.edits.entry(edit_key).or_default();
        if entry.text != text {
            entry.replace_text(text);
        }
    }

    /// Synchronises a setting's retained `TextEdit`s from `self.editor.current_value(s)`.
    pub(crate) fn sync_setting(&mut self, key: &str) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let v = self.editor.current_value(s);
        match s.ty {
            SettingType::String
            | SettingType::Path
            | SettingType::Font
            | SettingType::Theme
            | SettingType::Color
            | SettingType::Raw => self.put_edit(key.to_string(), plain(&v)),
            SettingType::FontStyle => {
                let t = match &v {
                    Value::Bool(false) => String::new(),
                    Value::String(st) if st == "default" => String::new(),
                    _ => plain(&v),
                };
                self.put_edit(key.to_string(), t);
            }
            SettingType::Int | SettingType::Float => {
                self.put_edit(key.to_string(), v.as_f64().map(format_number).unwrap_or_default());
            }
            SettingType::Margin => {
                for (i, n) in margin_expand(&v).into_iter().enumerate() {
                    self.put_edit(format!("{key}:{i}"), format_number(n));
                }
            }
            SettingType::Pair => {
                let nums: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                for i in 0..2 {
                    self.put_edit(format!("{key}:{i}"), nums.get(i).copied().map(format_number).unwrap_or_default());
                }
            }
            SettingType::Strings | SettingType::Fonts => {
                self.edits.entry(format!("{key}:chip")).or_default();
            }
            SettingType::Records => {
                let fields = s.fields.as_deref().unwrap_or_default();
                let wanted = v.as_array().cloned().unwrap_or_default();
                let current_ids = self.record_rows.entry(key.to_string()).or_default().clone();
                let current_values: Vec<Value> = current_ids
                    .iter()
                    .map(|row_id| self.record_row_value(key, *row_id, fields))
                    .collect();
                let norm = |items: &[Value]| -> Vec<Value> {
                    items
                        .iter()
                        .map(|o| {
                            Value::Object(
                                o.as_object()
                                    .map(|m| m.iter().map(|(k, v)| (k.clone(), Value::String(plain(v)))).collect())
                                    .unwrap_or_default(),
                            )
                        })
                        .collect()
                };
                if current_ids.len() != wanted.len()
                    || !same(&Value::Array(norm(&current_values)), &Value::Array(norm(&wanted)))
                {
                    let mut ids = Vec::with_capacity(wanted.len());
                    for row_val in &wanted {
                        self.next_row_id += 1;
                        let id = self.next_row_id;
                        ids.push(id);
                        for f in fields {
                            let cell = row_val.get(&f.key).map(plain).unwrap_or_default();
                            self.put_edit(format!("{key}:row:{id}:{}", f.key), cell);
                        }
                    }
                    self.record_rows.insert(key.to_string(), ids);
                }
            }
            SettingType::Bool | SettingType::Blur | SettingType::Enum | SettingType::Flags => {}
        }
    }

    pub(crate) fn record_row_value(&self, key: &str, row_id: u64, fields: &[settings_core::Field]) -> Value {
        let mut obj = Map::new();
        for f in fields {
            let cell_key = format!("{key}:row:{row_id}:{}", f.key);
            let t = self.edits.get(&cell_key).map(|e| e.text.trim().to_string()).unwrap_or_default();
            let val = match f.ty {
                SettingType::Int | SettingType::Float => {
                    t.parse::<f64>().ok().filter(|n| n.is_finite()).map_or(Value::String(t), number)
                }
                _ => Value::String(t),
            };
            obj.insert(f.key.clone(), val);
        }
        Value::Object(obj)
    }

    pub(crate) fn on_text_change(&mut self, key: &str) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let text_of = |k: &str| self.edits.get(k).map(|e| e.text.clone()).unwrap_or_default();
        let value = match s.ty {
            SettingType::FontStyle => {
                if self.editor.current_value(s) == Value::Bool(false) {
                    return;
                }
                let t = text_of(key);
                let t = t.trim();
                Value::String(if t.is_empty() { "default".into() } else { t.into() })
            }
            SettingType::String | SettingType::Path | SettingType::Font | SettingType::Theme => {
                let t = text_of(key);
                if t.is_empty() { Value::Null } else { Value::String(t) }
            }
            SettingType::Color => {
                let t = text_of(key).trim().to_string();
                if t.is_empty() { Value::Null } else { Value::String(t) }
            }
            SettingType::Int | SettingType::Float => {
                let t = text_of(key);
                let t = t.trim();
                if t.is_empty() {
                    Value::Null
                } else {
                    match t.parse::<f64>() {
                        Ok(n) if n.is_finite() => number(n),
                        _ => return,
                    }
                }
            }
            SettingType::Margin => {
                let nums: [f64; 4] = std::array::from_fn(|i| {
                    text_of(&format!("{key}:{i}")).trim().parse::<f64>().unwrap_or(0.0)
                });
                let list = margin_compress(nums[0], nums[1], nums[2], nums[3]);
                Value::Array(list.into_iter().map(number).collect())
            }
            SettingType::Pair => {
                let nums: Vec<Value> = (0..2)
                    .map(|i| number(text_of(&format!("{key}:{i}")).trim().parse::<f64>().unwrap_or(0.0)))
                    .collect();
                Value::Array(nums)
            }
            SettingType::Records => {
                let fields = s.fields.as_deref().unwrap_or_default();
                let ids = self.record_rows.get(key).cloned().unwrap_or_default();
                Value::Array(ids.into_iter().map(|id| self.record_row_value(key, id, fields)).collect())
            }
            SettingType::Raw => {
                let t = text_of(key);
                if t.trim().is_empty() { Value::Null } else { Value::String(t) }
            }
            _ => return,
        };
        self.edit(key, value);
    }

    pub(crate) fn commit_pending_entries(&mut self) {
        let keys: Vec<String> = self
            .editor
            .schema
            .settings
            .iter()
            .filter(|s| matches!(s.ty, SettingType::Strings | SettingType::Fonts))
            .map(|s| s.key.clone())
            .collect();
        for key in keys {
            self.commit_chip_draft(&key);
        }
    }

    pub(crate) fn commit_chip_draft(&mut self, key: &str) {
        let draft_key = format!("{key}:chip");
        let raw = self.edits.get(&draft_key).map(|e| e.text.clone()).unwrap_or_default();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let items: Vec<String> = if self.app.list_comma_separates {
            trimmed.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
        } else {
            vec![trimmed.to_string()]
        };
        if let Some(edit) = self.edits.get_mut(&draft_key) {
            edit.clear();
        }
        for item in items {
            self.add_chip(key, item);
        }
    }

    pub(crate) fn add_chip(&mut self, key: &str, text: String) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let Some(s) = self.editor.schema.find(key) else { return };
        let mut items: Vec<Value> = self.editor.current_value(s).as_array().cloned().unwrap_or_default();
        items.push(Value::String(text.to_string()));
        self.edit(key, Value::Array(items));
    }

    pub(crate) fn remove_chip(&mut self, key: &str, ix: usize) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let mut items: Vec<Value> = self.editor.current_value(s).as_array().cloned().unwrap_or_default();
        if ix < items.len() {
            items.remove(ix);
            self.edit(key, Value::Array(items));
        }
    }

    pub(crate) fn add_record(&mut self, key: &str) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let fields = s.fields.as_deref().unwrap_or_default();
        self.next_row_id += 1;
        let id = self.next_row_id;
        for f in fields {
            self.put_edit(format!("{key}:row:{id}:{}", f.key), String::new());
        }
        self.record_rows.entry(key.to_string()).or_default().push(id);
        self.on_text_change(key);
    }

    pub(crate) fn remove_record(&mut self, key: &str, row_id: u64) {
        if let Some(rows) = self.record_rows.get_mut(key) {
            rows.retain(|id| *id != row_id);
        }
        self.on_text_change(key);
    }

    pub(crate) fn toggle_flag(&mut self, key: &str, flag: &str) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let mut items: Vec<String> = self
            .editor
            .current_value(s)
            .as_array()
            .map(|a| a.iter().map(plain).collect())
            .unwrap_or_default();
        if items.iter().any(|x| x == flag) {
            items.retain(|x| x != flag);
        } else {
            items.push(flag.to_string());
        }
        let order = s.values.clone().unwrap_or_default();
        items.sort_by_key(|f| order.iter().position(|o| o == f).unwrap_or(usize::MAX));
        self.edit(key, serde_json::json!(items));
    }
}

fn decode_logo(bytes: &'static [u8], waker: Option<Waker>) -> Option<Image> {
    if bytes.is_empty() {
        return None;
    }
    #[cfg(target_arch = "wasm32")]
    let cache = ImageCache::new(waker?);
    #[cfg(not(target_arch = "wasm32"))]
    let cache = ImageCache::new(waker.unwrap_or_else(|| Arc::new(|| {})));
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    let img = if head.trim_start().starts_with('<') && head.contains("<svg") {
        cache.svg(bytes, 32.0, 2.0)
    } else {
        cache.load(bytes)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let _ = img.wait();
    Some(img)
}
