//! The settings window shared by every terminal's editor, built on Rune (`rune` + `rune-kit`).
//! A terminal's binary is `settings_rune::run(its AppDefinition, its Brand)`; nothing here is
//! terminal-specific.

pub mod controls;
pub mod icons;
#[path = "../../../../gpui/crates/settings-ui/src/model.rs"]
pub mod model;
pub mod state;
pub mod view;

use rune::{AppSpec, Chrome};
use settings_core::AppDefinition;
use std::path::PathBuf;
use std::sync::OnceLock;
use view::{Appearance, SettingsWindow};

/// The release workflow stamps the tag's version in through `SETTINGS_VERSION` at build time;
/// local builds report the crate version.
pub const VERSION: &str = match option_env!("SETTINGS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The four core brand colours for one appearance (light or dark), derived into a full `rune_kit::Kit`
/// via `Kit::from_palette`.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: &'static str,
    pub fg: &'static str,
    pub accent: &'static str,
    pub sidebar: &'static str,
}

/// A terminal's look: its light and dark palettes and its logo bytes (SVG or PNG).
#[derive(Clone, Copy)]
pub struct Brand {
    pub light: Palette,
    pub dark: Palette,
    pub logo: &'static [u8],
}

struct LaunchContext {
    app: &'static AppDefinition,
    brand: Brand,
    config_path: PathBuf,
    appearance_file: PathBuf,
}

static LAUNCH: OnceLock<LaunchContext> = OnceLock::new();

fn appearance_path(app_name: &str) -> PathBuf {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        let base = env("LOCALAPPDATA")
            .or_else(|| std::env::home_dir().map(|h| h.join("AppData").join("Local")))
            .unwrap_or_default();
        return base.join(app_name).join("appearance");
    }
    let base = env("XDG_STATE_HOME")
        .or_else(|| env("XDG_CONFIG_HOME"))
        .or_else(|| std::env::home_dir().map(|h| h.join(".config")))
        .unwrap_or_default();
    base.join(app_name).join("appearance")
}

fn parse_args(app: &AppDefinition) -> Option<PathBuf> {
    let args = rune::extra_args();
    let mut path: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("{} {VERSION}", app.name);
                return None;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: {} [--config <path>]\n\nNative settings editor for {} (Rune UI spike).",
                    app.name, app.terminal
                );
                return None;
            }
            "--config" => {
                let Some(p) = it.next() else {
                    eprintln!("{}: --config requires a file path", app.name);
                    std::process::exit(2);
                };
                path = Some(PathBuf::from(p));
            }
            other if other.starts_with("--config=") => {
                path = Some(PathBuf::from(&other["--config=".len()..]));
            }
            other => {
                eprintln!("{}: unknown option '{other}' (try --help)", app.name);
                std::process::exit(2);
            }
        }
    }
    Some(path.unwrap_or_else(|| (app.default_config_path)()))
}

fn init_window() -> SettingsWindow {
    let ctx = LAUNCH.get().expect("launch context initialized before rune::run");
    SettingsWindow::new(
        ctx.app,
        ctx.brand,
        ctx.config_path.clone(),
        Some(ctx.appearance_file.clone()),
    )
}

/// Parses the command line and runs the settings window (or Rune's headless `--snap` / `--tree`
/// tooling) until it closes.
pub fn run(app: AppDefinition, brand: Brand) {
    let app: &'static AppDefinition = Box::leak(Box::new(app));
    let Some(config_path) = parse_args(app) else { return };
    let appearance_file = appearance_path(app.name);
    let title: &'static str = Box::leak(app.schema.title.clone().into_boxed_str());
    rune::set_app_id(app.name);
    let _ = LAUNCH.set(LaunchContext {
        app,
        brand,
        config_path,
        appearance_file,
    });

    let chrome = Chrome {
        full_size_content: true,
        traffic_lights: Some((16.0, 23.0)),
        drag_regions: true,
    };
    let spec = AppSpec::<SettingsWindow> {
        min_size: Some((720.0, 480.0)),
        scenes: &[
            ("default", |_| {}),
            ("light", |s| s.appearance = Appearance::Light),
            ("dark", |s| s.appearance = Appearance::Dark),
            ("colors", |s| {
                if s.editor.schema.groups.iter().any(|g| g.id == "colors") {
                    s.active_group = "colors".into();
                }
            }),
            ("window", |s| {
                if s.editor.schema.groups.iter().any(|g| g.id == "window") {
                    s.active_group = "window".into();
                }
            }),
        ],
        setup: Some(|s, waker| s.waker = Some(waker)),
        poll: Some(|s, now, _| s.poll_async(now)),
        on_key: Some(|s, ev, tx| s.on_key(ev, tx)),
        ..AppSpec::new(title, (1180.0, 820.0), init_window, SettingsWindow::view)
    };
    rune::App::new(spec).chrome(chrome).run();
}
