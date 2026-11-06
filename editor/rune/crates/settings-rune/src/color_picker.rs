//! The color picker behind a Color setting's swatch, as GPUI Kit's: a popover with a palette of
//! preset swatches and hue, saturation, lightness and alpha sliders over gradient tracks. Rune Kit
//! has no picker of its own, so this one is drawn from plain elements. Values stay hex strings
//! (`#rrggbb`, or `#rrggbbaa` when not opaque); the hex field next to the swatch edits the same value.

use crate::state::SettingsWindow;
use rune::{Color, Drag, El, Key, KeyEvent, Phase, col, el, row, text};
use rune_kit::{Kit, Popover, SegmentedControl};
use serde_json::Value;
use settings_core::Setting;

/// A color as hue (degrees), saturation, lightness and alpha, each but hue in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsla {
    pub h: f32,
    pub s: f32,
    pub l: f32,
    pub a: f32,
}

impl Hsla {
    pub const BLACK: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };

    pub fn from_rgba([r, g, b, a]: [u8; 4]) -> Hsla {
        let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.0;
        let d = max - min;
        let (h, s) = if d == 0.0 {
            (0.0, 0.0)
        } else {
            let s = d / (1.0 - (2.0 * l - 1.0).abs());
            let h = if max == r {
                ((g - b) / d).rem_euclid(6.0)
            } else if max == g {
                (b - r) / d + 2.0
            } else {
                (r - g) / d + 4.0
            };
            (h * 60.0, s)
        };
        Hsla { h, s, l, a: f32::from(a) / 255.0 }
    }

    pub fn to_rgba(self) -> [u8; 4] {
        let c = (1.0 - (2.0 * self.l - 1.0).abs()) * self.s;
        let hp = self.h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.l - c / 2.0;
        let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(r), byte(g), byte(b), (self.a.clamp(0.0, 1.0) * 255.0).round() as u8]
    }

    pub fn hex(self) -> String {
        rgba_hex(self.to_rgba())
    }

    /// The color to draw (Rune colors are linear).
    pub fn color(self) -> Color {
        let [r, g, b, a] = self.to_rgba();
        let lin = |v: u8| rune::color::srgb_decode(f32::from(v) / 255.0);
        Color { r: lin(r), g: lin(g), b: lin(b), a: f32::from(a) / 255.0 }
    }

    fn with(self, channel: Channel, v: f32) -> Hsla {
        let mut c = self;
        match channel {
            Channel::Hue => c.h = v * 360.0,
            Channel::Saturation => c.s = v,
            Channel::Lightness => c.l = v,
            Channel::Alpha => c.a = v,
        }
        c
    }

    fn get(self, channel: Channel) -> f32 {
        match channel {
            Channel::Hue => self.h / 360.0,
            Channel::Saturation => self.s,
            Channel::Lightness => self.l,
            Channel::Alpha => self.a,
        }
    }
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` as sRGB bytes.
pub fn parse_rgba(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 => {
            let nib = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|v| v * 17);
            Some([nib(0)?, nib(1)?, nib(2)?, 255])
        }
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn rgba_hex([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// The open picker: which setting, which tab, and the color being edited. The HSL is kept rather
/// than read back from the hex each time, so dragging hue on a grey (where hex has no hue) or
/// saturation down to zero and back doesn't lose the hue.
pub struct ColorPickerState {
    pub key: String,
    pub tab: usize,
    pub hsla: Hsla,
    /// The palette's keyboard cursor, as `row * COLUMNS + column`.
    pub cursor: usize,
}

/// A slider: its channel, name, and gradient stops.
type Track = (Channel, &'static str, Vec<(f32, Color)>);

#[derive(Clone, Copy)]
enum Channel {
    Hue,
    Saturation,
    Lightness,
    Alpha,
}

const COLUMNS: usize = 12;
const SWATCH: f32 = 20.0;
const GAP: f32 = 4.0;
const TRACK_W: f32 = COLUMNS as f32 * SWATCH + (COLUMNS - 1) as f32 * GAP;
const TRACK_H: f32 = 10.0;
const KNOB: f32 = 16.0;
/// Both tabs' height, so switching between them doesn't resize the popover: the taller of the two
/// (four labelled sliders), with the palette's rows spread over it.
const PANEL_H: f32 = 180.0;

/// Rows of preset colors: a grey ramp, then six shades of twelve hues (lightest first).
fn palette() -> Vec<Vec<Hsla>> {
    let greys = (0..COLUMNS)
        .map(|i| Hsla { h: 0.0, s: 0.0, l: 1.0 - i as f32 / (COLUMNS - 1) as f32, a: 1.0 })
        .collect();
    let shades = [(0.9, 0.85), (0.85, 0.72), (0.8, 0.6), (0.75, 0.48), (0.7, 0.36), (0.65, 0.24)];
    let mut rows = vec![greys];
    rows.extend(shades.iter().map(|&(s, l)| {
        (0..COLUMNS).map(|i| Hsla { h: i as f32 * 360.0 / COLUMNS as f32, s, l, a: 1.0 }).collect()
    }));
    rows
}

impl SettingsWindow {
    /// The swatch next to a Color setting's hex field, opening the picker when pressed.
    pub(crate) fn render_color_picker(
        &self,
        s: &'static Setting,
        v: &Value,
        disabled: bool,
        k: &Kit,
        layer: &mut Vec<El>,
    ) -> El {
        let value = v.as_str().and_then(parse_rgba);
        let open = self.color_picker.as_ref().filter(|p| p.key == s.key);
        // The kept HSL while it still matches the value; the value's own once the hex field changed it.
        let hsla = match (open, value) {
            (Some(p), Some(rgba)) if p.hsla.to_rgba() == rgba => p.hsla,
            (_, Some(rgba)) => Hsla::from_rgba(rgba),
            (Some(p), None) => p.hsla,
            (None, None) => Hsla::BLACK,
        };
        let mut swatch = el()
            .key(format!("{}:swatch", s.key))
            .role("button")
            .label(format!("{}: pick a color", s.label))
            .size(24.0, 24.0)
            .radius(k.radius.sm)
            .fill(value.map_or(Color::TRANSPARENT, |_| hsla.color()))
            .border(1.0, k.c.border_strong);
        if !disabled {
            let (key, key_enter) = (s.key.clone(), s.key.clone());
            swatch = swatch
                .focusable(true)
                .on_press(move |app: &mut SettingsWindow| app.toggle_color_picker(&key))
                // Space presses a button; Enter opens it too, as the dropdowns do.
                .on_key(move |app: &mut SettingsWindow, ev: &KeyEvent, tx| {
                    if ev.key != Key::Enter {
                        return false;
                    }
                    if !tx.key_repeat() {
                        app.toggle_color_picker(&key_enter);
                    }
                    true
                });
        }
        let Some(open) = open.filter(|_| !disabled) else {
            return Popover::new(format!("color:{}", s.key)).trigger(swatch).build(k, layer);
        };
        let key = s.key.clone();
        let tabs = SegmentedControl::new(&["Palette", "Sliders"], Some(open.tab))
            .label("Picker")
            .on_pick(|app: &mut SettingsWindow, idx, _| {
                if let Some(p) = &mut app.color_picker {
                    p.tab = idx;
                }
            })
            .build(k)
            .key("tabs");
        let body = if open.tab == 0 { self.palette_panel(&key, hsla, open.cursor, k) } else { self.slider_panel(&key, hsla, k) };
        let footer = row()
            .key("current")
            .gap(8.0)
            .center_y()
            .child(el().key("preview").size(SWATCH, SWATCH).radius(k.radius.sm).fill(hsla.color()).border(1.0, k.c.border_strong))
            .child(text(hsla.hex()).key("hex").font(k.ty.mono).color(k.c.text_base));
        Popover::new(format!("color:{}", s.key))
            .label(format!("{} color", s.label))
            .trigger(swatch)
            .open(true)
            .on_close(|app: &mut SettingsWindow| app.color_picker = None)
            .child(
                col()
                    .key("picker")
                    // Modal: opening moves focus into it (the panel's first control), Tab stays in
                    // it, Escape or a press outside closes it, and focus goes back to the swatch.
                    .modal(true, |app: &mut SettingsWindow| app.color_picker = None)
                    .gap(12.0)
                    .w(TRACK_W)
                    .child(tabs)
                    .child(body)
                    .child(footer),
            )
            .build(k, layer)
    }

    /// The preset swatches as one keyboard control (a grid, as NSColorPanel's): arrows move a
    /// cursor in two dimensions, Home and End jump to the ends, and Return or Space picks.
    fn palette_panel(&self, key: &str, cur: Hsla, cursor: usize, k: &Kit) -> El {
        let cur_rgba = cur.to_rgba();
        let rows = palette();
        let count = rows.len() * COLUMNS;
        let picks: Vec<Hsla> = rows.iter().flatten().map(|&c| Hsla { a: cur.a, ..c }).collect();
        let key_nav = key.to_string();
        let value = picks.get(cursor).map(|c| c.hex()).unwrap_or_default();
        let mut grid = col()
            .key("palette")
            .role("grid")
            .label("Palette")
            .h(PANEL_H)
            .justify_between()
            .focusable(true)
            .tab_index(1)
            .on_key(move |app: &mut SettingsWindow, ev: &KeyEvent, tx| {
                if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
                    return false;
                }
                let Some(p) = app.color_picker.as_mut() else { return false };
                let at = p.cursor.min(count - 1);
                let next = match ev.key {
                    Key::Left => at.saturating_sub(1),
                    Key::Right => (at + 1).min(count - 1),
                    Key::Up => at.checked_sub(COLUMNS).unwrap_or(at),
                    Key::Down => if at + COLUMNS < count { at + COLUMNS } else { at },
                    Key::Home => 0,
                    Key::End => count - 1,
                    Key::Enter => {
                        if !tx.key_repeat() {
                            app.pick_color(&key_nav, picks[at]);
                        }
                        return true;
                    }
                    Key::Text(ref t) if t == " " => {
                        app.pick_color(&key_nav, picks[at]);
                        return true;
                    }
                    _ => return false,
                };
                p.cursor = next;
                true
            });
        for (r, colors) in rows.into_iter().enumerate() {
            let mut line = row().key(format!("row{r}")).gap(GAP);
            for (i, c) in colors.into_iter().enumerate() {
                let index = r * COLUMNS + i;
                let picked = c.to_rgba()[..3] == cur_rgba[..3];
                let key = key.to_string();
                // A picked swatch keeps the current alpha.
                let pick = Hsla { a: cur.a, ..c };
                let mut swatch = el()
                    .key(format!("swatch{r}-{i}"))
                    .size(SWATCH, SWATCH)
                    .radius(4.0)
                    .fill(c.color())
                    .border(if picked { 2.0 } else { 1.0 }, if picked { k.c.text_strong } else { k.alpha(k.c.border_strong, 0.5) })
                    .a11y_hidden()
                    .on_press(move |app: &mut SettingsWindow| {
                        if let Some(p) = app.color_picker.as_mut() {
                            p.cursor = index;
                        }
                        app.pick_color(&key, pick);
                    });
                if index == cursor {
                    swatch = swatch.outline(2.0, 1.0, k.c.border_focus);
                }
                line = line.child(swatch);
            }
            grid = grid.child(line);
        }
        // The grid is one control, so a reader hears the swatch under the cursor as its value.
        grid.a11y_value(value)
    }

    fn slider_panel(&self, key: &str, cur: Hsla, k: &Kit) -> El {
        let opaque = Hsla { a: 1.0, ..cur };
        let hues: Vec<(f32, Color)> =
            (0..=6).map(|i| (i as f32 / 6.0, Hsla { h: i as f32 * 60.0, s: 1.0, l: 0.5, a: 1.0 }.color())).collect();
        let tracks: [Track; 4] = [
            (Channel::Hue, "Hue", hues),
            (Channel::Saturation, "Saturation", vec![(0.0, opaque.with(Channel::Saturation, 0.0).color()), (1.0, opaque.with(Channel::Saturation, 1.0).color())]),
            (
                Channel::Lightness,
                "Lightness",
                vec![(0.0, Color::BLACK), (0.5, opaque.with(Channel::Lightness, 0.5).color()), (1.0, Color::WHITE)],
            ),
            (Channel::Alpha, "Alpha", vec![(0.0, Hsla { a: 0.0, ..opaque }.color()), (1.0, opaque.color())]),
        ];
        let mut panel = col().key("sliders").h(PANEL_H).justify_between();
        for (channel, name, stops) in tracks {
            panel = panel.child(
                col().key(format!("{name}-row")).gap(4.0)
                    .child(text(name).font(k.ty.small).color(k.c.text_muted))
                    .child(channel_slider(key, name, channel, cur, stops, k)),
            );
        }
        panel
    }

    fn toggle_color_picker(&mut self, key: &str) {
        if self.color_picker.as_ref().is_some_and(|p| p.key == key) {
            self.color_picker = None;
        } else {
            self.open_color_picker(key);
        }
    }

    pub(crate) fn open_color_picker(&mut self, key: &str) {
        let Some(s) = self.editor.schema.find(key) else { return };
        let hsla = self.editor.current_value(s).as_str().and_then(parse_rgba).map_or(Hsla::BLACK, Hsla::from_rgba);
        let tab = self.color_picker.as_ref().map_or(0, |p| p.tab);
        // The cursor starts on the current color's swatch, if it is one.
        let rgb = &hsla.to_rgba()[..3];
        let cursor = palette().iter().flatten().position(|c| &c.to_rgba()[..3] == rgb).unwrap_or(0);
        self.color_picker = Some(ColorPickerState { key: key.to_string(), tab, hsla, cursor });
    }

    /// Sets the setting to `hsla`, unless that is the value it already has: re-picking the current
    /// color mustn't turn `#1E1E2E` into a change to `#1e1e2e`.
    pub(crate) fn pick_color(&mut self, key: &str, hsla: Hsla) {
        if let Some(p) = self.color_picker.as_mut().filter(|p| p.key == key) {
            p.hsla = hsla;
        }
        let Some(s) = self.editor.schema.find(key) else { return };
        let cur = self.editor.current_value(s).as_str().and_then(parse_rgba);
        if cur != Some(hsla.to_rgba()) {
            self.set_and_sync(key, Value::String(hsla.hex()));
        }
    }
}

