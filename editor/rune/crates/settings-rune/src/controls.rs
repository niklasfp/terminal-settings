//! One Rune control per setting, picked by the setting's `SettingType`.
//!
//! Values live in `Editor` (`model.rs`). Retained `TextEdit` and `ComboboxState` values live in
//! maps on `SettingsWindow` (`state.rs`) keyed by the setting key (or `"{key}:{slot}"` for
//! multi-field controls).

use crate::icons;
use crate::model::{format_number, plain, same};
use crate::state::SettingsWindow;
use rune::tok::motion;
use rune::{
    Color, Drag, El, FileDialogOptions, Key, KeyEvent, Phase, TextEdit, Tx, col, el, row, text,
};
use rune_kit::{
    Button, ButtonVariant, Checkbox, Combobox, ComboboxOption, ComboboxState, ControlSize,
    IconButton, IconButtonVariant, Kit, SegmentedControl, Select, State, Switch, TextArea,
    TextInput, glyphs,
};
use serde_json::{Value, json};
use settings_core::{Setting, SettingType, humanize};

const MAX_FLAG_CHECKBOXES: usize = 6;

/// Display label for an enum value, e.g. `"nativetab"` → `"Native tabs"`.
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

/// CSS-style margin: the shortest of `[t]`, `[t, r]` or `[t, r, b, l]`.
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
    let a: Vec<f64> = v
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    match a.as_slice() {
        [x] => [*x; 4],
        [t, r] => [*t, *r, *t, *r],
        [t, r, b, l, ..] => [*t, *r, *b, *l],
        _ => [0.0; 4],
    }
}

