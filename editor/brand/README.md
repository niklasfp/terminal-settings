# Brand assets

Logos and palettes of the terminals each editor configures. Each editor embeds only its own
logo. The palettes in `gpui/apps/<app>/<terminal>-theme.json` were taken from the projects'
websites.

| File | Source | License |
|---|---|---|
| `rio-logo.svg` | [raphamorim/rio](https://github.com/raphamorim/rio) `misc/logo-2024.svg` | MIT |
| `ghostty-logo.png` | [ghostty-org/ghostty](https://github.com/ghostty-org/ghostty) `images/icons/icon_32@2x.png` | MIT |
| `rio-settings-icon.svg` | Ours, drawn by `tools/gpui/make-icons.ts`: Rio's colors, its eyes as slider knobs | MIT |
| `ghostty-settings-icon.svg` | Ours, drawn by `tools/gpui/make-icons.ts`: Ghostty's colors, a ghost wearing a gear | MIT |
| `terminal-settings-icon.svg` | Ours, drawn by `tools/gpui/make-icons.ts`: for a single app that edits both terminals | MIT |

`rio-settings-icon.ico` and `ghostty-settings-icon.ico` are the same icons for Windows (generated
by the same script, embedded by `gpui/apps/app-build.rs`).

The two `*-settings-icon.svg` files are the editors' app icons. They're deliberately not the
terminals' own icons, so the editors can't be mistaken for the terminals in the Dock or Finder.

Palettes:
- **Rio** ([rioterm.com](https://rioterm.com)): fuchsia accent `#c026d3` (light) / `#e879f9` (dark)
  on zinc neutrals (`#18181b`, `#e4e4e7`), with Noto Sans.
- **Ghostty** ([ghostty.org](https://ghostty.org)): brand blue `#3551f3` on the site's gray
  scale (`--gray-0` `#0f0f11` … `--gray-9` `#f0f0f4`, inverted for light as in its print
  stylesheet), with JetBrains Mono.
