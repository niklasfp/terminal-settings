# Third-party notices

The editors include or derive from the following. Each keeps its own license.

## GPUI Kit / GPUI Component (Apache-2.0)
UI components, theming, and application shell (`gpui-kit`, `gpui-component`, `gpui-base`,
`gpui-component-assets`).
<https://github.com/longbridge/gpui-kit>, Copyright (c) Longbridge.

## Zed GPUI (`gpui-pre`, Apache-2.0)
GPU-accelerated desktop UI framework.
<https://github.com/zed-industries/zed>, Copyright (c) Zed Industries, Inc.

## Lucide Icons (ISC)
Icons embedded via `gpui-component-assets`.
<https://lucide.dev>, Copyright (c) Lucide Contributors (portions Copyright (c) Cole Bemis, MIT).

## toml_edit (MIT OR Apache-2.0)
Used by `rio-settings` to parse and edit `config.toml` spans without losing comments or formatting.
<https://github.com/toml-rs/toml>, Copyright (c) `toml_edit` contributors.

## ureq and rustls (MIT OR Apache-2.0 / ISC)
Used by `rio-settings` on Linux (and native OS TLS via `ureq` on macOS and Windows) to download
selected Rio themes from GitHub.
<https://github.com/algesten/ureq>, Copyright (c) Martin Algesten, Jacob Hoffman-Andrews, and
contributors.
<https://github.com/rustls/rustls>, Copyright (c) Rustls contributors.

## serde_json, sha2, tempfile, embed-resource (MIT OR Apache-2.0 / MIT)
JSON handling, SHA-256 config versioning, atomic file writes, and Windows icon embedding:
- `serde_json`: <https://github.com/serde-rs/json>, Copyright (c) Erick Tryzelaar, David Tolnay.
- `sha2`: <https://github.com/RustCrypto/hashes>, Copyright (c) RustCrypto Developers.
- `tempfile`: <https://github.com/Stebalien/tempfile>, Copyright (c) Steven Allen and contributors.
- `embed-resource`: <https://github.com/nabijaczleweli/rust-embed-resource> (MIT), Copyright (c)
  наб and contributors.

## Rio logo and configuration schema (MIT)
`editor/brand/rio-logo.svg` is Rio's `misc/logo-2024.svg`. The Rio settings schema is generated
from Rio's source (`rio-backend`). <https://github.com/raphamorim/rio>, Copyright (c) Raphael
Amorim.

## Ghostty icon and configuration docs (MIT)
`editor/brand/ghostty-logo.png` is Ghostty's `images/icons/icon_32@2x.png`. The Ghostty settings
window shows Ghostty's own option documentation (`ghostty +show-config --docs`, `Config.zig`).
<https://github.com/ghostty-org/ghostty>, Copyright (c) Mitchell Hashimoto, Ghostty contributors.

## iTerm2 Color Schemes (MIT)
The Rio theme catalogue (names and preview colors in `editor/schema/rio/rio-themes.json`) comes
from the `rio/` folder of <https://github.com/mbadolato/iTerm2-Color-Schemes>, Copyright (c) Mark
Badolato. Theme files are downloaded from that repository only when you pick one.

---

The full license texts are in each project's repository at the links above.
