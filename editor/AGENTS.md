# AGENTS.md

Guidance for coding agents working in this repo. See README.md for what the apps do.

## Layout

- `src/Settings.Core`: shared library. It contains `SettingsServer` (the ASP.NET Core minimal
  API on `CreateSlimBuilder`), `ConfigStore` (the save/diff/conflict flow and atomic writes),
  `Setting`/`SettingCatalog`, and the UI runtime in `wwwroot/` (app.css, app.js), embedded as
  resources.
- `src/Settings.Generators`: netstandard2.0 incremental generator. It picks the merger from the
  AdditionalFiles names (`rio-*` → `RioSchemaMerger`, `ghostty-*` → `GhosttySchemaMerger`) and
  emits `Settings.Generated.AppSchema` (descriptors + `Html`). It uses its own `MiniJson` because
  generators can't easily ship System.Text.Json.
- `src/RioSettings`, `src/GhosttySettings`: one `PublishAot` executable each. An app is just an
  `AppDefinition` (format, schema, paths, optional validate/after-save hooks) plus its
  `IConfigFormat` implementation.
- `schema/<terminal>/<terminal>-source.json`: **generated** by `node tools/<terminal>/update-schema.ts`.
  Never hand-edit it.
- `schema/<terminal>/<terminal>-overlay.json`: the hand-written layer (groups, ranges, widgets,
  labels where needed).
- `tests/<App>.Tests`: TUnit (Microsoft.Testing.Platform, see `global.json`), one project per app.

## Standing rules

- The two apps ship separately. Nothing terminal-specific belongs in `Settings.Core`. Put it in
  the app's `AppDefinition`/format or in its overlay (e.g. `app.themeLocksColors`, `app.scopes`).
- Native AOT must stay clean: warnings are errors, so any IL2xxx/IL3xxx fails the build.
  Don't use reflection-based JSON. Build responses with `JsonNode` and `ToJsonString()`;
  for `JsonArray.Add` pass a `JsonNode`, not a generic `T`.
- Edits must preserve everything they don't touch:
  - Rio: splice text into the exact TOML syntax span and re-parse after every edit.
  - Ghostty: replace whole `key = value` lines (the last one for scalars, all of them for
    repeatable keys, in place).
  - Never re-serialise a whole file.
- Rio: only emit TOML 1.0 syntax (Rio uses the Rust `toml` crate, while Tomlyn parses 1.1). So no
  multi-line inline tables, and no `\e` escape (use `\u001B`).
- Ghostty: never trim values when writing (`selection-word-chars` starts with a space), and quote
  values with leading or trailing whitespace. Every save must pass Ghostty's `+validate-config`.
  The AppleScript reload must never launch Ghostty.
- Writes go through `ConfigStore.WriteAtomically`, which resolves symlinks first, so a symlinked
  config is never replaced by a regular file.
- New options: describe them in the overlay. Don't special-case keys in C# or JS. Widgets are
  picked by `type`. A new widget type means updating `SchemaMerger.KnownTypes`,
  `HtmlEmitter.EmitWidget`, `SettingType`, each app's value writer, and `controllerFor` in app.js.
- The server binds to 127.0.0.1 only and checks the `Host` header and the per-run token.
  Keep both.
- UI colors come only from CSS custom properties using `light-dark()`. Never hardcode a
  color in generated markup or JS, except user-chosen terminal colors in the preview.
- Scripts are TypeScript run with Node's native type stripping (`node file.ts`). No Python,
  no tsx.

## Commands

```sh
dotnet build TerminalSettings.slnx
dotnet test --solution TerminalSettings.slnx
dotnet publish src/RioSettings -c Release -o out
dotnet publish src/GhosttySettings -c Release -o out
node tools/rio/update-schema.ts [--update]
node tools/ghostty/update-schema.ts
```

`dotnet test --nologo` runs zero tests on .NET 10 MTP; don't pass `--nologo`.
