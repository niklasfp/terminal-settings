# Developing the settings editors

This is the developer guide. For what the apps do and how to install them, see the
[top-level README](../README.md).

Each editor is a native desktop application built with [GPUI Kit](https://gpui-kit.com) (Rust, on
Zed's GPUI). Both terminals share one window implementation; each app is a thin binary with its own
palette, icon, and config format adapter. The settings schemas come from each terminal's own source.

| App | Edits | Schema comes from |
|---|---|---|
| `rio-settings` | Rio's `config.toml` | `rio-backend`'s `Config::default()` (Rust dump) + enums/colors scraped from Rio's source |
| `ghostty-settings` | Ghostty's `config.ghostty` / `config` | `ghostty +show-config --default --docs` + `src/config/Config.zig` |

Both editors share the same behaviour:
- **Search and grouping:** every option the terminal has, grouped into sections and searchable, with
  its own docs and "In your config" badges.
- **Minimal edits:** saving touches only the lines you changed, so comments and formatting stay.
  A diff preview shows exactly what will change.
- **Light and dark:** follows the system by default, with a System / Light / Dark choice that is
  remembered between runs.
- **External edits:** notices when the file changes elsewhere and never overwrites it silently.
- **Themes:** a searchable dropdown with color previews. Rio ships no themes, so its editor offers
  the 889 Rio-format themes from mbadolato/iTerm2-Color-Schemes (MIT) and downloads the one you pick
  into `themes/`. Ghostty lists its 819 bundled themes.
- **Rio:** reloads by itself when the file changes. The editor also offers per-platform overrides
  (`[platform.macos.window]` and so on) through "Applies to".
- **Ghostty:** every save is checked by Ghostty's own `+validate-config` first. After saving,
  the editor asks a running Ghostty to reload via AppleScript (`reload_config`) on macOS. On Linux,
  press Ctrl+Shift+, instead.

## Layout

```
gpui/
  crates/settings-core    Terminal-agnostic core: schema model, ConfigFormat seam, save flow, line diff
  crates/settings-ui      The GPUI window, shared by both apps; UI integration tests in tests/ui.rs
  crates/rio-config       Rio: schema merge, span-splicing config.toml format, value checks, themes
  crates/ghostty-config   Ghostty: schema merge, line format, +validate-config, reload, themes
  apps/rio-settings       main.rs = settings_ui::run(rio_config::app(), Rio's brand)
  apps/ghostty-settings   main.rs = settings_ui::run(ghostty_config::app(), Ghostty's brand)
  apps/settings-web       The browser playground: settings_ui::run_web for either app (?app=rio|ghostty)
  vendor/gpui-kit         gpui-kit 0.9.0 with gpui-pre's `multithreaded` default off (see its VENDORED.md)
schema/<terminal>/        Generated source schema (<terminal>-source.json) + hand-written overlay (<terminal>-overlay.json)
tools/<terminal>/         Scripts that regenerate the source schema and theme catalogue
tools/gpui/               macOS .app bundler (package-macos.ts) and app icon generator (make-icons.ts)
tools/web/                Builds the browser playground (build.ts)
brand/                    Terminal logos and app icons
```

The schemas in `schema/<terminal>/` (`*-source.json` + `*-overlay.json`) are compiled in with
`include_str!` and merged at startup.

## Run

```sh
cd gpui
cargo run -p rio-settings -- --config /tmp/copy-of-config.toml
cargo run -p ghostty-settings -- --config /tmp/copy-of-config.ghostty
```

Without `--config` they edit the live configs (`~/.config/rio/config.toml` or `$RIO_CONFIG_HOME`;
`~/Library/Application Support/com.mitchellh.ghostty/config[.ghostty]` on macOS or
`~/.config/ghostty/config[.ghostty]`). Always test against a copy.

## Web playground

`gpui/apps/settings-web` builds both editors for `wasm32-unknown-unknown` with GPUI's browser
platform (WebGPU, falling back to WebGL2, drawn into one full-page canvas). It runs
`settings_ui::run_web`: the config is text in memory (`settings_core::Document::Memory`), **Open…**
reads a file the user picks, saving downloads the result, and `AppDefinition::for_browser` drops
what needs the system (the installed-theme lists and installing, Ghostty's validation and suggestion
lists, the reload). `rio_config::browser_app` and `ghostty_config::browser_app` add their built-in
theme lists back: Rio's catalogue (picking one downloads its file through the `Playground`'s
`install_theme`) and the themes Ghostty ships (`schema/ghostty/ghostty-themes.json`). The browser side (file picker, download) comes in through `settings_ui::playground::Playground`,
so `crates/settings-ui/tests/playground.rs` tests the same window natively.

