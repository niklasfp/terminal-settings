//! Themes for the theme dropdowns: the list (loaded off the UI thread, since an app may ask its
//! terminal for it) and choosing a theme that still has to be downloaded.

use crate::dropdowns::ThemeItem;
use crate::view::SettingsWindow;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;
use serde_json::Value;
use settings_core::SettingType;
use std::rc::Rc;

impl SettingsWindow {
    /// Loads the theme list in the background, then refreshes every theme dropdown.
    pub(crate) fn load_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.app.themes else { return };
        let path = self.document.path();
        let task = cx.background_executor().spawn(async move { list(&path) });
        cx.spawn_in(window, async move |this, cx| {
            let themes = task.await;
            this.update_in(cx, |this, window, cx| {
                this.themes = Rc::new(themes.into_iter().map(ThemeItem::new).collect());
                let keys: Vec<String> =
                    this.editor.schema.settings.iter().filter(|s| s.ty == SettingType::Theme).map(|s| s.key.clone()).collect();
                for key in keys {
                    this.sync(&key, window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Loads suggestion lists (e.g. Ghostty's keybind actions) in the background.
    pub(crate) fn load_lists(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lists) = self.app.lists else { return };
        let mut names: Vec<String> = self
            .editor
            .schema
            .settings
            .iter()
            .flat_map(|s| s.fields.iter().flatten())
            .filter_map(|f| f.list.clone())
            .collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            return;
        }
        let task = cx.background_executor().spawn(async move {
            names
                .into_iter()
                .map(|name| {
                    let items: Vec<SharedString> = lists(&name).into_iter().map(SharedString::from).collect();
                    (name, items)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let loaded = task.await;
            this.update_in(cx, |this, _, cx| {
                for (name, items) in loaded {
                    this.lists.insert(name, Rc::new(items));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Makes any theme download still running for `key` stale: a later choice, a clear or a reset
    /// wins over it. Returns the new request's number.
    pub(crate) fn supersede_theme_request(&mut self, key: &str) -> u64 {
        let n = self.theme_requests.entry(key.to_string()).or_default();
        *n += 1;
        *n
    }

    /// A theme was picked (or cleared, `None`): use it, downloading it first if it isn't installed.
    pub(crate) fn choose_theme(&mut self, key: &str, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let request = self.supersede_theme_request(key);
        let Some(name) = name else {
            self.edit(key, Value::Null, window, cx);
            return;
        };
        let installed = self.themes.iter().find(|t| t.info.name == name).is_none_or(|t| t.info.installed);
        if installed {
            self.edit(key, Value::String(name), window, cx);
            return;
        }
        let Some(install) = self.app.install_theme else { return };
        let path = self.document.path();
        let key = key.to_string();
        window.push_notification(format!("Downloading “{name}”…"), cx);
        let task = cx.background_executor().spawn({
            let name = name.clone();
            async move { install(&path, &name) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                // A choice made while this downloaded wins; the file is installed either way.
                let current = this.theme_requests.get(&key) == Some(&request);
                match result {
                    Ok(()) => {
                        if current {
                            this.edit(&key, Value::String(name), window, cx);
                        }
                        this.load_themes(window, cx);
                    }
                    Err(message) => {
                        window.push_notification(Notification::error(message), cx);
                        if current {
                            // The dropdown already shows the theme; put it back to the setting's value.
                            this.sync(&key, window, cx);
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }
}