/// One channel's slider: its gradient track and a knob filled with the current color.
fn channel_slider(key: &str, name: &str, channel: Channel, cur: Hsla, stops: Vec<(f32, Color)>, k: &Kit) -> El {
    let travel = TRACK_W - KNOB;
    let frac = cur.get(channel).clamp(0.0, 1.0);
    let to_frac = move |x: f32| ((x - KNOB / 2.0) / travel).clamp(0.0, 1.0);
    let (key_step, key_limit, key_drag) = (key.to_string(), key.to_string(), key.to_string());
    el().key(format!("{name}-slider"))
        .role("slider")
        .label(name)
        .range_value(f64::from(frac), 0.0, 1.0)
        .range_step(0.01)
        .size(TRACK_W, KNOB)
        .focusable(true)
        // Before the tab switcher, so opening the picker focuses the first slider.
        .tab_index(1)
        .on_step_limit(move |app: &mut SettingsWindow, max, _| {
            app.pick_color(&key_limit, cur.with(channel, if max { 1.0 } else { 0.0 }));
        })
        .child(
            el().key("track")
                .size(TRACK_W, TRACK_H)
                .absolute(0.0, (KNOB - TRACK_H) / 2.0)
                .radius(TRACK_H / 2.0)
                .fill(k.c.layer_03)
                .linear_gradient(std::f32::consts::FRAC_PI_2, stops)
                .border(1.0, k.alpha(k.c.border_strong, 0.5))
                .a11y_hidden(),
        )
        .child(
            el().key("knob")
                .size(KNOB, KNOB)
                .absolute(frac * travel, 0.0)
                .radius(KNOB / 2.0)
                .fill(Hsla { a: 1.0, ..cur }.color())
                .border(2.0, Color::WHITE)
                .a11y_hidden(),
        )
        .on_step(move |app: &mut SettingsWindow, steps, _| {
            let next = (frac + steps as f32 * 0.01).clamp(0.0, 1.0);
            app.pick_color(&key_step, cur.with(channel, next));
        })
        .on_drag(move |app: &mut SettingsWindow, d: &mut Drag, tx| {
            if matches!(d.phase, Phase::Down | Phase::Move) {
                app.pick_color(&key_drag, cur.with(channel, to_frac(d.x)));
                tx.instant();
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_survives_the_round_trip_through_hsl() {
        for hex in ["#1e1e2e", "#ff0000", "#00ff7f", "#808080", "#000000", "#ffffff", "#12345678", "#f5c2e7"] {
            let rgba = parse_rgba(hex).unwrap();
            assert_eq!(Hsla::from_rgba(rgba).hex(), hex);
        }
    }

    #[test]
    fn short_and_upper_case_hex_parse() {
        assert_eq!(parse_rgba("#FFF"), Some([255, 255, 255, 255]));
        assert_eq!(parse_rgba("#1E1E2E"), parse_rgba("#1e1e2e"));
        assert_eq!(parse_rgba("red"), None);
    }

    #[test]
    fn the_palette_is_a_full_grid() {
        let rows = palette();
        assert_eq!(rows.len(), 7);
        assert!(rows.iter().all(|r| r.len() == COLUMNS));
    }
}
