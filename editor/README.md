# Developing the settings editors

This is the developer guide. For what the apps do and how to install them, see the
[top-level README](../README.md).

Each editor is a single Native AOT binary. It starts a loopback web server and serves a
settings page that is generated, at build time, from the terminal's own source.

| App | Edits | Schema comes from |
|---|---|---|
| `rio-settings` | Rio's `config.toml` | `rio-backend`'s `Config::default()` (Rust dump) + enums/colors scraped from Rio's source |
| `ghostty-settings` | Ghostty's `config.ghostty` / `config` | `ghostty +show-config --default --docs` + `src/config/Config.zig` |

Both editors share the same behaviour:
- **Search and grouping:** every option the terminal has, grouped and searchable, with its own docs.
- **Minimal edits:** saving touches only the lines you changed, so comments and formatting stay.
  A diff preview shows exactly what will change.
- **Light and dark:** follows the system by default, with a System / Light / Dark switch.
- **External edits:** notices when the file changes elsewhere and never overwrites it silently.
- **Themes:** a picker with color previews. Rio ships no themes, so its editor offers the 889
  Rio-format themes from mbadolato/iTerm2-Color-Schemes (MIT) and downloads the one you pick into
  `themes/`. Ghostty lists its 819 bundled themes.
- **Rio:** reloads by itself when the file changes. The editor also offers per-platform overrides
  (`[platform.macos.window]` and so on) through "Applies to".
- **Ghostty:** every save is checked by Ghostty's own `+validate-config` first. After saving,
  the editor asks a running Ghostty to reload via AppleScript (`reload_config`). On Linux,
  press Ctrl+Shift+, instead.

## Use

```sh
dotnet publish src/RioSettings -c Release -o out       # → out/rio-settings
dotnet publish src/GhosttySettings -c Release -o out   # → out/ghostty-settings
./out/ghostty-settings                                 # opens the browser
./out/rio-settings --config path/to/config.toml --no-open --port 5299
```

The server listens on `127.0.0.1` only. Every API call needs the random token from the URL it
prints. It stops a few minutes after the browser tab closes, unless `--stay` is given.

## Layout

```
src/Settings.Core/        shared: web server, save/diff/conflict flow, setting types, UI runtime (wwwroot)
src/Settings.Generators/  Roslyn source generator: <terminal>-source.json + <terminal>-overlay.json
                          → AppSchema (descriptors) + the complete settings page
src/RioSettings/          Rio adapter: TOML format (Tomlyn spans), paths, themes
src/GhosttySettings/      Ghostty adapter: line format, +validate-config, AppleScript reload
schema/<terminal>/        generated source schema + hand-written overlay
tools/<terminal>/         scripts that regenerate the source schema
```

## Following a terminal release

```sh
node tools/rio/update-schema.ts --update   # needs cargo; builds rio-backend from git
node tools/rio/update-themes.ts            # refreshes the Rio theme catalogue (iTerm2-Color-Schemes, pinned commit)
node tools/ghostty/update-schema.ts        # needs the installed Ghostty; fetches Config.zig for its tag
dotnet build TerminalSettings.slnx         # schema drift shows up as RIO*/GHO* warnings
```

Options the overlay doesn't describe yet still show up, with a basic control.

## Develop

```sh
dotnet test --solution TerminalSettings.slnx
```

Requires the .NET 10 SDK, plus Node 22.18+ (and cargo for Rio) to regenerate schemas.

Two tests check every value the controls can produce against the terminal's own parser:
- **Ghostty:** `ghostty +validate-config`, which needs Ghostty installed.
- **Rio:** `tools/rio/schema-dump validate`, which needs `cargo build --release` in `tools/rio/schema-dump`.

Each skips itself when its tool isn't available.

### Platforms

| | macOS | Linux | Windows |
|---|---|---|---|
| rio-settings | ✓ | ✓ | ✓ |
| ghostty-settings | ✓ | ✓ | (no Ghostty for Windows) |

Platform-specific pieces live in `Settings.Core`:
- **Fonts:** CoreText on macOS, `fc-list` on Linux, the registry font list on Windows.
- **File dialogs:** AppleScript on macOS, zenity/kdialog on Linux, a WinForms dialog with a
  topmost owner on Windows (otherwise it opens behind the browser).
- **Line endings:** saves keep the file's existing endings, so CRLF files stay CRLF.

**Testing on Windows from a Mac.** Native AOT can't cross-compile, so build a self-contained
single-file exe instead and run it in a Windows VM:

```sh
dotnet publish src/RioSettings -c Release -r win-arm64 --self-contained \
  -p:PublishAot=false -p:PublishSingleFile=true -o out-win
```

The real AOT Windows builds come from the release workflow's Windows runners.

### Screenshots

`node tools/screenshots.ts` regenerates `docs/images/*.png`. It needs both apps published to
`out/` and Google Chrome. It drives headless Chrome against throwaway demo configs at 2× and
never touches your real config.

### Releasing

Each app is released on its own, and the release tag is the version: create a GitHub release
tagged `rio-settings/v1.2.3` or `ghostty-settings/v1.2.3`. The release workflow builds that app
with Native AOT for each platform and attaches the executables, archives and `SHA256SUMS`.
