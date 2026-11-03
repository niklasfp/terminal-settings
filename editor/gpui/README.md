# terminal-settings Cargo workspace

The Rust workspace for **rio-settings** and **ghostty-settings**, built on
[GPUI Kit](https://gpui-kit.com) (Zed's GPUI). For full developer documentation (schema
regeneration, packaging, screenshots, and releasing), see [editor/README.md](../README.md).

## Layout

```
crates/settings-core    Terminal-agnostic core: schema model, ConfigFormat seam, save flow, line diff
crates/settings-ui      The GPUI window, shared by both apps; UI integration tests in tests/ui.rs
crates/rio-config       Rio: schema merge, span-splicing config.toml format, value checks, themes
crates/ghostty-config   Ghostty: schema merge, line format, +validate-config, reload, themes
apps/rio-settings       main.rs = settings_ui::run(rio_config::app(), Rio's brand)
apps/ghostty-settings   main.rs = settings_ui::run(ghostty_config::app(), Ghostty's brand)
```

The schemas come from `../schema/<terminal>/` (`*-source.json` + `*-overlay.json`), compiled in with
`include_str!` and merged at startup.

## Run

```sh
cargo run -p rio-settings -- --config /tmp/copy-of-config.toml
cargo run -p ghostty-settings -- --config /tmp/copy-of-config.ghostty
```

Without `--config` they edit the live configs (`~/.config/rio/config.toml` or `$RIO_CONFIG_HOME`;
`~/Library/Application Support/com.mitchellh.ghostty/config[.ghostty]` on macOS or
`~/.config/ghostty/config[.ghostty]`). Always test against a copy.

## Test and lint

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`crates/settings-ui/tests/ui.rs` drives the real window headlessly (clicks, typing, dropdowns,
action suggestions, the save dialog) and checks the file on disk.