Build and serve it locally (needs the wasm target and `wasm-bindgen-cli` at the version of the
`wasm-bindgen` crate in `gpui/Cargo.lock`; the script checks and says what to install):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <version in Cargo.lock> --locked
node tools/web/build.ts            # or --dev for a quicker, bigger debug build
python3 -m http.server -d gpui/target/web 8000
```

Then open <http://localhost:8000/?app=rio> or `?app=ghostty` (no `?app` shows a small landing
page). The page fetches its fonts from jsDelivr, so it needs a network connection.

It runs single-threaded. gpui-pre's `multithreaded` feature needs nightly Rust and cross-origin
isolation headers, which GitHub Pages can't send, so `vendor/gpui-kit` turns it off.

**Deploy:** `.github/workflows/pages.yml` builds the site with the same script on every push to
`main` that touches the editor, and deploys it to GitHub Pages
(<https://niklasfp.github.io/terminal-settings/>). The `web` job in `ci.yml` builds it on every
pull request, so a change that breaks the wasm build fails CI.

## Following a terminal release

`.github/workflows/upstream-schema.yml` checks nightly whether Rio (`raphamorim/rio` GitHub
Releases) or Ghostty (`ghostty-org/ghostty` `vX.Y.Z` tags) has shipped a release — and on Mondays
whether `mbadolato/iTerm2-Color-Schemes` has updated `rio/` — and proposes the regenerated schema as
a draft PR on `schema/rio`, `schema/ghostty`, or `schema/rio-themes`:

1. **Detect (`ubuntu-latest`, read-only):** compares the latest upstream release tag against
   `rioVersion` in `schema/rio/rio-source.json` and `ghosttyVersion` in
   `schema/ghostty/ghostty-source.json` (skipping any version that already has an open PR).
2. **Regenerate (`contents: read`, no secrets):** builds `tools/rio/schema-dump` for the release tag
   on Linux or downloads and verifies the signed `Ghostty.dmg` (`com.mitchellh.ghostty`, Team ID
   `24VZTF6M5V`) on macOS, regenerates `*-source.json`, runs `cargo test -p <crate> --locked` to
   collect merge diagnostics (`RIO001…` / `GHO001…`) and widget-value checks, and uploads an
   artifact bundle.
3. **Open or update the draft PR (`contents: write, pull-requests: write`):** commits only the
   allowlisted schema files via GitHub's GraphQL `createCommitOnBranch` API (so commits are signed)
   and opens or updates the draft PR with the settings diff table, diagnostics, test results, and
   reviewer checklist. Set `APP_ID` + `APP_PRIVATE_KEY` (for `actions/create-github-app-token`) or
   `SCHEMA_BOT_TOKEN` in repository secrets so the PR triggers `ci.yml`; otherwise it falls back to
   `GITHUB_TOKEN`.
4. **Failure reporting:** if detection or regeneration fails, the workflow opens or comments on an
   issue titled `Upstream schema update failed for <terminal> <version>`.

You can also trigger the workflow manually (`workflow_dispatch`) with `rio`, `ghostty`, `themes`,
or `dry_run`, or run the generators locally:

```sh
node tools/rio/update-schema.ts --rio v0.5.29   # pins tools/rio/schema-dump/Cargo.toml to the release tag
node tools/rio/update-themes.ts                 # refreshes the Rio theme catalogue (iTerm2-Color-Schemes)
node tools/ghostty/update-schema.ts             # needs the installed Ghostty; fetches Config.zig for its tag
cargo test --manifest-path gpui/Cargo.toml      # schema drift fails real_schema_files_have_no_drift
```

Options the overlay doesn't describe yet still show up, with a basic control.

## Test and lint

```sh
cd gpui
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Requires Rust (stable) and Node 22.18+ for the TypeScript scripts in `tools/`.

`crates/settings-ui/tests/ui.rs` drives the real window headlessly (clicks, typing, dropdowns,
action suggestions, the save dialog) and checks the file on disk.

Two tests check every value the controls can produce against the terminal's own parser:
- **Ghostty:** `ghostty +validate-config`, which needs Ghostty installed (`crates/ghostty-config/tests/widget_values.rs`).
- **Rio:** `tools/rio/schema-dump validate`, which needs `cargo build --release` in `tools/rio/schema-dump` (`crates/rio-config/tests/widget_values.rs`).

Each skips itself when its tool isn't available.

## Platforms

| | macOS | Linux | Windows |
|---|---|---|---|
| rio-settings | ✓ | ✓ | ✓ |
| ghostty-settings | ✓ | ✓ | (no Ghostty for Windows) |

Building on Linux needs GPUI's system libraries, e.g. on Debian/Ubuntu:

```sh
sudo apt-get install pkg-config libfontconfig-dev libfreetype-dev libwayland-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev libzstd-dev libssl-dev
```

## Package for macOS

```sh
node tools/gpui/package-macos.ts
```

This builds both apps with the `dist` profile (release plus whole-program optimisation and no
symbol table: ~17 MB per app instead of ~30 MB, same startup; a clean build takes ~3 minutes)
and writes `gpui/target/macos/Rio Settings.app`, `Ghostty Settings.app` and a zip of the two (or
pass `--app rio-settings` / `--app ghostty-settings` to build and zip just one as
`<app>-macos-<arch>.zip`). The bundles open from Finder like any app, with their name in the menu
bar and their icon in the Dock. They're ad-hoc signed only (not notarized), so on another Mac
Gatekeeper blocks the first launch: click Done, then System Settings → Privacy & Security → Open
Anyway, or run `xattr -dr com.apple.quarantine "Rio Settings.app"` once. To open a bundle on a
copy: `open "Rio Settings.app" --args --config <copy>`.

## Screenshots

`node tools/screenshots.ts` regenerates `docs/images/*.png` using throwaway demo configs and never
touches your real config.

## Releasing

Each app is released on its own, and the tag is the version. To release, push a tag:

```sh
git tag rio-settings/v1.2.3 && git push origin rio-settings/v1.2.3
```

The release workflow then:
1. builds that app with the `dist` profile for each platform (`.app` zip on macOS, `.tar.gz` on
   Linux, `.zip` on Windows),
2. creates the GitHub release as a draft with the archives and `SHA256SUMS` attached,
3. publishes it.

The repo has immutable releases on, so a published release can't be changed. To fix a release,
push a new version.

- **Release notes:** `release-notes/<app>/<version>.md` if it exists, else the annotated tag's
  message, else GitHub's generated notes.
- **Pre-releases:** a suffix such as `1.2.3-alpha.1` marks the release as a pre-release.
- **Dry run:** run the workflow by hand (`workflow_dispatch`) with a tag to build everything
  without releasing.
