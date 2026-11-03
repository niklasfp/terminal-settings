//! The settings window shared by every terminal's editor, built on GPUI Kit. A terminal's binary
//! is `settings_ui::run(its AppDefinition, its Brand)`; nothing here is terminal-specific.

pub mod assets;
mod controls;
pub mod menus;
mod model;
mod dropdowns;
mod paths;
mod themes;
pub mod view;

use gpui_kit::component::{Theme, ThemeRegistry, scroll::ScrollbarMode};
use gpui_kit::*;
use settings_core::AppDefinition;
use std::path::PathBuf;

/// The release workflow stamps the tag's version in through `SETTINGS_VERSION` at build time;
/// local builds report the crate version.
pub const VERSION: &str = match option_env!("SETTINGS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// A terminal's look: its light and dark palettes and its logo.
pub struct Brand {
    /// A GPUI Kit theme file holding the light and the dark theme below.
    pub theme_json: &'static str,
    pub light_theme: &'static str,
    pub dark_theme: &'static str,
    /// SVG or PNG bytes.
    pub logo: &'static [u8],
}

/// The logo's asset path, for the header.
pub(crate) struct LogoPath(pub &'static str);

impl Global for LogoPath {}

/// Where the user's System / Light / Dark choice is remembered between runs.
pub struct AppearanceFile(pub PathBuf);

impl Global for AppearanceFile {}

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

/// Parses the command line and runs the settings window until it's closed.
pub fn run(app: AppDefinition, brand: Brand) {
    let app: &'static AppDefinition = Box::leak(Box::new(app));
    #[cfg(windows)]
    attach_parent_console();
    let Some(path) = parse_args(app) else { return };

    gpui_kit::application().with_assets(assets::AppAssets { logo: brand.logo }).run(move |cx| {
        gpui_kit::init(cx);
        install_brand_theme(&brand, cx);
        cx.set_global(LogoPath(assets::logo_path(brand.logo)));
        let pref = appearance_path(app.name);
        let initial = menus::Appearance::load(&pref);
        cx.set_global(AppearanceFile(pref));
        view::bind_keys(cx);
        menus::bind_keys(cx);
        menus::set(&app_name(app), initial, cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(820.)), cx)),
            window_min_size: Some(size(px(720.), px(480.))),
            titlebar: Some(TitlebarOptions { title: Some(app.schema.title.clone().into()), ..Default::default() }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| view::SettingsWindow::new(app, path, window, cx)))
            .expect("failed to open the settings window");
        cx.activate(true);
        // A settings window is the whole app: closing it quits.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}

/// "Rio Settings", for the menu bar and the About box.
pub fn app_name(app: &AppDefinition) -> String {
    format!("{} Settings", app.terminal)
}

/// The config file to edit, or None when the arguments were fully handled (--help, --version).
/// Exits with status 2 on bad arguments.
/// The apps are GUI programs on Windows (no console window opens beside them), so `--help`,
/// `--version` and argument errors typed in a terminal would print nowhere. With arguments and no
/// output handle of its own, the process joins the console it was started from. A pipe or a
/// redirect (how CI reads `--version`) already is an output handle and is left alone.
#[cfg(windows)]
fn attach_parent_console() {
    use std::ffi::c_void;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(std_handle: u32) -> *mut c_void;
    }
    if std::env::args_os().len() < 2 {
        return;
    }
    // SAFETY: plain Win32 calls with constant arguments; no pointers are passed in.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out as isize == -1 {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn parse_args(app: &AppDefinition) -> Option<PathBuf> {
    let mut config: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => match args.next() {
                Some(path) => config = Some(PathBuf::from(path)),
                None => {
                    eprintln!("--config needs a file path.");
                    std::process::exit(2);
                }
            },
            "--version" => {
                println!("{} {VERSION}", app.name);
                return None;
            }
            "-h" | "--help" => {
                print!("{}", usage(app));
                return None;
            }
            // macOS may pass a process serial number when the .app is opened from Finder.
            psn if psn.starts_with("-psn_") => {}
            other => {
                eprintln!("Unknown option '{other}'.\n\n{}", usage(app));
                std::process::exit(2);
            }
        }
    }
    let path = config.unwrap_or_else(app.default_config_path);
    Some(std::path::absolute(&path).unwrap_or(path))
}

fn usage(app: &AppDefinition) -> String {
    format!(
        "{name} {VERSION}\n\nA settings window for {terminal}'s config file.\n\n\
         Usage: {name} [--config <file>]\n\n\
         Options:\n  --config <file>  Edit this file instead of {default}\n  \
         --version        Print the version\n  -h, --help       Print this help\n",
        name = app.name,
        terminal = app.terminal,
        default = (app.default_config_path)().display(),
    )
}

/// Registers the terminal's own light and dark palettes and makes them the defaults.
fn install_brand_theme(brand: &Brand, cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    if let Err(err) = registry.load_themes_from_str(brand.theme_json) {
        eprintln!("Couldn't load the brand theme: {err}");
        return;
    }
    let light = registry.themes().get(brand.light_theme).cloned();
    let dark = registry.themes().get(brand.dark_theme).cloned();
    Theme::update(cx, |theme| {
        if let Some(light) = light {
            theme.light_theme = light;
        }
        if let Some(dark) = dark {
            theme.dark_theme = dark;
        }
    });
    Theme::sync_system_appearance(None, cx);
    // Like the system: scroll bars show while scrolling, or all the time when macOS says so.
    let mode = if cx.should_auto_hide_scrollbars() { ScrollbarMode::Scrolling } else { ScrollbarMode::Always };
    Theme::set_scrollbar_mode(mode, cx);
}
