//! The settings window rendered with Rune and rune-kit: header (logo, file path, platform scope,
//! appearance, Save button with dirty count), sidebar of section groups with search and counts,
//! setting cards (or search results across all groups), Rio's live terminal colors preview, and
//! the save-with-diff modal dialog.

pub use crate::state::{
    Appearance, AsyncMsg, Banner, PreviewPayload, SaveDonePayload, SaveOutcome, SavePreview,
    SettingsWindow,
};

use crate::controls::parse_hex_color;
use crate::icons;
use crate::model::{ANSI, Lock};
use crate::VERSION;
use rune::tok::motion;
use rune::{Color, Cx, El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{
    Badge, BadgeVariant, Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant,
    Kit, ScrollBar, SegmentedControl, Select, TextInput, Toast, ToastRegion, glyphs,
};
use serde_json::Value;
use settings_core::{Setting, SettingType};
use std::path::Path;

const PLATFORMS: [(&str, &str); 4] = [
    ("", "All platforms"),
    ("macos", "macOS"),
    ("linux", "Linux"),
    ("windows", "Windows"),
];

fn platform_label(p: &str) -> &str {
    PLATFORMS.iter().find(|(id, _)| *id == p).map_or(p, |(_, l)| l)
}

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

/// `"~/…"` for paths under the home folder (a Unix convention; Windows shows the full path).
pub fn tidy_path(p: &Path) -> String {
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

impl SettingsWindow {
    pub fn view(&self, cx: &mut Cx<Self>) -> El {
        let (k, sidebar_bg) = self.kit(cx.system_dark());
        let mut layer: Vec<El> = Vec::new();
        let query = self.query();

        let header = self.render_header(&k, &mut layer, cx.titlebar_inset());
        let sidebar = self.render_sidebar(&query, &k, sidebar_bg);
        let content = self.render_content(&query, &k, &mut layer);

        self.render_save_dialog(&k, &mut layer);
        self.render_discard_dialog(&k, &mut layer);
        if let Some(msg) = &self.toast {
            let toast = Toast::new("status-toast", msg.clone())
                .closable()
                .on_close(|app: &mut Self| app.toast = None)
                .build(&k);
            layer.push(ToastRegion::new().toast(toast).build(&k));
        }

        let split = row().key("main-split").grow(1.0).min_h(0.0).w_full().child(sidebar).child(content);
        let mut base = col().key("settings-window").w_full().h_full().fill(k.c.bg_base).child(header);
        if let Some(banner) = self.render_playground_banner(&k) {
            base = base.child(banner);
        }
        base = base.child(split);
        rune_kit::root(base, layer)
    }

    fn render_header(&self, k: &Kit, layer: &mut Vec<El>, titlebar_inset: f32) -> El {
        let dirty = self.editor.dirty_count();
        let playground = self.playground.is_some();
        let state = &self.editor.state;
        let path = if state.target != state.path {
            format!("{} → {}", tidy_path(&state.path), tidy_path(&state.target))
        } else {
            tidy_path(&state.path)
        };
        let left_pad = (titlebar_inset + 16.0).max(20.0);
        let mut r = row()
            .key("header")
            .h(60.0)
            .w_full()
            .pl(left_pad)
            .pr(20.0)
            .gap(14.0)
            .center_y()
            .fill(k.c.layer_01)
            .border(1.0, k.c.border_weak)
            .window_drag();

        if let Some(logo) = &self.logo {
            r = r.child(el().key("app-logo").size(32.0, 32.0).pixels(logo.pixels()).a11y_hidden());
        }

        r = r.child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(self.editor.schema.title.clone()).key("window-title").font(k.ty.body_medium).color(k.c.text_base))
                .child(text(path).key("config-path").font(k.ty.mono).color(k.c.text_muted).truncate()),
        );

        if !self.editor.schema.scopes.is_empty() {
            let scope_pairs: Vec<(&str, &str)> = PLATFORMS
                .iter()
                .copied()
                .filter(|(id, _)| id.is_empty() || self.editor.schema.scopes.iter().any(|s| s == id))
                .collect();
            let labels: Vec<&str> = scope_pairs.iter().map(|(_, l)| *l).collect();
            let cur_label = platform_label(&self.editor.scope);
            let open = self.open_select.as_deref() == Some("header:scope");
            r = r.child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(text("Applies to").font(k.ty.small).color(k.c.text_muted))
                    .child(
                        Select::new("header:scope", &labels)
                            .label("Applies to")
                            .width(148.0)
                            .current(Some(cur_label))
                            .open(open)
                            .disabled(dirty > 0)
                            .on_toggle(|app: &mut Self| {
                                if app.open_select.as_deref() == Some("header:scope") {
                                    app.open_select = None;
                                } else {
                                    app.open_select = Some("header:scope".into());
                                }
                            })
                            .on_close(|app: &mut Self| app.open_select = None)
                            .on_choose(move |app: &mut Self, chosen: &str, _| {
                                app.open_select = None;
                                let scope = PLATFORMS.iter().find(|(_, l)| *l == chosen).map_or("", |(id, _)| *id);
                                app.editor.set_scope(scope);
                                app.sync_all();
                            })
                            .build(k, layer),
                    ),
            );
        }

        let app_idx = match self.appearance {
            Appearance::System => 0,
            Appearance::Light => 1,
            Appearance::Dark => 2,
        };
        r = r.child(
            SegmentedControl::new(&["System", "Light", "Dark"], Some(app_idx))
                .label("Appearance")
                .fit()
                .on_pick(|app: &mut Self, idx, tx| {
                    let next = match idx {
                        1 => Appearance::Light,
                        2 => Appearance::Dark,
                        _ => Appearance::System,
                    };
                    app.appearance = next;
                    if let Some(path) = &app.appearance_file {
                        next.save(path);
                    }
                    tx.color_motion(motion::CALM);
                })
                .build(k)
                .key("appearance"),
        );

        if playground {
            r = r.child(
                Button::new("Open…")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Large)
                    .icon(&glyphs::FOLDER_OPEN)
                    .disabled(self.busy)
                    .help("Edit a config file from your computer")
                    .on_press(|app: &mut Self| app.open_file())
                    .build(k)
                    .key("open"),
            );
        }

        let save_label = if dirty > 0 { format!("Save ({dirty})") } else { "Save".to_string() };
        let save_help = if playground { "Download or copy the changed file" } else { "Save (⌘S)" };
        r.child(
            Button::new(save_label)
                .variant(ButtonVariant::Contrast)
                .control_size(ControlSize::Large)
                .disabled(dirty == 0 || self.busy)
                .help(save_help)
                .on_press(|app: &mut Self| app.start_save())
                .build(k)
                .key("save"),
        )
    }

    fn render_sidebar(&self, query: &str, k: &Kit, sidebar_bg: Color) -> El {
        let schema = self.editor.schema;
        let searching = !query.is_empty();
        let mut groups_col = col().key("groups-menu").w_full().padding(10.0).gap(2.0);
        for g in &schema.groups {
            let count = if searching {
                schema.settings.iter().filter(|s| s.group == g.id && self.visible(s, query)).count()
            } else {
                self.editor.in_file_count(&g.id)
            };
            if searching && count == 0 {
                continue;
            }
            let active = !searching && self.active_group == g.id;
            let id = g.id.clone();
            let mut item = row()
                .key(format!("group:{}", g.id))
                .role("tab")
                .label(&g.title)
                .checked(active)
                .focusable(true)
                .w_full()
                .h(34.0)
                .px(10.0)
                .gap(8.0)
                .radius(k.radius.md)
                .center_y()
                .fill(if active { k.c.surface_hover } else { Color::TRANSPARENT })
                .hover(k.c.hover)
                .press(k.c.pressed)
                .on_press(move |app: &mut Self| {
                    app.active_group = id.clone();
                    app.search.clear();
                })
                .child(
                    icon(icons::group_icon(&g.icon), 15.0)
                        .color(if active { k.c.text_accent } else { k.c.icon_muted })
                        .a11y_hidden(),
                )
                .child(
                    text(g.title.clone())
                        .grow(1.0)
                        .font(if active { k.ty.ui_medium } else { k.ty.ui })
                        .color(if active { k.c.text_base } else { k.c.text_muted })
                        .truncate()
                        .a11y_hidden(),
                );
            if count > 0 {
                item = item.child(
                    row()
                        .key(format!("group-count:{}", g.id))
                        .px(6.0)
                        .py(1.0)
                        .radius(10.0)
                        .fill(k.c.layer_03)
                        .child(text(count.to_string()).font(k.ty.small_medium).color(k.c.text_muted)),
                );
            }
            groups_col = groups_col.child(item);
        }

        let revision: String = schema.revision.chars().take(7).collect();
        let viewport = col()
            .key("groups-scroll")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec { stick_bottom: false })
            .child(groups_col)
            .scroll_bar(k);

        let search_box = TextInput::new(&self.search)
            .label("Search settings")
            .placeholder("Search settings… (/)")
            .leading_icon(&glyphs::SEARCH)
            .clear_button()
            .width(None)
            .on_edit(|app: &mut Self| &mut app.search)
            .build(k)
            .key("search");

        col()
            .key("groups")
            .w(248.0)
            .h_full()
            .min_h(0.0)
            .fill(sidebar_bg)
            .border(1.0, k.c.border_weak)
            .child(col().w_full().pt(12.0).px(12.0).pb(4.0).child(search_box))
            .child(viewport)
            .child(
                col()
                    .key("groups-footer")
                    .w_full()
                    .px(16.0)
                    .pb(12.0)
                    .pt(6.0)
                    .gap(2.0)
                    .child(text(format!("Schema from {} {revision}", schema.terminal)).font(k.ty.small).color(k.c.text_faint))
                    .child(text(format!("{} {VERSION}", self.app.name)).font(k.ty.small).color(k.c.text_faint)),
            )
    }

    fn render_content(&self, query: &str, k: &Kit, layer: &mut Vec<El>) -> El {
        let mut page_col = col().key("content-inner").w_full().max_w(896.0).px(32.0).py(28.0).gap(24.0);

        if let Some(banner) = self.render_banner(k) {
            page_col = page_col.child(banner);
        }
        if !self.editor.scope.is_empty() {
            let scope = &self.editor.scope;
            page_col = page_col.child(self.alert_box(
                "scope-banner",
                format!(
                    "Editing {} overrides ([platform.{scope}] in {}). Dimmed settings can't be overridden per platform; switch “Applies to” back to All platforms to edit them.",
                    platform_label(scope),
                    self.file_name()
                ),
                k.c.info.bg,
                k.c.info.border,
                k.c.text_base,
                None,
                k,
            ));
        }
        for (i, notice) in self.editor.state.notices.iter().enumerate() {
            page_col = page_col.child(
                text(notice.clone()).key(format!("notice-{i}")).font(k.ty.text_small).color(k.c.text_muted).wrap(),
            );
        }

        let pages: Vec<El> = if query.is_empty() {
            self.render_group(&self.active_group, "", k, layer).into_iter().collect()
        } else {
            self.editor.schema.groups.iter().filter_map(|g| self.render_group(&g.id, query, k, layer)).collect()
        };

        if pages.is_empty() {
            page_col = page_col.child(
                row()
                    .key("empty-search")
                    .w_full()
                    .py(48.0)
                    .center()
                    .child(text("No settings match your search.").font(k.ty.body).color(k.c.text_muted)),
            );
        } else {
            for p in pages {
                page_col = page_col.child(p);
            }
        }

        let viewport = col()
            .key("content")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .items_center()
            .scroll(ScrollSpec { stick_bottom: false })
            .child(page_col)
            .scroll_bar(k);

        col().key("content-pane").grow(1.0).min_w(0.0).min_h(0.0).h_full().child(viewport)
    }

    #[allow(clippy::too_many_arguments)]
    fn alert_box(
        &self,
        key: &str,
        message: String,
        bg: Color,
        border: Color,
        fg: Color,
        action: Option<El>,
        k: &Kit,
    ) -> El {
        let mut r = row()
            .key(key)
            .w_full()
            .px(14.0)
            .py(10.0)
            .gap(12.0)
            .radius(k.radius.md)
            .fill(bg)
            .border(1.0, border)
            .center_y()
            .child(text(message).grow(1.0).font(k.ty.text_small).color(fg).wrap());
        if let Some(btn) = action {
            r = r.child(btn);
        }
        r
    }

    fn render_banner(&self, k: &Kit) -> Option<El> {
        let file = self.file_name();
        match self.banner.as_ref()? {
            Banner::Conflict => {
                let btn = Button::new("Reload and discard my changes")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Regular)
                    .on_press(|app: &mut Self| {
                        app.banner = None;
                        app.reload();
                    })
                    .build(k)
                    .key("reload");
                Some(self.alert_box(
                    "conflict-banner",
                    format!("{file} changed on disk (maybe you edited it in your editor)."),
                    k.c.warning.bg,
                    k.c.warning.border,
                    k.c.text_base,
                    Some(btn),
                    k,
                ))
            }
            Banner::ParseError(message) => Some(self.alert_box(
                "parse-error-banner",
                self.parse_error_message(message),
                k.c.danger.bg,
                k.c.danger.border,
                k.c.danger.fg,
                None,
                k,
            )),
        }
    }

    fn render_group(&self, group_id: &str, query: &str, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
        let schema = self.editor.schema;
        let group = schema.groups.iter().find(|g| g.id == group_id)?;
        let settings: Vec<&'static Setting> =
            schema.settings.iter().filter(|s| s.group == group.id && self.visible(s, query)).collect();
        if settings.is_empty() {
            return None;
        }
        let mut subgroups: Vec<Option<String>> = Vec::new();
        if settings.iter().any(|s| s.subgroup.is_none()) {
            subgroups.push(None);
        }
        for s in &settings {
            if s.subgroup.is_some() && !subgroups.contains(&s.subgroup) {
                subgroups.push(s.subgroup.clone());
            }
        }

        let mut sec = col().key(format!("page:{}", group.id)).w_full().gap(16.0).child(
            row()
                .gap(10.0)
                .center_y()
                .child(icon(icons::group_icon(&group.icon), 20.0).color(k.c.text_accent).a11y_hidden())
                .child(text(group.title.clone()).font(k.ty.body_medium).color(k.c.text_base)),
        );
        if let Some(doc) = &group.doc {
            sec = sec.child(text(doc.replace('`', "")).font(k.ty.text_small).color(k.c.text_muted).wrap());
        }
        if group.id == "colors" && query.is_empty() && schema.theme_locks_colors {
            sec = sec.child(self.render_colors_extras(k));
        }

        for (sg_ix, sub) in subgroups.into_iter().enumerate() {
            let group_rows: Vec<&'static Setting> = settings.iter().copied().filter(|s| s.subgroup == sub).collect();
            let mut card = col()
                .key(format!("card:{}:{sg_ix}", group.id))
                .w_full()
                .radius(k.radius.lg)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_weak);
            if let Some(title) = &sub {
                card = card.child(
                    row()
                        .w_full()
                        .px(16.0)
                        .pt(12.0)
                        .pb(6.0)
                        .child(text(title.to_uppercase()).font(k.ty.small_medium).color(k.c.text_faint)),
                );
            }
            for (i, s) in group_rows.iter().enumerate() {
                if i > 0 {
                    card = card.child(
                        el().key(format!("sep:{}:{i}", s.key)).h(1.0).w_full().fill(k.c.border_weak).a11y_hidden(),
                    );
                }
                card = card.child(self.render_row(s, k, layer));
            }
            sec = sec.child(card);
        }
        Some(sec)
    }

    fn render_row(&self, s: &'static Setting, k: &Kit, layer: &mut Vec<El>) -> El {
        let lock = self.editor.lock(s);
        let disabled = lock.is_some() || self.busy;
        let dirty = self.editor.is_dirty(&s.key);
        let set = self.editor.is_set(s);
        let in_file = self.editor.file_value(s).is_some();
        let error = self.errors.get(&s.key).cloned();
        let wide = matches!(s.ty, SettingType::Records | SettingType::Raw);
        let current_plat = current_platform();

        let mut title_row =
            row().gap(6.0).flex_wrap().center_y().child(text(s.label.clone()).font(k.ty.ui_medium).color(k.c.text_base));
        if in_file {
            title_row = title_row.child(
                Badge::new("In your config").variant(BadgeVariant::Accent).build(k).key(format!("{}:set", s.key)),
            );
        }
        for p in s.platforms.iter().flatten() {
            let v = if p == current_plat { BadgeVariant::Accent } else { BadgeVariant::Subtle };
            title_row = title_row.child(Badge::new(platform_label(p)).variant(v).build(k));
        }
        for t in &s.tags {
            title_row = title_row.child(Badge::new(t.clone()).variant(BadgeVariant::Subtle).build(k));
        }
        if s.inferred {
            title_row = title_row.child(Badge::new("new").variant(BadgeVariant::Accent).build(k));
        }

        let meta_line = format!("{} · {}", s.key, self.editor.default_note(s));
        let mut label_col = col()
            .key(format!("{}:label", s.key))
            .grow(1.0)
            .min_w(0.0)
            .gap(4.0)
            .child(title_row)
            .child(text(meta_line).font(k.ty.mono).color(k.c.text_faint).wrap());

        if let Some(doc) = &s.doc {
            label_col = label_col.child(text(doc.replace('`', "")).font(k.ty.text_small).color(k.c.text_muted).wrap());
        }
        if let Some(note) = &s.built_ins_note {
            label_col = label_col.child(
                text(format!("Lines here are added to {}'s {note}.", self.app.terminal))
                    .font(k.ty.text_small)
                    .color(k.c.text_muted)
                    .wrap(),
            );
        }
        if let Some(more) = &s.doc_more {
            let open = self.expanded.contains(&s.key);
            let key = s.key.clone();
            label_col = label_col.child(
                row().child(
                    Button::new(if open { "Less" } else { "More" })
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .on_press(move |app: &mut Self| {
                            if !app.expanded.remove(&key) {
                                app.expanded.insert(key.clone());
                            }
                        })
                        .build(k)
                        .key(format!("{}:more", s.key)),
                ),
            );
            if open {
                label_col = label_col.child(
                    text(more.replace('`', ""))
                        .key(format!("{}:doc", s.key))
                        .font(k.ty.text_small)
                        .color(k.c.text_muted)
                        .wrap(),
                );
            }
        }
        if let Some(err) = &error {
            label_col = label_col.child(
                text(err.clone()).key(format!("{}:error", s.key)).font(k.ty.text_small).color(k.c.danger.fg).wrap(),
            );
        }

        let reset_slot = if dirty || set {
            let key = s.key.clone();
            let (ic, tip) = if dirty {
                (&icons::UNDO, "Undo this change")
            } else {
                (&glyphs::CIRCLE_X, "Reset to default (removes the line from the file)")
            };
            row().w(28.0).justify_end().child(
                IconButton::new(ic)
                    .variant(IconButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .label(tip)
                    .help(tip)
                    .disabled(disabled)
                    .on_press(move |app: &mut Self| app.reset(&key))
                    .build(k)
                    .key(format!("{}:reset", s.key)),
            )
        } else {
            row().w(28.0)
        };

        let control = self.render_control(s, disabled, k, layer);
        let body = if wide {
            col()
                .w_full()
                .gap(10.0)
                .child(row().w_full().gap(12.0).items_start().child(label_col).child(reset_slot))
                .child(control)
        } else {
            row().w_full().gap(16.0).items_start().child(label_col).child(control).child(reset_slot)
        };

        let bar_color = if dirty {
            k.c.bg_accent
        } else if error.is_some() {
            k.c.danger.fg
        } else {
            Color::TRANSPARENT
        };

        let mut r = row()
            .key(format!("row:{}", s.key))
            .w_full()
            .pr(16.0)
            .py(12.0)
            .gap(12.0)
            .child(el().w(3.0).h_full().radius(1.5).fill(bar_color).a11y_hidden())
            .child(col().grow(1.0).min_w(0.0).child(body));
        if disabled {
            r = r.opacity(0.55);
        }
        match lock {
            Some(Lock::Scope) => r = r.help("This setting can't be overridden per platform"),
            Some(Lock::Theme) => r = r.help(format!("{} ignores colors while a theme is set", self.app.terminal)),
            None => {}
        }
        r
    }

    fn render_colors_extras(&self, k: &Kit) -> El {
        let color = |name: &str| self.editor.color(name).and_then(|h| parse_hex_color(&h));
        let fg = color("foreground").unwrap_or(k.c.text_base);
        let bg = color("background").unwrap_or(k.c.bg_deep);
        let c = |name: &str| color(name).unwrap_or(fg);
        let span = |name: &str, s: &str| text(s.to_string()).font(k.ty.mono).color(c(name));
        let swatches = |prefix: &str| {
            let mut r = row().gap(6.0);
            for a in ANSI {
                r = r.child(
                    el().size(32.0, 20.0)
                        .radius(k.radius.sm)
                        .fill(c(&format!("{prefix}{a}")))
                        .border(1.0, k.c.border_weak),
                );
            }
            r
        };
        let locked = self.editor.schema.theme_locks_colors && self.editor.theme_active();
        let mut out = col().key("colors-extras").w_full().gap(12.0);
        if locked {
            let clear_btn = Button::new("Clear theme")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Regular)
                .on_press(|app: &mut Self| {
                    app.set_and_sync("theme", Value::Null);
                    app.active_group = "theme".into();
                })
                .build(k)
                .key("clear-theme");
            out = out.child(self.alert_box(
                "colors-locked",
                format!("{} ignores these colors while a theme is set. Clear the theme to use them.", self.app.terminal),
                k.c.info.bg,
                k.c.info.border,
                k.c.text_base,
                Some(clear_btn),
                k,
            ));
        }
        let sel_bg = color("selection-background").unwrap_or(k.c.layer_03);
        let sel_fg = color("selection-foreground").unwrap_or(fg);
        out.child(
            col()
                .key("terminal-preview")
                .w_full()
                .padding(16.0)
                .gap(8.0)
                .radius(k.radius.lg)
                .fill(bg)
                .border(1.0, k.c.border_base)
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(span("green", "➜"))
                        .child(span("cyan", "~/project"))
                        .child(row().child(span("blue", "git:(")).child(span("red", "main")).child(span("blue", ")")))
                        .child(text("ls -la").font(k.ty.mono).color(fg)),
                )
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(span("blue", "drwxr-xr-x"))
                        .child(span("yellow", "settings"))
                        .child(span("magenta", "config.toml"))
                        .child(
                            el().px(4.0).py(1.0).radius(2.0).fill(sel_bg).child(text("selected text").font(k.ty.mono).color(sel_fg)),
                        )
                        .child(el().size(8.0, 16.0).fill(c("cursor"))),
                )
                .child(swatches(""))
                .child(swatches("light-")),
        )
    }
}
