//! The dropdown used wherever a value is chosen from a known list: fonts (each in its own face),
//! themes (each with its colors) and long flag lists. Every such field in every app goes through
//! `Chooser`, so a font or theme looks and behaves the same in a setting, a list of fonts, or a
//! column in a table of entries.

use crate::model::is_hex_color;
use gpui_kit::component::{
    ActiveTheme as _, Colorize as _, Sizable as _, h_flex,
    combobox::{Combobox, ComboboxState},
    searchable_list::SearchableListItem,
    tag::Tag,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use settings_core::ThemeInfo;
use std::rc::Rc;

/// An entry a `Chooser` can list. Its value is the text written to the config.
pub trait ChoiceItem: SearchableListItem<Value = SharedString> + 'static {
    /// The entry for a value that isn't in the list (e.g. a font built into the terminal).
    fn unlisted(name: &SharedString) -> Self;
}

impl ChoiceItem for SharedString {
    fn unlisted(name: &SharedString) -> Self {
        name.clone()
    }
}

/// A font, shown in its own face.
#[derive(Clone)]
pub struct FontItem(pub SharedString);

impl SearchableListItem for FontItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.0.clone()
    }

    fn value(&self) -> &SharedString {
        &self.0
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .w_full()
            .gap_3()
            .child(div().flex_1().truncate().font_family(self.0.clone()).child(format!("{}  0O 1lI {{}} =>", self.0)))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.0.clone()))
    }
}

impl ChoiceItem for FontItem {
    fn unlisted(name: &SharedString) -> Self {
        FontItem(name.clone())
    }
}

/// A theme, shown with its background, foreground and ANSI colors.
#[derive(Clone)]
pub struct ThemeItem {
    name: SharedString,
    pub info: ThemeInfo,
}

impl ThemeItem {
    pub fn new(info: ThemeInfo) -> Self {
        Self { name: info.name.clone().into(), info }
    }
}

impl SearchableListItem for ThemeItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &SharedString {
        &self.name
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let t = &self.info;
        // Terminal colors are user data, so the preview paints them as-is.
        let color = |hex: &Option<String>| hex.as_deref().filter(|h| is_hex_color(h)).and_then(|h| Hsla::parse_hex(h).ok());
        let bg = color(&t.background).unwrap_or(theme.muted);
        let fg = color(&t.foreground).unwrap_or(theme.foreground);
        h_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .flex_none()
                    .w_32()
                    .h_6()
                    .px_2()
                    .gap_1()
                    .rounded(theme.radius)
                    .bg(bg)
                    .border_1()
                    .border_color(theme.border)
                    .child(div().text_xs().text_color(fg).child("Aa"))
                    .children(t.ansi.iter().map(|c| div().size_2().rounded_full().bg(color(c).unwrap_or(fg)))),
            )
            .child(div().flex_1().truncate().child(self.name.clone()))
            .when(!t.installed, |r| r.child(Tag::secondary().small().child("Download")))
    }
}

impl ChoiceItem for ThemeItem {
    fn unlisted(name: &SharedString) -> Self {
        ThemeItem::new(ThemeInfo { name: name.to_string(), installed: true, background: None, foreground: None, ansi: Vec::new() })
    }
}

pub type ChoiceState<I> = ComboboxState<Vec<I>>;

/// A dropdown over a shared list (`base`) plus the names it must show on top of it: values that
/// aren't in the list (a built-in font, a font from another machine, a theme file that's gone).
pub struct Chooser<I: ChoiceItem> {
    pub state: Entity<ChoiceState<I>>,
    base: Option<Rc<Vec<I>>>,
    extra: Vec<SharedString>,
}

