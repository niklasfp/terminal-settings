# terminal-settings on GPUI Kit

A native port of the settings editors to [GPUI Kit](https://gpui-kit.com) (Rust, built on Zed's
GPUI). It does the same job as the .NET apps in `editor/src`. Instead of opening a page in the
browser through a local server, it is a desktop window.

Both terminals share one window implementation; each app is a thin binary with its own palette and logo.

| | What it adds |
|---|---|
| **rio-settings** | Per-platform overrides, theme download, TOML that keeps everything you didn't touch |
| **ghostty-settings** | Every save is checked by `ghostty +validate-config`, then Ghostty is asked to reload (macOS) |

## Layout

```
editor/gpui/
  crates/settings-core    Terminal-agnostic core: schema model, ConfigFormat seam, save flow, line diff
  crates/settings-ui      The GPUI window, shared by both apps; UI integration tests in tests/ui.rs
  crates/rio-config       Rio: schema merge, span-splicing config.toml format, value checks, themes
  crates/ghostty-config   Ghostty: schema merge, line format, +validate-config, reload, themes
  apps/rio-settings       main.rs = settings_ui::run(rio_config::app(), Rio's brand)
  apps/ghostty-settings   main.rs = settings_ui::run(ghostty_config::app(), Ghostty's brand)
```

The schemas still come from `editor/schema/<terminal>/` (`*-source.json` + `*-overlay.json`),
compiled in with `include_str!` and merged at startup. The .NET source generator merged it at build time
instead.

## Run

```sh
cd editor/gpui
cargo run -p rio-settings -- --config /tmp/copy-of-config.toml
cargo run -p ghostty-settings -- --config /tmp/copy-of-config.ghostty
```

Without `--config` they edit the live configs (`~/.config/rio/config.toml` or `$RIO_CONFIG_HOME`;
`~/.config/ghostty/config.ghostty`, or the older `config`). Test against a copy.

## Platforms

The same as the .NET apps: Rio Settings for macOS arm64, Linux x64/arm64 and Windows x64/arm64;
Ghostty Settings for macOS arm64 and Linux x64/arm64 (Ghostty has no Windows version). CI lints
and tests on Linux, macOS and Windows and smoke-tests a release build per app and platform; the
release workflow builds every target natively and attaches `<app>-gpui-<version>-<rid>` assets
next to the .NET ones (a zipped `.app` on macOS).

Building on Linux needs GPUI's system libraries, e.g. on Debian/Ubuntu:

```sh
sudo apt-get install pkg-config libfontconfig-dev libfreetype-dev libwayland-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev libzstd-dev libssl-dev
```

## Package for macOS

```sh
cd editor
node tools/gpui/package-macos.ts
```

This builds both apps with the `dist` profile (release plus whole-program optimisation and no
symbol table: ~17 MB per app instead of ~30 MB, same startup; a clean build takes ~3 minutes)
and writes `gpui/target/macos/Rio Settings.app`,
`Ghostty Settings.app` and a zip of the two. The bundles open from Finder like any app (a bare
executable opens a Terminal window), with their name in the menu bar and their icon in the Dock.
They're ad-hoc signed only, so on another Mac run `xattr -cr "Rio Settings.app"` once (or
right-click → Open). To open a bundle on a copy: `open "Rio Settings.app" --args --config <copy>`.

## Test

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
```

`crates/settings-ui/tests/ui.rs` drives the real window headlessly (clicks, typing, the save dialog)
and checks the file on disk.

## What's different from the browser version

- Path settings are a text field with its own small buttons: choose (the native file dialog; the
  browser version needed its own in-page picker), "Show in Finder" when the path exists, and a
  warning when it doesn't. Files and folders dragged from Finder onto the field are used too.
- One section at a time in the main pane. Searching shows matches from every section.
- Fonts, themes and long flag lists are searchable dropdowns (fonts in their own face, themes with
  their colors). Font lists are multi-select, in fallback order. A Ghostty `light:…,dark:…` theme
  pair is kept and shown, but can't be built in the dropdown.
- The light/dark choice isn't remembered between runs yet.
- No local server, token or `Host` check, because there's nothing listening.
