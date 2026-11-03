# AGENTS.md

The GPUI Kit port of the settings editors. See README.md for layout and commands. The rules in
`../AGENTS.md` about config edits still apply here: splice into exact spans, re-parse after every
edit, TOML 1.0 output only, atomic writes through symlinks, and nothing terminal-specific in the
core.

## Standing rules

- `settings-core` and `settings-ui` stay terminal-agnostic. Terminal specifics live in
  `rio-config` / `ghostty-config`, the overlay, or the app's `Brand`.
- Anything both terminals do is written once and shared; only what differs by file format stays
  in a terminal crate. Every control lives in `settings-ui`. Schema-merge plumbing (diagnostics,
  `MergedSchema`, `humanize`, JSON accessors) is in `settings_core::merge`. Value checks and their
  messages (`boolean`, `int`, `float`, `items`, `number`, `check_range`) are in
  `settings_core::values`. Theme folder and ordering are `settings_core::themes_dir` /
  `theme_list`. If you're about to write the same thing in the second terminal crate, move it
  into core instead.
- The schema files in `editor/schema/` are shared with the .NET apps. Don't fork them here.
- New widget types: describe them in the overlay, then update `SettingType`, the Rio value writer
  (`rio-config/src/values.rs`), and `create_control` / `on_input` / `sync` / `render_control` in
  `crates/settings-ui/src/controls.rs` (and Ghostty's writer in `ghostty-config`).
- UI code reads colors from `cx.theme()`. The brand palettes live in
  `apps/<app>/<terminal>-theme.json`, passed in through `settings_ui::Brand`. The only raw colors in Rust are the user's own
  terminal colors (preview, theme swatches).
- Before using a GPUI Kit API, look up its signature in the crate source
  (`~/.cargo/registry/src/*/gpui-component-0.9.0`, `gpui-base-0.9.0`). Don't guess from other UI
  frameworks.
- Widget events feed the `Editor` model through `on_input`. `sync` pushes model values back into
  widgets. Both must be idempotent: a synced value that echoes back as an event must not produce
  a pending change.
- Choosing from a known list (fonts, themes, long flag lists) goes through
  `dropdowns::Chooser`, in settings and in table columns alike, so both apps show one control. Don't
  add another picker; give the item type a `ChoiceItem` impl instead. Every file or folder setting
  (the overlay's `pick`) goes through `paths.rs` the same way.
- Store calls that can be slow (validation runs the terminal, the after-save step may run
  `osascript`) go through `SettingsWindow::in_background`, never on the UI thread.
- Test modules that need GPUI must not `use gpui_kit::*`, because it shadows `#[test]`.

## Gotchas

- `SliderState::min`/`max` each re-clamp the value with `clamp(min, max)`, which panics while
  min > max mid-chain (the default range is 0..100). Widen before narrowing.
- `InputState::set_value`, `SliderState::set_value` and `ColorPickerState::set_value` emit no
  events. When a widget writes into another widget, call `on_input` yourself.
- `Colorize::to_hex` truncates, so hex values drift through HSL. Use `controls::to_hex`.
- `Select` and `Combobox` render their root at `size_full()`, and a width set on them only sizes
  the inner part. In a row that squeezes everything else (a label one letter wide). Put the
  width on a wrapper `div` with `flex_none()`.
- Scroll areas get no scroll bar from `overflow_y_scroll()`. Wrap them in a `relative()` div and
  add `.vertical_scrollbar(&handle)` after the scroll area, or use `overflow_y_scrollbar()`.
- With `test-support`, Cargo resolves the weak `gpui-fast` deps, which pin exact versions (e.g.
  `taffy =0.15.0`). If resolution fails after adding a dependency, regenerate `Cargo.lock`
  instead of fighting individual pins.
- GPUI rasterizes an `img()` SVG at its declared `width`/`height` times the display scale. The
  shared `brand/rio-logo.svg` says 3000×3000, which cost ~290 MB (RAM + GPU), so `assets.rs`
  strips those attributes when serving it. Check the declared size of any new SVG image.
