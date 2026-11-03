//! One control per setting, picked by the setting's type. Each control keeps its retained widget
//! state (inputs, sliders, pickers); the value itself always lives in the `Editor` model.
//! `sync` pushes the model's value into the widgets; widget events go back through `on_input`.

use crate::dropdowns::{Chooser, FontItem, ThemeItem, names};
use crate::paths::PathKind;
use crate::model::{plain, same};
use settings_core::{format_number, humanize};
use std::rc::Rc;
use crate::view::SettingsWindow;
use gpui_kit::component::{
    ActiveTheme as _, Colorize as _, Disableable as _, IconName, Sizable as _, h_flex, v_flex,
    button::{Button, ButtonGroup, ButtonVariants as _},
    checkbox::Checkbox,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    combobox::ComboboxEvent,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    select::{Select, SelectEvent, SelectState},
    slider::{Slider, SliderEvent, SliderState},
    switch::Switch,
    tag::Tag,
};
use gpui_kit::*;
use gpui_kit::base::Selectable as _;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::{Map, Value, json};
use settings_core::{Setting, SettingType};

/// Display label for an enum value, e.g. "nativetab" → "Native tabs".
pub fn value_label(v: &str) -> String {
    let known = match v {
        "nativetab" => "Native tabs",
        "tab" => "Tabs",
        "plain" => "Plain",
        "srgb" => "sRGB",
        "display-p3" => "Display P3",
        "rec2020" => "Rec. 2020",
        "donotround" => "Square",
        "roundsmall" => "Small",
        "round" => "Round",
        "default" => "Default",
        "webgpu" => "WebGPU",
        "metal" => "Metal",
        "vulkan" => "Vulkan",
        "false" => "Off",
        "true" => "On",
        "OFF" => "Off",
        "ERROR" => "Error",
        "WARN" => "Warn",
        "INFO" => "Info",
        "DEBUG" => "Debug",
        "TRACE" => "Trace",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    humanize(v)
}

/// Enums with few, short values render as segmented buttons; the rest as a dropdown.
pub fn is_segmented(values: &[String]) -> bool {
    values.len() <= 5 && values.iter().map(|v| value_label(v).len()).sum::<usize>() <= 32
}

/// CSS-style margin: the shortest of [t], [t, r] or [t, r, b, l].
pub fn margin_compress(t: f64, r: f64, b: f64, l: f64) -> Vec<f64> {
    if t == r && r == b && b == l {
        vec![t]
    } else if t == b && r == l {
        vec![t, r]
    } else {
        vec![t, r, b, l]
    }
}

pub fn margin_expand(v: &Value) -> [f64; 4] {
    let a: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    match a.as_slice() {
        [x] => [*x; 4],
        [t, r] => [*t, *r, *t, *r],
        [t, r, b, l, ..] => [*t, *r, *b, *l],
        _ => [0.0; 4],
    }
}

fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        json!(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

/// Rounds slider output to the step's precision (0.1 + 0.2 style noise otherwise reaches the file).
fn round_to_step(v: f64, step: f64) -> f64 {
    let decimals = format_number(step).split('.').nth(1).map_or(0, str::len).min(6) as i32;
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

pub fn parse_hex(s: &str) -> Option<Hsla> {
    let s = s.trim();
    if !crate::model::is_hex_color(s) {
        return None;
    }
    Hsla::parse_hex(s).ok()
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque. Rounds each channel, so a hex value survives the
/// round trip through HSL (Colorize::to_hex truncates and turns #112233 into #112232).
pub fn to_hex(c: Hsla) -> String {
    let rgba = Rgba::from(c);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b, a) = (byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a));
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// One column of an entry in a table setting.
pub enum Cell {
    Text(Entity<InputState>),
    Font(Chooser<FontItem>),
}

impl Cell {
    fn text(&self, cx: &App) -> String {
        match self {
            Cell::Text(input) => input.read(cx).value().to_string(),
            Cell::Font(chooser) => chooser.selected(cx).first().map(ToString::to_string).unwrap_or_default(),
        }
    }
}

pub struct RecordRow {
    pub id: u64,
    pub cells: Vec<Cell>,
    /// Keys the entry has in the file that the schema has no field for (e.g. a binding's `bytes`).
    extra: Map<String, Value>,
    _subs: Vec<Subscription>,
}

impl RecordRow {
    /// The entry as the model sees it: the extra keys plus every non-empty field.
    fn value(&self, fields: &[settings_core::Field], cx: &App) -> Value {
        let mut obj = self.extra.clone();
        for (f, cell) in fields.iter().zip(&self.cells) {
            let v = cell.text(cx);
            if !v.is_empty() {
                obj.insert(f.key.clone(), Value::String(v));
            }
        }
        Value::Object(obj)
    }
}

/// A setting's dropdown at the shared width. (Select and Combobox fill their parent, so the box
/// sets their size; without it they take the whole row and squeeze the label.)
fn choice(id: ElementId, dropdown: impl IntoElement) -> AnyElement {
    div().id(id).test_support().w_80().flex_none().child(dropdown).into_any_element()
}

/// Flag settings with more options than this are a multi-select dropdown instead of checkboxes.
const MAX_FLAG_CHECKBOXES: usize = 4;

pub enum Control {
    /// Rendered straight from the model: switches, segmented choices, flags.
    Plain,
    /// A font, or a list of fonts (in fallback order).
    Fonts(Chooser<FontItem>),
    /// A theme.
    Theme(Chooser<ThemeItem>),
    /// Flags with many options: the flags in the schema's order, and a multi-select dropdown.
    Flags(Rc<Vec<SharedString>>, Chooser<SharedString>),
    Select(Entity<SelectState<Vec<SharedString>>>),
    Text(Entity<InputState>),
    Number { input: Entity<InputState>, slider: Option<Entity<SliderState>> },
    Color { input: Entity<InputState>, picker: Entity<ColorPickerState> },
    Chips(Entity<InputState>),
    Multi(Vec<Entity<InputState>>),
    Records(Vec<RecordRow>),
    Raw(Entity<TextareaState>),
}

fn placeholder(s: &Setting) -> String {
    s.placeholder.clone().unwrap_or_else(|| match s.ty {
        SettingType::Theme => "No theme".into(),
        SettingType::Color if s.values.as_ref().is_some_and(|v| !v.is_empty()) => "#rrggbb, name or keyword".into(),
        SettingType::Color => "#rrggbb".into(),
        SettingType::FontStyle => "default".into(),
        SettingType::Strings => "Add…".into(),
        SettingType::Fonts => "Choose fonts…".into(),
        SettingType::Font => "Choose a font…".into(),
        SettingType::Int | SettingType::Float if s.nullable => "auto".into(),
        _ if s.nullable => "Not set".into(),
        _ => String::new(),
    })
}

impl SettingsWindow {
    /// Creates the control for a setting and subscribes to its widget events.
    pub(crate) fn create_control(&mut self, s: &'static Setting, window: &mut Window, cx: &mut Context<Self>) -> Control {
        let key = s.key.clone();
        let text_input = |window: &mut Window, cx: &mut Context<Self>| {
            let ph = placeholder(s);
            cx.new(|cx| InputState::new(window, cx).placeholder(ph))
        };
        match s.ty {
            SettingType::Flags if s.values.as_ref().is_some_and(|v| v.len() > MAX_FLAG_CHECKBOXES) => {
                let order: Rc<Vec<SharedString>> = Rc::new(s.values.iter().flatten().map(SharedString::from).collect());
                let chooser = Chooser::new(true, false, window, cx);
                let flags = order.clone();
                let sub = cx.subscribe_in(&chooser.state, window, move |this, _, ev: &ComboboxEvent<Vec<SharedString>>, window, cx| {
                    let ComboboxEvent::Change(on) = ev else { return };
                    // The file lists flags in the schema's order, whatever order they were ticked in.
                    let on: Vec<Value> = flags.iter().filter(|f| on.contains(f)).map(|f| Value::String(f.to_string())).collect();
                    this.edit(&key, Value::Array(on), window, cx);
                });
                self.subs.push(sub);
                Control::Flags(order, chooser)
            }
            SettingType::Theme => {
                let chooser = Chooser::new(false, true, window, cx);
                let sub = cx.subscribe_in(&chooser.state, window, move |this, _, ev: &ComboboxEvent<Vec<ThemeItem>>, window, cx| {
                    let ComboboxEvent::Change(names) = ev else { return };
                    this.choose_theme(&key, names.first().map(ToString::to_string), window, cx);
                });
                self.subs.push(sub);
                Control::Theme(chooser)
            }
            SettingType::Bool | SettingType::Blur | SettingType::Flags => Control::Plain,
            SettingType::Enum => {
                let values = s.values.clone().unwrap_or_default();
                if is_segmented(&values) {
                    return Control::Plain;
                }
                let items: Vec<SharedString> = values.into_iter().map(SharedString::from).collect();
                let state = cx.new(|cx| SelectState::new(items, None, window, cx));
                let sub = cx.subscribe_in(&state, window, move |this, _, ev: &SelectEvent<Vec<SharedString>>, window, cx| {
                    let SelectEvent::Confirm(Some(v)) = ev else { return };
                    this.edit(&key, Value::String(v.to_string()), window, cx);
                });
                self.subs.push(sub);
                Control::Select(state)
            }
            SettingType::Font | SettingType::Fonts => {
                let multiple = s.ty == SettingType::Fonts;
                // Items arrive with the first sync, which also adds the value's fonts that aren't installed.
                let chooser = Chooser::new(multiple, true, window, cx);
                let sub = cx.subscribe_in(&chooser.state, window, move |this, _, ev: &ComboboxEvent<Vec<FontItem>>, window, cx| {
                    let ComboboxEvent::Change(fonts) = ev else { return };
                    let mut fonts = fonts.iter().map(|f| Value::String(f.to_string()));
                    let value = if multiple { Value::Array(fonts.collect()) } else { fonts.next().unwrap_or(Value::Null) };
                    this.edit(&key, value, window, cx);
                });
                self.subs.push(sub);
                Control::Fonts(chooser)
            }
            SettingType::String | SettingType::Path | SettingType::FontStyle => {
                let input = text_input(window, cx);
                self.watch(&input, key, window, cx);
                Control::Text(input)
            }
            SettingType::Int | SettingType::Float => {
                let input = text_input(window, cx);
                self.watch(&input, key.clone(), window, cx);
                let slider = match (s.min, s.max) {
                    (Some(min), Some(max)) => {
                        let step = s.step.unwrap_or(if s.ty == SettingType::Int { 1.0 } else { (max - min) / 100.0 });
                        let slider = cx.new(|_| {
                            // Each setter clamps against the other bound (default 0..100), so widen before narrowing.
                            SliderState::new().max(max.max(0.0) as f32).min(min as f32).max(max as f32).step(step as f32).default_value(min as f32)
                        });
                        let input = input.clone();
                        let key = key.clone();
                        let sub = cx.subscribe_in(&slider, window, move |this, _, ev: &SliderEvent, window, cx| {
                            let SliderEvent::Change(v) = ev else { return };
                            let v = round_to_step(v.start() as f64, step);
                            // set_value emits no change event, so record the edit here.
                            input.update(cx, |state, cx| state.set_value(format_number(v), window, cx));
                            this.on_input(&key, window, cx);
                        });
                        self.subs.push(sub);
                        Some(slider)
                    }
                    _ => None,
                };
                Control::Number { input, slider }
            }
            SettingType::Color => {
                let input = text_input(window, cx);
                self.watch(&input, key.clone(), window, cx);
                let picker = cx.new(|cx| ColorPickerState::new(window, cx));
                let hex_input = input.clone();
                let sub = cx.subscribe_in(&picker, window, move |this, _, ev: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(Some(c)) = ev else { return };
                    let Some(setting) = this.editor.schema.find(&key) else { return };
                    // Re-picking the current color must not turn #1E1E2E into a "change" to #1e1e2e.
                    if let Value::String(cur) = this.editor.current_value(setting)
                        && parse_hex(&cur).map(to_hex) == Some(to_hex(*c))
                    {
                        return;
                    }
                    let hex = to_hex(*c);
                    // set_value emits no change event, so record the edit here.
                    hex_input.update(cx, |state, cx| state.set_value(hex, window, cx));
                    this.on_input(&key, window, cx);
                });
                self.subs.push(sub);
                Control::Color { input, picker }
            }
            SettingType::Strings => {
                let input = text_input(window, cx);
                let comma_commits = self.app.list_comma_separates;
                let sub = cx.subscribe_in(&input, window, move |this, state, ev: &InputEvent, window, cx| {
                    let raw = state.read(cx).value().to_string();
                    let (entries, rest): (Vec<&str>, &str) = match ev {
                        InputEvent::PressEnter { .. } | InputEvent::Blur => (vec![raw.as_str()], ""),
                        // A comma ends an entry (typed or pasted), except in NAME=value entries where it
                        // may be part of the value.
                        InputEvent::Change if comma_commits && raw.contains(',') && !raw.contains('=') => {
                            let mut parts: Vec<&str> = raw.split(',').collect();
                            let rest = parts.pop().unwrap_or("");
                            (parts, rest)
                        }
                        _ => return,
                    };
                    let entries: Vec<String> = entries.iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
                    if entries.is_empty() && rest == raw {
                        return;
                    }
                    let rest = rest.to_string();
                    state.update(cx, |state, cx| state.set_value(rest, window, cx));
                    for entry in entries {
                        this.add_chip(&key, entry, window, cx);
                    }
                });
                self.subs.push(sub);
                Control::Chips(input)
            }
            SettingType::Margin | SettingType::Pair => {
                let n = if s.ty == SettingType::Margin { 4 } else { 2 };
                let inputs: Vec<_> = (0..n).map(|_| cx.new(|cx| InputState::new(window, cx).placeholder("0"))).collect();
                for input in &inputs {
                    self.watch(input, key.clone(), window, cx);
                }
                Control::Multi(inputs)
            }
            SettingType::Records => Control::Records(Vec::new()),
            SettingType::Raw => {
                let area = cx.new(|cx| TextareaState::new(window, cx).rows(8).placeholder("[[hints.rules]]"));
                let sub = cx.subscribe_in(&area, window, move |this, _, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Change) {
                        this.on_input(&key, window, cx);
                    }
                });
                self.subs.push(sub);
                Control::Raw(area)
            }
        }
    }

    fn watch(&mut self, input: &Entity<InputState>, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let sub = cx.subscribe_in(input, window, move |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::Change) {
                this.on_input(&key, window, cx);
            }
        });
        self.subs.push(sub);
    }

    /// A widget changed: read the control's value and record it.
    pub(crate) fn on_input(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let Some(control) = self.controls.get(key) else { return };
        let text = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let value = match control {
            Control::Text(input) => {
                let t = text(input);
                if s.ty == SettingType::FontStyle {
                    if self.editor.current_value(s) == Value::Bool(false) {
                        return;
                    }
                    let t = t.trim();
                    Value::String(if t.is_empty() { "default".into() } else { t.into() })
                } else if t.is_empty() {
                    Value::Null
                } else {
                    Value::String(t)
                }
            }
            Control::Number { input, slider } => {
                let t = text(input);
                let t = t.trim();
                let value = if t.is_empty() {
                    Value::Null
                } else {
                    match t.parse::<f64>() {
                        Ok(n) if n.is_finite() => number(n),
                        // Half-typed numbers ("1.", "-") wait for the next keystroke.
                        _ => return,
                    }
                };
                if let (Some(slider), Some(n)) = (slider, value.as_f64()) {
                    slider.update(cx, |state, cx| state.set_value(n as f32, window, cx));
                }
                value
            }
            Control::Color { input, picker } => {
                let t = text(input).trim().to_string();
                if let Some(c) = parse_hex(&t) {
                    picker.update(cx, |state, cx| state.set_value(c, window, cx));
                }
                if t.is_empty() { Value::Null } else { Value::String(t) }
            }
            Control::Multi(inputs) => {
                let nums: Vec<f64> = inputs.iter().map(|i| text(i).trim().parse::<f64>().unwrap_or(0.0)).collect();
                let list = if s.ty == SettingType::Margin {
                    margin_compress(nums[0], nums[1], nums[2], nums[3])
                } else {
                    nums
                };
                Value::Array(list.into_iter().map(number).collect())
            }
            Control::Records(rows) => {
                let fields = s.fields.as_deref().unwrap_or_default();
                Value::Array(rows.iter().map(|row| row.value(fields, cx)).collect())
            }
            Control::Raw(area) => {
                let t = area.read(cx).value().to_string();
                if t.trim().is_empty() { Value::Null } else { Value::String(t) }
            }
            Control::Plain | Control::Select(_) | Control::Chips(_) | Control::Fonts(_) | Control::Theme(_) | Control::Flags(..) => return,
        };
        self.edit(key, value, window, cx);
    }

    /// Commits list entries still being typed (as Enter would), so a save includes them.
    pub(crate) fn commit_pending_entries(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pending: Vec<(String, Entity<InputState>, String)> = self
            .controls
            .iter()
            .filter_map(|(key, control)| match control {
                Control::Chips(input) => {
                    let text = input.read(cx).value().trim().to_string();
                    (!text.is_empty()).then(|| (key.clone(), input.clone(), text))
                }
                _ => None,
            })
            .collect();
        for (key, input, text) in pending {
            input.update(cx, |state, cx| state.set_value("", window, cx));
            self.add_chip(&key, text, window, cx);
        }
    }

    pub(crate) fn add_chip(&mut self, key: &str, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let mut items = self.editor.current_value(s).as_array().cloned().unwrap_or_default();
        items.push(Value::String(text));
        self.edit(key, Value::Array(items), window, cx);
    }

    fn remove_chip(&mut self, key: &str, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let mut items = self.editor.current_value(s).as_array().cloned().unwrap_or_default();
        if ix < items.len() {
            items.remove(ix);
        }
        self.edit(key, Value::Array(items), window, cx);
    }

    fn new_record_row(&mut self, key: &str, record: &Value, window: &mut Window, cx: &mut Context<Self>) -> RecordRow {
        let s = self.editor.schema.find(key).expect("records setting");
        self.next_row_id += 1;
        let mut cells = Vec::new();
        let mut subs = Vec::new();
        for f in s.fields.as_deref().unwrap_or_default() {
            let initial = record.get(&f.key).map(plain).unwrap_or_default();
            let key = key.to_string();
            if f.ty == SettingType::Font {
                let mut chooser: Chooser<FontItem> = Chooser::new(false, true, window, cx);
                chooser.sync(&self.fonts.clone(), &names(&Value::String(initial)), &[], window, cx);
                subs.push(cx.subscribe_in(&chooser.state, window, move |this, _, ev: &ComboboxEvent<Vec<FontItem>>, window, cx| {
                    if matches!(ev, ComboboxEvent::Change(_)) {
                        this.on_input(&key, window, cx);
                    }
                }));
                cells.push(Cell::Font(chooser));
                continue;
            }
            let ph = f.placeholder.clone().unwrap_or_default();
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(ph).default_value(initial));
            subs.push(cx.subscribe_in(&input, window, move |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.on_input(&key, window, cx);
                }
            }));
            cells.push(Cell::Text(input));
        }
        let known: Vec<&str> = s.fields.iter().flatten().map(|f| f.key.as_str()).collect();
        let extra = record
            .as_object()
            .map(|m| m.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        RecordRow { id: self.next_row_id, cells, extra, _subs: subs }
    }

    fn add_record(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.new_record_row(key, &json!({}), window, cx);
        let first = match row.cells.first() {
            Some(Cell::Text(input)) => Some(input.clone()),
            _ => None,
        };
        if let Some(Control::Records(rows)) = self.controls.get_mut(key) {
            rows.push(row);
        }
        self.on_input(key, window, cx);
        if let Some(first) = first {
            first.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    fn remove_record(&mut self, key: &str, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Control::Records(rows)) = self.controls.get_mut(key) {
            rows.retain(|r| r.id != id);
        }
        self.on_input(key, window, cx);
    }

    /// Pushes the model's current value into the setting's widgets.
    pub(crate) fn sync(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let v = self.editor.current_value(s);
        // Dropdowns also list the default and the file's own values when the list lacks them
        // (a font built into the terminal, a font from another machine).
        let (wanted, defaults) = (names(&v), names(&s.default));
        match self.controls.get_mut(key) {
            Some(Control::Fonts(chooser)) => return chooser.sync(&self.fonts, &wanted, &defaults, window, cx),
            Some(Control::Theme(chooser)) => return chooser.sync(&self.themes, &wanted, &defaults, window, cx),
            Some(Control::Flags(order, chooser)) => return chooser.sync(order, &wanted, &[], window, cx),
            _ => {}
        }
        let set_text =|input: &Entity<InputState>, text: String, window: &mut Window, cx: &mut Context<Self>| {
            if input.read(cx).value().as_ref() != text {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        };
        match self.controls.get(key) {
            Some(Control::Text(input)) => {
                let t = match (&v, s.ty) {
                    (Value::Bool(false), SettingType::FontStyle) => String::new(),
                    (Value::String(st), SettingType::FontStyle) if st == "default" => String::new(),
                    _ => plain(&v),
                };
                set_text(input, t, window, cx);
            }
            Some(Control::Number { input, slider }) => {
                let t = v.as_f64().map(format_number).unwrap_or_default();
                set_text(input, t, window, cx);
                if let Some(slider) = slider {
                    let n = v.as_f64().unwrap_or(s.min.unwrap_or(0.0));
                    slider.update(cx, |state, cx| state.set_value(n as f32, window, cx));
                }
            }
            Some(Control::Color { input, picker }) => {
                set_text(input, plain(&v), window, cx);
                let parsed = v.as_str().and_then(parse_hex);
                picker.update(cx, |state, cx| match parsed {
                    Some(c) => state.set_value(c, window, cx),
                    None => state.clear_value(window, cx),
                });
            }
            Some(Control::Multi(inputs)) => {
                let nums: Vec<f64> = if s.ty == SettingType::Margin {
                    margin_expand(&v).to_vec()
                } else {
                    v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
                };
                for (i, input) in inputs.clone().iter().enumerate() {
                    set_text(input, nums.get(i).copied().map(format_number).unwrap_or_default(), window, cx);
                }
            }
            Some(Control::Raw(area)) => {
                let t = plain(&v);
                if area.read(cx).value().as_ref() != t {
                    area.update(cx, |state, cx| state.set_value(t, window, cx));
                }
            }
            Some(Control::Select(select)) => {
                let value = v.as_str().map(SharedString::from);
                select.update(cx, |state, cx| match value {
                    Some(value) => state.set_selected_value(&value, window, cx),
                    None => state.set_selected_index(None, window, cx),
                });
            }
            Some(Control::Records(rows)) => {
                // Rebuild only when the rows no longer describe the value (e.g. after a reload).
                let fields = s.fields.as_deref().unwrap_or_default();
                let current: Vec<Value> = rows.iter().map(|row| row.value(fields, cx)).collect();
                let wanted = v.as_array().cloned().unwrap_or_default();
                let as_strings = |items: &[Value]| -> Vec<Value> {
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
                if !same(&Value::Array(as_strings(&current)), &Value::Array(as_strings(&wanted))) {
                    let rows: Vec<RecordRow> = wanted.iter().map(|r| self.new_record_row(key, r, window, cx)).collect();
                    self.controls.insert(key.to_string(), Control::Records(rows));
                }
            }
            Some(Control::Plain | Control::Chips(_) | Control::Fonts(_) | Control::Theme(_) | Control::Flags(..)) | None => {}
        }
    }

    /// The setting's widget, without the label column.
    pub(crate) fn render_control(&self, s: &'static Setting, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let v = self.editor.current_value(s);
        let key = s.key.clone();
        let id = |part: &str| ElementId::Name(format!("{}:{part}", s.key).into());
        let segmented = |options: Vec<(Value, String)>, cx: &mut Context<Self>| {
            let selected: Vec<bool> = options.iter().map(|(o, _)| same(o, &v)).collect();
            let values: Vec<Value> = options.iter().map(|(o, _)| o.clone()).collect();
            let key = key.clone();
            ButtonGroup::new(id("seg"))
                .small()
                .children(
                    options
                        .into_iter()
                        .enumerate()
                        .map(|(i, (_, label))| Button::new(i).label(label).selected(selected[i]).disabled(disabled)),
                )
                .on_click(cx.listener(move |this, ix: &Vec<usize>, window, cx| {
                    if let Some(v) = ix.first().and_then(|i| values.get(*i)) {
                        this.edit(&key, v.clone(), window, cx);
                    }
                }))
                .into_any_element()
        };

        match (s.ty, self.controls.get(&s.key)) {
            (SettingType::Bool, _) if s.nullable && s.default.is_null() => {
                segmented(vec![(json!(true), "On".into()), (json!(false), "Off".into())], cx)
            }
            (SettingType::Bool, _) => Switch::new(id("switch"))
                .checked(v.as_bool().unwrap_or(false))
                .disabled(disabled)
                .on_change(cx.listener(move |this, checked: &bool, window, cx| this.edit(&key, json!(*checked), window, cx)))
                .into_any_element(),
            (SettingType::Blur, _) => segmented(
                vec![
                    (json!(false), "Off".into()),
                    (json!(true), "Blur".into()),
                    (json!("macos-glass-regular"), "Glass".into()),
                    (json!("macos-glass-clear"), "Clear glass".into()),
                ],
                cx,
            ),
            (SettingType::Enum, Some(Control::Select(state))) => {
                choice(id("select"), Select::new(state).small().placeholder("—").disabled(disabled))
            }
            (SettingType::Enum, _) => segmented(
                s.values.iter().flatten().map(|x| (Value::String(x.clone()), value_label(x))).collect(),
                cx,
            ),
            (SettingType::Flags, Some(Control::Flags(_, chooser))) => {
                choice(id("flags"), chooser.render("None", "", false, disabled))
            }
            (SettingType::Theme, Some(Control::Theme(chooser))) => {
                choice(id("theme"), chooser.render("No theme", "Search themes", s.nullable, disabled))
            }
            (SettingType::Flags, _) => {
                let on: Vec<String> = v.as_array().map(|a| a.iter().map(plain).collect()).unwrap_or_default();
                h_flex()
                    .gap_3()
                    .children(s.values.iter().flatten().map(|flag| {
                        let key = key.clone();
                        let flag = flag.clone();
                        let checked = on.contains(&flag);
                        Checkbox::new(id(&flag))
                            .label(flag.clone())
                            .checked(checked)
                            .disabled(disabled)
                            .on_change(cx.listener(move |this, next: &bool, window, cx| {
                                let Some(s) = this.editor.schema.find(&key) else { return };
                                let mut items: Vec<String> = this
                                    .editor
                                    .current_value(s)
                                    .as_array()
                                    .map(|a| a.iter().map(plain).collect())
                                    .unwrap_or_default();
                                items.retain(|f| f != &flag);
                                if *next {
                                    items.push(flag.clone());
                                }
                                // Keep the schema's order.
                                let order = s.values.clone().unwrap_or_default();
                                items.sort_by_key(|f| order.iter().position(|o| o == f).unwrap_or(usize::MAX));
                                this.edit(&key, json!(items), window, cx);
                            }))
                    }))
                    .into_any_element()
            }
            (_, Some(Control::Text(input))) if PathKind::of(s).is_some() => {
                let kind = PathKind::of(s).expect("checked");
                let text = input.read(cx).value().to_string();
                let suffix = self.path_suffix(s, kind, &text, disabled, cx);
                Input::new(input)
                    .id(id("input"))
                    .small()
                    .w_80()
                    .prefix(Self::path_prefix(kind, cx))
                    .suffix(suffix)
                    .disabled(disabled)
                    .into_any_element()
            }
            (_, Some(Control::Text(input))) => {
                let mut row = h_flex().gap_2().child(
                    Input::new(input).id(id("input")).small().map(|i| if s.compact { i.w_32() } else { i.w_64() }).disabled(disabled || v == Value::Bool(false) && s.ty == SettingType::FontStyle),
                );
                if s.ty == SettingType::FontStyle {
                    row = row.child(
                        Checkbox::new(id("off"))
                            .label("Off")
                            .tooltip("Never use a face for this style")
                            .checked(v == Value::Bool(false))
                            .disabled(disabled)
                            .on_change(cx.listener(move |this, off: &bool, window, cx| {
                                // Turning a face back on restores whatever the (disabled) field still shows.
                                let shown = match this.controls.get(&key) {
                                    Some(Control::Text(input)) => input.read(cx).value().trim().to_string(),
                                    _ => String::new(),
                                };
                                let value = if *off {
                                    json!(false)
                                } else if shown.is_empty() {
                                    json!("default")
                                } else {
                                    json!(shown)
                                };
                                this.edit(&key, value, window, cx);
                            })),
                    );
                }
                row.into_any_element()
            }
            (_, Some(Control::Number { input, slider })) => {
                let mut field = Input::new(input).id(id("input")).small().w_32().disabled(disabled);
                if let Some(unit) = &s.unit {
                    field = field.suffix(div().text_xs().text_color(cx.theme().muted_foreground).child(unit.clone()));
                }
                h_flex()
                    .gap_3()
                    .when_some(slider.as_ref(), |row, slider| row.child(div().id(id("slider")).test_support().w_40().child(Slider::new(slider).disabled(disabled))))
                    .child(field)
                    .into_any_element()
            }
            (_, Some(Control::Color { input, picker })) => h_flex()
                .gap_2()
                .child(div().id(id("picker")).test_support().child(ColorPicker::new(picker).small()))
                .child(Input::new(input).id(id("input")).small().w_32().disabled(disabled))
                .into_any_element(),
            (_, Some(Control::Chips(input))) => {
                let items: Vec<String> = v.as_array().map(|a| a.iter().map(plain).collect()).unwrap_or_default();
                v_flex()
                    .gap_2()
                    .items_end()
                    .when(!items.is_empty(), |col| {
                        col.child(h_flex().gap_1().flex_wrap().justify_end().max_w_96().children(items.into_iter().enumerate().map(
                            |(ix, item)| {
                                let key = key.clone();
                                Tag::secondary().small().child(
                                    h_flex().gap_1().child(item).child(
                                        Button::new(id(&format!("chip-{ix}")))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Close)
                                            .tooltip("Remove")
                                            .disabled(disabled)
                                            .on_click(cx.listener(move |this, _, window, cx| this.remove_chip(&key, ix, window, cx))),
                                    ),
                                )
                            },
                        )))
                    })
                    .child(
                        Input::new(input)
                            .id(id("input"))
                            .small()
                            .w_64()
                            .when_some(PathKind::of(s), |field, kind| {
                                // A list of paths: the field adds entries, so it only offers "Choose…".
                                field.prefix(Self::path_prefix(kind, cx)).suffix(self.path_suffix(s, kind, "", disabled, cx))
                            })
                            .disabled(disabled),
                    )
                    .into_any_element()
            }
            (_, Some(Control::Fonts(chooser))) => {
                let dropdown = chooser.render(placeholder(s), "Search fonts", s.ty == SettingType::Fonts || s.nullable, disabled);
                choice(id("fonts"), dropdown)
            }
            (_, Some(Control::Multi(inputs))) => {
                let labels: &[&str] = if s.ty == SettingType::Margin { &["T", "R", "B", "L"] } else { &["Fast", "Slow"] };
                h_flex()
                    .gap_1()
                    .children(inputs.iter().zip(labels).map(|(input, label)| {
                        Input::new(input)
                            .small()
                            .w_20()
                            .disabled(disabled)
                            .prefix(div().text_xs().text_color(cx.theme().muted_foreground).child(*label))
                    }))
                    .into_any_element()
            }
            (_, Some(Control::Records(rows))) => {
                let fields = s.fields.clone().unwrap_or_default();
                v_flex()
                    .gap_2()
                    .w_full()
                    .children(rows.iter().enumerate().map(|(row_ix, row)| {
                        let key = key.clone();
                        let row_id = row.id;
                        h_flex()
                            .gap_2()
                            .items_end()
                            .children(fields.iter().zip(&row.cells).map(|(f, cell)| {
                                let cell_id = id(&format!("{row_ix}:{}", f.key));
                                let widget = match cell {
                                    Cell::Text(input) => Input::new(input).id(cell_id).small().disabled(disabled).into_any_element(),
                                    Cell::Font(chooser) => div()
                                        .id(cell_id)
                                        .test_support()
                                        .w_full()
                                        .child(chooser.render("Choose a font…", "Search fonts", false, disabled))
                                        .into_any_element(),
                                };
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(f.label.clone()))
                                    .child(widget)
                            }))
                            .child(
                                Button::new(id(&format!("rm-{row_id}")))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .tooltip("Remove")
                                    .disabled(disabled)
                                    .on_click(cx.listener(move |this, _, window, cx| this.remove_record(&key, row_id, window, cx))),
                            )
                    }))
                    .child(
                        h_flex().child(
                            Button::new(id("add"))
                                .small()
                                .outline()
                                .icon(gpui_kit::assets::IconName::Plus)
                                .label("Add")
                                .disabled(disabled)
                                .on_click(cx.listener(move |this, _, window, cx| this.add_record(&key, window, cx))),
                        ),
                    )
                    .into_any_element()
            }
            (_, Some(Control::Raw(area))) => {
                div().w_full().font_family(cx.theme().mono_font_family.clone()).child(Textarea::new(area).disabled(disabled)).into_any_element()
            }
            _ => div().into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in GPUI's `test` attribute, which shadows the standard one.
    use super::{humanize, is_segmented, margin_compress, margin_expand, parse_hex, round_to_step, to_hex, value_label};
    use serde_json::json;

    #[test]
    fn margins_compress_css_style() {
        assert_eq!(margin_compress(2.0, 2.0, 2.0, 2.0), vec![2.0]);
        assert_eq!(margin_compress(1.0, 2.0, 1.0, 2.0), vec![1.0, 2.0]);
        assert_eq!(margin_compress(1.0, 2.0, 3.0, 4.0), vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(margin_expand(&json!([5])), [5.0; 4]);
        assert_eq!(margin_expand(&json!([1, 2])), [1.0, 2.0, 1.0, 2.0]);
    }

    #[test]
    fn labels_and_layout() {
        assert_eq!(value_label("nativetab"), "Native tabs");
        assert_eq!(value_label("windowed"), "Windowed");
        assert_eq!(humanize("light-black"), "Light black");
        assert!(is_segmented(&["maximized".into(), "fullscreen".into(), "windowed".into()]));
        assert_eq!(round_to_step(0.30000000000000004, 0.05), 0.3);
    }

    #[test]
    fn hex_round_trip() {
        let c = parse_hex("#1E1E2E").unwrap();
        assert_eq!(to_hex(c), "#1e1e2e");
        for hex in ["#112233", "#000000", "#ffffff", "#c026d3", "#1e1e2ecc", "#0f0d0e"] {
            assert_eq!(to_hex(parse_hex(hex).unwrap()), hex);
        }
        assert!(parse_hex("red").is_none());
    }
}
