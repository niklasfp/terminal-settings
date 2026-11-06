//! The theme setting's searchable list, with each theme previewed as GPUI Kit's chooser does: its
//! background with "Aa" in its foreground and dots of its eight ANSI colors, then the name, and a
//! "Download" tag on a theme that isn't installed. Rune Kit's `Combobox` draws text-only rows (its
//! options take no element), so this is the same popover built from Rune Kit's public parts: the
//! field trigger, a plain search input over a divider, and a virtualized scrolling list.
//!
//! It keeps `Combobox`'s contract: the popover is keyed `combo:<setting>`, the search field takes
//! focus when it opens, Up/Down move the highlight, Enter picks it and Escape closes.

use crate::controls::parse_hex_color;
use crate::state::SettingsWindow;
use rune::{Color, El, GrowthSpec, Key, ScrollSpec, col, el, row, text};
use rune_kit::{Kit, Popover, Select, SelectAppearance, TextInput};
use settings_core::{Setting, ThemeInfo};

const ROW_H: f32 = 34.0;
const MAX_H: f32 = 320.0;
const WIDTH: f32 = 340.0;

/// Case-insensitive subsequence match; contiguous runs and word starts score higher (as
/// `Combobox`'s own filter does).
fn score(query: &str, value: &str) -> Option<i32> {
    let needle: Vec<char> = query.chars().collect();
    let (mut n, mut score, mut previous, mut boundary) = (0, 0, false, true);
    for c in value.to_lowercase().chars() {
        if n < needle.len() && c == needle[n] {
            score += 10 + if previous { 12 } else { 0 } + if boundary { 8 } else { 0 };
            previous = true;
            n += 1;
        } else {
            previous = false;
            score -= 1;
        }
        boundary = !c.is_alphanumeric();
    }
    (n == needle.len()).then_some(score)
}

/// Indices of the themes matching `query`, best first; all of them, in order, for an empty one.
pub fn filter(themes: &[ThemeInfo], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0..themes.len()).collect();
    }
    let mut hits: Vec<(usize, i32)> = themes.iter().enumerate().filter_map(|(i, t)| score(&query, &t.name).map(|s| (i, s))).collect();
    hits.sort_by_key(|&(i, s)| (std::cmp::Reverse(s), i));
    hits.into_iter().map(|(i, _)| i).collect()
}

/// The swatch showing a theme: background, "Aa" in the foreground, and its ANSI colors. Terminal
/// colors are user data, so they're painted as they are.
pub fn preview(t: &ThemeInfo, k: &Kit) -> El {
    let color = |hex: Option<&str>| hex.and_then(parse_hex_color);
    let bg = color(t.background.as_deref()).unwrap_or(k.c.layer_03);
    let fg = color(t.foreground.as_deref()).unwrap_or(k.c.text_base);
    let mut chip = row()
        .key("preview")
        .w(120.0)
        .h(22.0)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .radius(k.radius.sm)
        .fill(bg)
        .border(1.0, k.c.border_base)
        .a11y_hidden()
        .child(text("Aa").font(k.ty.small).color(fg));
    for (i, c) in t.ansi.iter().enumerate() {
        chip = chip.child(el().key(format!("ansi{i}")).size(8.0, 8.0).radius(4.0).fill(color(c.as_deref()).unwrap_or(fg)));
    }
    chip
}

