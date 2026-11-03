# AGENTS.md

- The apps live in `editor/`. Their rules and commands are in [editor/AGENTS.md](editor/AGENTS.md)
  and [editor/gpui/AGENTS.md](editor/gpui/AGENTS.md).
- The editors default to the user's live configs (`~/.config/rio/config.toml`,
  `~/Library/Application Support/com.mitchellh.ghostty/config[.ghostty]`,
  `~/.config/ghostty/config[.ghostty]`). Test against a copy with `--config`, not those files.
