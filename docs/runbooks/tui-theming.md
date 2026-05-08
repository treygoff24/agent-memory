# Memorum TUI theming

The TUI loads `default-warm-dark` unless overridden.

## Flags

- `--theme <name>`: one of `default-warm-dark`, `default-light`, `kanagawa`, `gruvbox-dark`, `catppuccin-mocha`, `tokyo-night`.
- `--theme-config <path>`: load a TOML theme file. The default path is `~/.config/memorum/theme.toml` when it exists.
- `--charset <full|extended|minimal>`: force glyph support. Use `minimal` for ASCII-only terminals.
- `--no-motion`: disable transitions.
- `--color-capability <truecolor|256|16|mono>`: force color floor.

Resolution precedence: CLI flags override `MEMORUM_FORCE_COLOR`; env overrides auto-detection.

## Custom theme TOML

A theme declares top-level `name`, `borders`, `density`, `[colors]`, `[glyphs]`, and `[motion]` sections. The color tokens use OKLCH strings and are validated strictly; unknown tokens or missing required color tokens fail loudly.

## Fonts and glyphs

Recommended fonts: JetBrains Mono, Berkeley Mono, MonoLisa, Iosevka, and Cascadia Code. If glyphs render as boxes or question marks, use `--charset minimal`; Memorum will force ASCII glyphs and plain borders.

## Hot reload

When `--theme-config` points at a file, valid edits are picked up by the hot-reload watcher. Invalid TOML is rejected; the last valid theme stays active and the reload error is retained for display.

## Common misdetection fixes

- tmux reports `screen-256color` on a true-color host: use `--color-capability truecolor` or `MEMORUM_FORCE_COLOR=truecolor`.
- SSH/dumb terminals show broken glyphs: use `--charset minimal`.
- Accent is invisible on a monochrome palette: try `--theme default-light` or author a custom accent token.