impl SettingsWindow {
    pub(crate) fn render_theme_chooser(&self, s: &'static Setting, cur: Option<&str>, disabled: bool, k: &Kit, layer: &mut Vec<El>) -> El {
        let key = s.key.clone();
        let popover_key = format!("combo:{key}");
        let state = self.combos.get(&key);
        let open = state.is_some_and(|c| c.open) && !disabled;
        let mut trigger = Select::new::<&str>(format!("{popover_key}-value"), &[])
            .current(cur)
            .placeholder("No theme")
            .label(s.label.clone())
            .appearance(SelectAppearance::Field)
            .width(260.0)
            .disabled(disabled)
            .trigger(k);
        if !disabled {
            let (key, current) = (key.clone(), cur.map(str::to_string));
            trigger = trigger.on_press_tx(move |app: &mut SettingsWindow, tx| {
                let c = app.combos.entry(key.clone()).or_default();
                c.open = !c.open;
                c.search.clear();
                c.search.focused = c.open;
                c.active = current.clone();
                if c.open && let Some(name) = &current {
                    tx.scroll_into_view(format!("{key}-theme-{name}"));
                }
            });
        }
        let close_key = key.clone();
        let popover = Popover::new(&popover_key)
            .trigger(trigger)
            .label(s.label.clone())
            .padding(0.0)
            .open(open)
            .on_close(move |app: &mut SettingsWindow| app.combos.entry(close_key.clone()).or_default().close());
        let Some(state) = state.filter(|_| open) else { return popover.build(k, layer) };

        let matches = filter(&self.themes, &state.search.text);
        // The highlight: the remembered one while it still matches, else the first match.
        let active = state
            .active
            .as_deref()
            .and_then(|name| matches.iter().copied().find(|&i| self.themes[i].name == name))
            .or_else(|| matches.first().copied());

        let (k_edit, k_change, k_key) = (key.clone(), key.clone(), key.clone());
        let search = TextInput::new(&state.search)
            .plain()
            .leading_icon(k.glyph.search)
            .height(36.0)
            .width(None)
            .placeholder("Search themes…")
            .label(format!("Search {}", s.label))
            .on_edit(move |app: &mut SettingsWindow| &mut app.combos.entry(k_edit.clone()).or_default().search)
            .on_change(move |app: &mut SettingsWindow, _| {
                if let Some(c) = app.combos.get_mut(&k_change) {
                    c.active = None;
                }
            })
            .on_key(move |app: &mut SettingsWindow, ev, tx| {
                let Some(c) = app.combos.get(&k_key) else { return false };
                if c.search.preedit.is_some() {
                    return false;
                }
                let matches = filter(&app.themes, &c.search.text);
                let at = c
                    .active
                    .as_deref()
                    .and_then(|name| matches.iter().position(|&i| app.themes[i].name == name))
                    .unwrap_or(0);
                match ev.key {
                    Key::Escape => {
                        app.combos.entry(k_key.clone()).or_default().close();
                        true
                    }
                    Key::Enter => {
                        if let Some(&i) = matches.get(at) {
                            let name = app.themes[i].name.clone();
                            app.choose_theme(&k_key, &name);
                        }
                        true
                    }
                    Key::Down | Key::Up if !matches.is_empty() => {
                        let delta = if ev.key == Key::Down { 1 } else { -1 };
                        let next = (at as isize + delta).rem_euclid(matches.len() as isize) as usize;
                        let name = app.themes[matches[next]].name.clone();
                        tx.scroll_into_view(format!("{k_key}-theme-{name}"));
                        app.combos.entry(k_key.clone()).or_default().active = Some(name);
                        true
                    }
                    _ => false,
                }
            })
            .build(k)
            .key("search");

        let mut rows = col().key("rows").w_full().growth(GrowthSpec::default()).virtualize(ROW_H).reveal_selected(true);
        for (n, &i) in matches.iter().enumerate() {
            let t = &self.themes[i];
            let is_active = active == Some(i);
            let is_current = cur == Some(t.name.as_str());
            let (k_hover, k_pick, name) = (key.clone(), key.clone(), t.name.clone());
            let hover_name = name.clone();
            let mut item = row()
                .key(format!("{key}-theme-{}", t.name))
                .role("option")
                .label(t.name.clone())
                .position_in_set(n + 1, matches.len())
                .focus_selected(is_active)
                .w_full()
                .h(ROW_H)
                .px(8.0)
                .gap(10.0)
                .center_y()
                .radius(4.0)
                .fill(if is_active { k.c.hover } else { Color::TRANSPARENT })
                .child(preview(t, k))
                .child(text(t.name.clone()).font(k.ty.ui_compact).color(if is_current { k.c.text_strong } else { k.c.text_base }).grow(1.0).shrink().truncate().a11y_hidden());
            if !t.installed {
                item = item.child(
                    el().key("download")
                        .px(6.0)
                        .py(1.0)
                        .radius(4.0)
                        .fill(k.c.layer_03)
                        .child(text("Download").font(k.ty.small).color(k.c.text_muted))
                        .a11y_hidden(),
                );
            }
            item = item
                .on_hover(move |app: &mut SettingsWindow| {
                    if let Some(c) = app.combos.get_mut(&k_hover) {
                        c.active = Some(hover_name.clone());
                    }
                })
                .on_press(move |app: &mut SettingsWindow| app.choose_theme(&k_pick, &name));
            rows = rows.child(item);
        }
        let viewport = col()
            .key("viewport")
            .role("listbox")
            .label(s.label.clone())
            .w_full()
            .h((matches.len() as f32 * ROW_H).min(MAX_H))
            .shrink()
            .min_h(if matches.is_empty() { 0.0 } else { ROW_H })
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(16.0)
            .scroll_margin(16.0)
            .clip_soft(0.0)
            .child(rows);
        let mut results = col()
            .key("results")
            .w_full()
            .padding(4.0)
            .shrink()
            .min_h(0.0)
            .preserve_focus_on_press(true)
            .child(rune_kit::scroll_bar(k, &format!("{popover_key}-scroll-area"), "viewport", viewport).w_full().shrink().min_h(0.0));
        if matches.is_empty() {
            results = results.child(row().key("empty").h(28.0).px(8.0).center_y().child(text("No matching themes").font(k.ty.ui_compact).color(k.c.text_weak)));
        }
        let body = col()
            .w(WIDTH)
            .shrink()
            .min_h(0.0)
            .child(row().key("header").w_full().px(4.0).child(search))
            .child(el().key("divider").w_full().h(1.0).fill(k.c.border_weak).a11y_hidden())
            .child(results);
        popover.child(body).build(k, layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(name: &str) -> ThemeInfo {
        ThemeInfo { name: name.into(), installed: true, background: None, foreground: None, ansi: Vec::new() }
    }

    #[test]
    fn the_filter_ranks_word_starts_and_runs_first() {
        let themes = [theme("Solarized Dark"), theme("Dracula"), theme("Dark Pastel")];
        assert_eq!(filter(&themes, ""), [0, 1, 2]);
        assert_eq!(filter(&themes, "drac")[0], 1);
        assert_eq!(filter(&themes, "dark"), [2, 0]);
        assert!(filter(&themes, "zzz").is_empty());
    }
}