impl<I: ChoiceItem> Chooser<I> {
    pub fn new<V: 'static>(multiple: bool, searchable: bool, window: &mut Window, cx: &mut Context<V>) -> Self {
        let state = cx.new(|cx| ChoiceState::new(Vec::new(), Vec::new(), window, cx).multiple(multiple).searchable(searchable));
        Self { state, base: None, extra: Vec::new() }
    }

    /// Lists `base` (plus `wanted` and `defaults` where it lacks them) and selects `wanted`.
    /// Emits no event, so it never records an edit.
    pub fn sync<V: 'static>(&mut self, base: &Rc<Vec<I>>, wanted: &[SharedString], defaults: &[SharedString], window: &mut Window, cx: &mut Context<V>) {
        // Recomputed every time, so a placeholder goes away once the list has the real item (a
        // theme list arriving after the first sync) and a value the setting no longer has drops out.
        let mut extra: Vec<SharedString> = Vec::new();
        for name in defaults.iter().chain(wanted) {
            if !extra.contains(name) && !base.iter().any(|i| i.value() == name) {
                extra.push(name.clone());
            }
        }
        let rebuild = !self.base.as_ref().is_some_and(|b| Rc::ptr_eq(b, base)) || extra != self.extra;
        self.extra = extra;
        if rebuild {
            self.base = Some(base.clone());
            let items: Vec<I> = self.extra.iter().map(I::unlisted).chain(base.iter().cloned()).collect();
            self.state.update(cx, |state, cx| state.set_items(items, window, cx));
        }
        // Re-selecting clears the dropdown's search, so leave a selection that already matches.
        if rebuild || self.state.read(cx).selected_values() != wanted {
            self.state.update(cx, |state, cx| state.set_selected_values(wanted, window, cx));
        }
    }

    /// The selected values, in selection order.
    pub fn selected(&self, cx: &App) -> Vec<SharedString> {
        self.state.read(cx).selected_values()
    }

    /// The dropdown, sized by the caller's wrapper (see AGENTS.md).
    pub fn render(&self, placeholder: impl Into<SharedString>, search: &'static str, cleanable: bool, disabled: bool) -> Combobox<Vec<I>> {
        Combobox::new(&self.state)
            .small()
            .menu_width(px(420.))
            .placeholder(placeholder)
            .when(!search.is_empty(), |c| c.search_placeholder(search))
            .cleanable(cleanable)
            .disabled(disabled)
    }
}

/// The names a value holds: a list's entries, or a single string. Empty strings don't count.
pub fn names(v: &serde_json::Value) -> Vec<SharedString> {
    let one = |v: &serde_json::Value| crate::model::plain(v);
    let all: Vec<String> = match v {
        serde_json::Value::Array(items) => items.iter().map(one).collect(),
        serde_json::Value::Null => Vec::new(),
        other => vec![one(other)],
    };
    all.into_iter().filter(|n| !n.is_empty()).map(SharedString::from).collect()
}

#[cfg(test)]
mod tests {
    // Not `gpui_kit::*`: that brings in GPUI's `test` attribute, which shadows the standard one.
    use super::{Chooser, ThemeItem};
    use gpui_kit::{AppContext as _, Context, SharedString, TestAppContext};
    use settings_core::ThemeInfo;
    use std::rc::Rc;

    struct Host;

    #[gpui_kit::test]
    fn a_placeholder_gives_way_to_the_real_item_when_the_list_arrives(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            cx.new(|cx: &mut Context<Host>| {
                let mine: SharedString = "Mine".into();
                let mut chooser: Chooser<ThemeItem> = Chooser::new(false, true, window, cx);
                // At startup the theme list hasn't loaded yet: "Mine" can only be a placeholder.
                chooser.sync(&Rc::new(Vec::new()), std::slice::from_ref(&mine), &[], window, cx);
                let real = ThemeItem::new(ThemeInfo {
                    name: "Mine".into(),
                    installed: true,
                    background: Some("#101010".into()),
                    foreground: None,
                    ansi: Vec::new(),
                });
                chooser.sync(&Rc::new(vec![real]), std::slice::from_ref(&mine), &[], window, cx);
                let selection = chooser.state.read(cx).selection().to_vec();
                assert_eq!(selection.len(), 1);
                assert_eq!(selection[0].1.info.background.as_deref(), Some("#101010"), "the real theme is selected");
                assert!(chooser.extra.is_empty(), "no placeholder is listed next to it");
                Host
            });
        });
    }
}