pub fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        json!(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

/// Rounds slider output to the step's precision so floating-point noise never reaches the file.
pub fn round_to_step(v: f64, step: f64) -> f64 {
    let decimals = format_number(step).split('.').nth(1).map_or(0, str::len).min(6) as i32;
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

pub fn parse_hex_color(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|v| f32::from(v) / 255.0);
    let (r, g, b, a) = match h.len() {
        3 => {
            let q = |i: usize| {
                u8::from_str_radix(h.get(i..i + 1)?, 16)
                    .ok()
                    .map(|v| f32::from(v * 17) / 255.0)
            };
            (q(0)?, q(1)?, q(2)?, 1.0)
        }
        6 => (p(0)?, p(2)?, p(4)?, 1.0),
        8 => (p(0)?, p(2)?, p(4)?, p(6)?),
        _ => return None,
    };
    let dec = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(Color { r: dec(r), g: dec(g), b: dec(b), a })
}

fn placeholder(s: &Setting) -> String {
    if let Some(p) = &s.placeholder {
        return p.clone();
    }
    match s.ty {
        SettingType::Theme => "No theme".into(),
        SettingType::Color if s.values.as_ref().is_some_and(|v| !v.is_empty()) => {
            "#rrggbb, name or keyword".into()
        }
        SettingType::Color => "#rrggbb".into(),
        SettingType::FontStyle => "default".into(),
        SettingType::Strings => "Add…".into(),
        SettingType::Fonts => "Add a font…".into(),
        SettingType::Font => "Font family".into(),
        SettingType::Int | SettingType::Float if s.nullable => "auto".into(),
        _ => String::new(),
    }
}

impl SettingsWindow {
    pub(crate) fn render_control(
        &self,
        s: &'static Setting,
        disabled: bool,
        k: &Kit,
        layer: &mut Vec<El>,
    ) -> El {
        let v = self.editor.current_value(s);
        let key = s.key.clone();
        let segmented = |options: Vec<(Value, String)>| -> El {
            let labels: Vec<String> = options.iter().map(|(_, l)| l.clone()).collect();
            let selected = options.iter().position(|(o, _)| same(o, &v));
            let values: Vec<Value> = options.into_iter().map(|(o, _)| o).collect();
            let mut ctl = SegmentedControl::new(&labels, selected).label(&s.label).fit();
            if disabled {
                for i in 0..labels.len() {
                    ctl = ctl.segment_state(i, State::Disabled);
                }
            } else {
                let key = key.clone();
                ctl = ctl.on_pick(move |app: &mut SettingsWindow, i, _| {
                    if let Some(val) = values.get(i) {
                        app.set_and_sync(&key, val.clone());
                    }
                });
            }
            ctl.build(k).key(format!("{}:seg", s.key))
        };

        match s.ty {
            SettingType::Bool if s.nullable && s.default.is_null() => {
                segmented(vec![(json!(true), "On".into()), (json!(false), "Off".into())])
            }
            SettingType::Bool => {
                let checked = v.as_bool().unwrap_or(false);
                Switch::new(checked)
                    .label(&s.label)
                    .hide_label()
                    .disabled(disabled)
                    .on_toggle(move |app: &mut SettingsWindow| {
                        app.edit(&key, json!(!checked));
                    })
                    .build(k)
                    .key(format!("{}:switch", s.key))
            }
            SettingType::Blur => segmented(vec![
                (json!(false), "Off".into()),
                (json!(true), "Blur".into()),
                (json!("macos-glass-regular"), "Glass".into()),
                (json!("macos-glass-clear"), "Clear glass".into()),
            ]),
            SettingType::Enum => {
                let values = s.values.as_deref().unwrap_or_default();
                if is_segmented(values) {
                    segmented(
                        values
                            .iter()
                            .map(|x| (Value::String(x.clone()), value_label(x)))
                            .collect(),
                    )
                } else {
                    let labels: Vec<String> = values.iter().map(|x| value_label(x)).collect();
                    let current_label = v
                        .as_str()
                        .and_then(|cur| values.iter().position(|x| x == cur))
                        .map(|i| labels[i].as_str());
                    let select_id = format!("select:{}", s.key);
                    let open = self.open_select.as_deref() == Some(select_id.as_str());
                    let owned_values: Vec<String> = values.to_vec();
                    let owned_labels = labels.clone();
                    let (id_toggle, id_choose) = (select_id.clone(), select_id.clone());
                    Select::new(&select_id, &labels)
                        .label(&s.label)
                        .width(200.0)
                        .placeholder("—")
                        .current(current_label)
                        .open(open)
                        .disabled(disabled)
                        .on_toggle(move |app: &mut SettingsWindow| {
                            if app.open_select.as_deref() == Some(id_toggle.as_str()) {
                                app.open_select = None;
                            } else {
                                app.open_select = Some(id_toggle.clone());
                            }
                        })
                        .on_close(|app: &mut SettingsWindow| app.open_select = None)
                        .on_choose(move |app: &mut SettingsWindow, chosen: &str, _| {
                            app.open_select = None;
                            if let Some(pos) = owned_labels.iter().position(|l| l == chosen)
                                && let Some(raw) = owned_values.get(pos)
                            {
                                app.set_and_sync(&key, Value::String(raw.clone()));
                            }
                        })
                        .build(k, layer)
                        .key(id_choose)
                }
            }
            SettingType::Flags => self.render_flags_control(s, &v, disabled, k, layer),
            SettingType::Theme if !self.theme_options.is_empty() => {
                self.render_theme_control(s, &v, disabled, k, layer)
            }
            SettingType::Int | SettingType::Float => self.render_number_control(s, &v, disabled, k),
            SettingType::Color => self.render_color_control(s, &v, disabled, k),
            SettingType::Strings | SettingType::Fonts => {
                self.render_chips_control(s, &v, disabled, k)
            }
            SettingType::Margin | SettingType::Pair => self.render_multi_control(s, disabled, k),
            SettingType::Records => self.render_records_control(s, disabled, k),
            SettingType::Raw => {
                let empty = TextEdit::default();
                let edit = self.edits.get(&s.key).unwrap_or(&empty);
                let (k_edit, k_change) = (s.key.clone(), s.key.clone());
                col().w_full().child(
                    TextArea::new(edit)
                        .label(&s.label)
                        .placeholder(placeholder(s))
                        .rows(5)
                        .auto_grow(3, 14)
                        .disabled(disabled)
                        .on_edit(move |app: &mut SettingsWindow| {
                            app.edits.entry(k_edit.clone()).or_default()
                        })
                        .on_change(move |app: &mut SettingsWindow, _| {
                            app.on_text_change(&k_change);
                        })
                        .build(k)
                        .key(format!("{}:raw", s.key)),
                )
            }
            SettingType::String
            | SettingType::Path
            | SettingType::Font
            | SettingType::Theme
            | SettingType::FontStyle => self.render_text_control(s, &v, disabled, k),
        }
    }

    fn render_text_control(&self, s: &'static Setting, v: &Value, disabled: bool, k: &Kit) -> El {
        let empty = TextEdit::default();
        let edit = self.edits.get(&s.key).unwrap_or(&empty);
        let style_off = s.ty == SettingType::FontStyle && *v == Value::Bool(false);
        let width = if s.compact { 128.0 } else { 240.0 };
        let (k_edit, k_change) = (s.key.clone(), s.key.clone());
        let mut r = row().gap(8.0).center_y().child(
            TextInput::new(edit)
                .label(&s.label)
                .placeholder(placeholder(s))
                .width(Some(width))
                .disabled(disabled || style_off)
                .on_edit(move |app: &mut SettingsWindow| {
                    app.edits.entry(k_edit.clone()).or_default()
                })
                .on_change(move |app: &mut SettingsWindow, _| {
                    app.on_text_change(&k_change);
                })
                .build(k)
                .key(format!("{}:input", s.key)),
        );
        if s.ty == SettingType::FontStyle {
            let key = s.key.clone();
            r = r.child(
                Checkbox::new(style_off)
                    .label("Off")
                    .disabled(disabled)
                    .on_toggle(move |app: &mut SettingsWindow| {
                        let shown = app
                            .edits
                            .get(&key)
                            .map(|e| e.text.trim().to_string())
                            .unwrap_or_default();
                        let next = if !style_off {
                            json!(false)
                        } else if shown.is_empty() {
                            json!("default")
                        } else {
                            json!(shown)
                        };
                        app.edit(&key, next);
                    })
                    .build(k)
                    .key(format!("{}:off", s.key)),
            );
        }
        if (s.pick.is_some() || s.ty == SettingType::Path) && !disabled {
            let key = s.key.clone();
            let folder = s.pick.as_deref() == Some("folder");
            let title = format!("Choose {}", s.label.to_lowercase());
            r = r.child(
                IconButton::new(&glyphs::FOLDER_OPEN)
                    .variant(IconButtonVariant::Neutral)
                    .control_size(ControlSize::Regular)
                    .label("Browse…")
                    .help("Choose from disk…")
                    .on_press_tx(move |_: &mut SettingsWindow, tx: &mut Tx| {
                        let opts = FileDialogOptions { title: title.clone(), ..Default::default() };
                        let key = key.clone();
                        if folder {
                            tx.open_folder(opts, move |app: &mut SettingsWindow, res, _| {
                                if let Ok(Some(path)) = res {
                                    app.set_and_sync(&key, Value::String(path.display().to_string()));
                                }
                            });
                        } else {
                            tx.open_file(opts, move |app: &mut SettingsWindow, res, _| {
                                if let Ok(Some(path)) = res {
                                    app.set_and_sync(&key, Value::String(path.display().to_string()));
                                }
                            });
                        }
                    })
                    .build(k)
                    .key(format!("{}:browse", s.key)),
            );
        }
        r
    }

    fn render_number_control(&self, s: &'static Setting, v: &Value, disabled: bool, k: &Kit) -> El {
        let empty = TextEdit::default();
        let edit = self.edits.get(&s.key).unwrap_or(&empty);
        let (k_edit, k_change) = (s.key.clone(), s.key.clone());
        let mut input = TextInput::new(edit)
            .label(&s.label)
            .placeholder(placeholder(s))
            .width(Some(104.0))
            .disabled(disabled)
            .on_edit(move |app: &mut SettingsWindow| app.edits.entry(k_edit.clone()).or_default())
            .on_change(move |app: &mut SettingsWindow, _| app.on_text_change(&k_change));
        if let Some(unit) = &s.unit {
            input = input.trailing(text(unit.clone()).font(k.ty.small).color(k.c.text_muted));
        }
        let mut r = row().gap(10.0).center_y();
        if let (Some(min), Some(max)) = (s.min, s.max)
            && max > min
        {
            let step = s.step.unwrap_or(if s.ty == SettingType::Int {
                1.0
            } else {
                (max - min) / 100.0
            });
            let cur = v.as_f64().unwrap_or(min).clamp(min, max);
            r = r.child(self.render_slider(s, cur, min, max, step, disabled, k));
        }
        r.child(input.build(k).key(format!("{}:input", s.key)))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_slider(
        &self,
        s: &'static Setting,
        cur: f64,
        min: f64,
        max: f64,
        step: f64,
        disabled: bool,
        k: &Kit,
    ) -> El {
        let (w, h, knob_d, bar) = (136.0f32, 22.0f32, 16.0f32, 4.0f32);
        let travel = w - knob_d;
        let frac = (((cur - min) / (max - min)) as f32).clamp(0.0, 1.0);
        let snap = move |f: f32| -> f64 {
            let raw = min + f64::from(f.clamp(0.0, 1.0)) * (max - min);
            let stepped = min + ((raw - min) / step).round() * step;
            round_to_step(stepped.clamp(min, max), step)
        };
        let to_frac = move |x: f32| ((x - knob_d / 2.0) / travel).clamp(0.0, 1.0);
        let mut slider = el()
            .key(format!("{}:slider", s.key))
            .role("slider")
            .label(&s.label)
            .range_value(cur, min, max)
            .range_step(step)
            .size(w, h)
            .motion(motion::SNAPPY)
            .child(
                el().key("track")
                    .size(w, bar)
                    .absolute(0.0, (h - bar) / 2.0)
                    .radius(bar / 2.0)
                    .fill(k.c.border_base)
                    .a11y_hidden(),
            )
            .child(
                el().key("fill")
                    .size(knob_d / 2.0 + frac * travel, bar)
                    .absolute(0.0, (h - bar) / 2.0)
                    .radius(bar / 2.0)
                    .fill(k.c.bg_accent)
                    .a11y_hidden(),
            )
            .child(
                el().key("knob")
                    .size(knob_d, knob_d)
                    .absolute(frac * travel, (h - knob_d) / 2.0)
                    .radius(knob_d / 2.0)
                    .fill(k.c.knob)
                    .border(1.0, k.c.border_strong)
                    .a11y_hidden(),
            );
        if !disabled {
            let (key_step, key_drag) = (s.key.clone(), s.key.clone());
            slider = slider
                .focusable(true)
                .on_step(move |app: &mut SettingsWindow, steps, _| {
                    let next = round_to_step((cur + f64::from(steps) * step).clamp(min, max), step);
                    app.set_and_sync(&key_step, number(next));
                })
                .on_drag(move |app: &mut SettingsWindow, d: &mut Drag, tx| match d.phase {
                    Phase::Down => {
                        d.data = frac;
                        let knob_x = knob_d / 2.0 + frac * travel;
                        if (d.x - knob_x).abs() > knob_d / 2.0 + 2.0 {
                            let next = snap(to_frac(d.x));
                            app.set_and_sync(&key_drag, number(next));
                        }
                    }
                    Phase::Move => {
                        let knob_x = knob_d / 2.0 + d.data * travel;
                        let grab = if (d.start_x - knob_x).abs() <= knob_d / 2.0 + 2.0 {
                            d.start_x - knob_x
                        } else {
                            0.0
                        };
                        let next = snap(to_frac(d.x - grab));
                        app.set_and_sync(&key_drag, number(next));
                        tx.instant();
                    }
                    Phase::Up => {
                        if d.cancelled() {
                            let prev = snap(d.data);
                            app.set_and_sync(&key_drag, number(prev));
                        }
                    }
                });
        }
        slider
    }

    fn render_color_control(&self, s: &'static Setting, v: &Value, disabled: bool, k: &Kit) -> El {
        let empty = TextEdit::default();
        let edit = self.edits.get(&s.key).unwrap_or(&empty);
        let swatch_color = v
            .as_str()
            .and_then(parse_hex_color)
            .unwrap_or(Color::TRANSPARENT);
        let (k_edit, k_change) = (s.key.clone(), s.key.clone());
        row()
            .gap(8.0)
            .center_y()
            .child(
                el().key(format!("{}:swatch", s.key))
                    .size(24.0, 24.0)
                    .radius(k.radius.sm)
                    .fill(swatch_color)
                    .border(1.0, k.c.border_strong)
                    .a11y_hidden(),
            )
            .child(
                TextInput::new(edit)
                    .label(&s.label)
                    .placeholder(placeholder(s))
                    .width(Some(132.0))
                    .disabled(disabled)
                    .on_edit(move |app: &mut SettingsWindow| {
                        app.edits.entry(k_edit.clone()).or_default()
                    })
                    .on_change(move |app: &mut SettingsWindow, _| {
                        app.on_text_change(&k_change);
                    })
                    .build(k)
                    .key(format!("{}:input", s.key)),
            )
    }

    fn render_theme_control(
        &self,
        s: &'static Setting,
        v: &Value,
        disabled: bool,
        k: &Kit,
        layer: &mut Vec<El>,
    ) -> El {
        let cur = v.as_str().filter(|t| !t.is_empty());
        let empty_combo = ComboboxState::default();
        let state = self.combos.get(&s.key).unwrap_or(&empty_combo);
        let (k_acc, k_choose, k_clear) = (s.key.clone(), s.key.clone(), s.key.clone());
        let mut r = row().gap(8.0).center_y();
        if let Some(name) = cur
            && let Some(info) = self.themes.iter().find(|t| t.name == name)
        {
            let mut dots = row().gap(2.0).center_y();
            for hex_str in info.ansi.iter().take(6).flatten() {
                if let Some(c) = parse_hex_color(hex_str) {
                    dots = dots.child(el().size(8.0, 14.0).radius(2.0).fill(c).a11y_hidden());
                }
            }
            r = r.child(dots);
        }
        r = r.child(
            Combobox::new(
                format!("combo:{}", s.key),
                state,
                &self.theme_options,
                move |app: &mut SettingsWindow| app.combos.entry(k_acc.clone()).or_default(),
            )
            .label(&s.label)
            .placeholder("No theme")
            .search_placeholder("Search themes…")
            .current(cur)
            .width(260.0)
            .disabled(disabled)
            .on_choose(move |app: &mut SettingsWindow, chosen: &str, _| {
                app.choose_theme(&k_choose, chosen);
            })
            .build(k, layer)
            .key(format!("{}:theme", s.key)),
        );
        if cur.is_some() && s.nullable && !disabled {
            r = r.child(
                IconButton::new(&glyphs::CLOSE)
                    .variant(IconButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .label("Clear theme")
                    .help("Clear theme")
                    .on_press(move |app: &mut SettingsWindow| {
                        app.set_and_sync(&k_clear, Value::Null);
                    })
                    .build(k)
                    .key(format!("{}:clear-theme", s.key)),
            );
        }
        r
    }

    fn render_flags_control(
        &self,
        s: &'static Setting,
        v: &Value,
        disabled: bool,
        k: &Kit,
        layer: &mut Vec<El>,
    ) -> El {
        let values = s.values.as_deref().unwrap_or_default();
        let on: Vec<String> = v.as_array().map(|a| a.iter().map(plain).collect()).unwrap_or_default();
        if values.len() <= MAX_FLAG_CHECKBOXES {
            let mut r = row().gap(12.0).flex_wrap().center_y();
            for flag in values {
                let checked = on.contains(flag);
                let (key, flag_name) = (s.key.clone(), flag.clone());
                r = r.child(
                    Checkbox::new(checked)
                        .label(flag.clone())
                        .disabled(disabled)
                        .on_toggle(move |app: &mut SettingsWindow| app.toggle_flag(&key, &flag_name))
                        .build(k)
                        .key(format!("{}:flag:{flag}", s.key)),
                );
            }
            return r;
        }
        let options: Vec<ComboboxOption> = values
            .iter()
            .map(|f| {
                let mut o = ComboboxOption::new(f.clone(), f.clone());
                if on.contains(f) {
                    o.detail = "✓".into();
                }
                o
            })
            .collect();
        let empty_combo = ComboboxState::default();
        let state = self.combos.get(&s.key).unwrap_or(&empty_combo);
        let (k_acc, k_choose) = (s.key.clone(), s.key.clone());
        let label = if on.is_empty() { "None".to_string() } else { on.join(", ") };
        Combobox::new(format!("flags:{}", s.key), state, &options, move |app: &mut SettingsWindow| {
            app.combos.entry(k_acc.clone()).or_default()
        })
        .label(&s.label)
        .placeholder(label)
        .search_placeholder("Filter flags…")
        .width(240.0)
        .disabled(disabled)
        .on_choose(move |app: &mut SettingsWindow, chosen: &str, _| app.toggle_flag(&k_choose, chosen))
        .build(k, layer)
        .key(format!("{}:flags", s.key))
    }

    fn render_chips_control(&self, s: &'static Setting, v: &Value, disabled: bool, k: &Kit) -> El {
        let items: Vec<String> = v.as_array().map(|a| a.iter().map(plain).collect()).unwrap_or_default();
        let draft_key = format!("{}:chip", s.key);
        let empty = TextEdit::default();
        let edit = self.edits.get(&draft_key).unwrap_or(&empty);
        let mut c = col().gap(6.0).items_end();
        if !items.is_empty() {
            let mut chips = row().gap(4.0).flex_wrap().justify_end().max_w(380.0);
            for (ix, item) in items.into_iter().enumerate() {
                let key = s.key.clone();
                let mut chip = row()
                    .key(format!("{}:chip-{ix}", s.key))
                    .gap(4.0)
                    .px(8.0)
                    .py(2.0)
                    .radius(k.radius.sm)
                    .fill(k.c.layer_02)
                    .border(1.0, k.c.border_base)
                    .center_y()
                    .child(text(item).font(k.ty.small).color(k.c.text_base));
                if !disabled {
                    chip = chip.child(
                        IconButton::new(&glyphs::CLOSE)
                            .variant(IconButtonVariant::GhostMuted)
                            .control_size(ControlSize::Mini)
                            .label("Remove")
                            .on_press(move |app: &mut SettingsWindow| app.remove_chip(&key, ix))
                            .build(k),
                    );
                }
                chips = chips.child(chip);
            }
            c = c.child(chips);
        }
        let (k_edit, k_submit, k_key) = (draft_key.clone(), s.key.clone(), s.key.clone());
        let comma_splits = self.app.list_comma_separates;
        c.child(
            TextInput::new(edit)
                .label(&s.label)
                .placeholder(placeholder(s))
                .width(Some(220.0))
                .disabled(disabled)
                .on_edit(move |app: &mut SettingsWindow| app.edits.entry(k_edit.clone()).or_default())
                .on_submit(move |app: &mut SettingsWindow, _| app.commit_chip_draft(&k_submit))
                .on_key(move |app: &mut SettingsWindow, ev: &KeyEvent, _| {
                    if comma_splits && matches!(&ev.key, Key::Text(t) if t == ",") {
                        app.commit_chip_draft(&k_key);
                        return true;
                    }
                    false
                })
                .build(k)
                .key(format!("{}:input", s.key)),
        )
    }

    fn render_multi_control(&self, s: &'static Setting, disabled: bool, k: &Kit) -> El {
        let labels: &[&str] = if s.ty == SettingType::Margin { &["T", "R", "B", "L"] } else { &["Fast", "Slow"] };
        let empty = TextEdit::default();
        let mut r = row().gap(6.0).center_y();
        for (i, label) in labels.iter().enumerate() {
            let slot_key = format!("{}:{i}", s.key);
            let edit = self.edits.get(&slot_key).unwrap_or(&empty);
            let (k_edit, k_change) = (slot_key.clone(), s.key.clone());
            let input = TextInput::new(edit)
                .label(format!("{} {label}", s.label))
                .width(Some(64.0))
                .disabled(disabled)
                .on_edit(move |app: &mut SettingsWindow| app.edits.entry(k_edit.clone()).or_default())
                .on_change(move |app: &mut SettingsWindow, _| app.on_text_change(&k_change))
                .build(k)
                .key(slot_key);
            r = r.child(row().gap(4.0).center_y().child(text((*label).to_string()).font(k.ty.small).color(k.c.text_muted)).child(input));
        }
        r
    }

    fn render_records_control(&self, s: &'static Setting, disabled: bool, k: &Kit) -> El {
        let fields = s.fields.as_deref().unwrap_or_default();
        let row_ids = self.record_rows.get(&s.key).cloned().unwrap_or_default();
        let empty = TextEdit::default();
        let mut list = col().gap(8.0).w_full();
        for (row_ix, row_id) in row_ids.into_iter().enumerate() {
            let mut line = row().gap(8.0).items_end().w_full();
            for f in fields {
                let cell_key = format!("{}:row:{row_id}:{}", s.key, f.key);
                let edit = self.edits.get(&cell_key).unwrap_or(&empty);
                let (k_edit, k_change) = (cell_key.clone(), s.key.clone());
                let input = TextInput::new(edit)
                    .label(format!("{} {}", s.label, f.label))
                    .placeholder(f.placeholder.clone().unwrap_or_default())
                    .width(None)
                    .disabled(disabled)
                    .on_edit(move |app: &mut SettingsWindow| app.edits.entry(k_edit.clone()).or_default())
                    .on_change(move |app: &mut SettingsWindow, _| app.on_text_change(&k_change))
                    .build(k)
                    .key(format!("{}:{row_ix}:{}", s.key, f.key));
                line = line.child(col().grow(1.0).min_w(0.0).gap(3.0).child(text(f.label.clone()).font(k.ty.small).color(k.c.text_muted)).child(input));
            }
            let key = s.key.clone();
            line = line.child(
                IconButton::new(&glyphs::CLOSE)
                    .variant(IconButtonVariant::GhostMuted)
                    .control_size(ControlSize::Regular)
                    .label("Remove")
                    .help("Remove")
                    .disabled(disabled)
                    .on_press(move |app: &mut SettingsWindow| app.remove_record(&key, row_id))
                    .build(k)
                    .key(format!("{}:rm-{row_id}", s.key)),
            );
            list = list.child(line);
        }
        let key = s.key.clone();
        list.child(
            row().child(
                Button::new("Add")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Regular)
                    .icon(&icons::PLUS)
                    .disabled(disabled)
                    .on_press(move |app: &mut SettingsWindow| app.add_record(&key))
                    .build(k)
                    .key(format!("{}:add", s.key)),
            ),
        )
    }
}
