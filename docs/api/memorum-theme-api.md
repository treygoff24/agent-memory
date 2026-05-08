# Memorum Theme API

`memorum-theme` is presentation tooling. It does not change Stream G protocol payloads or MCP contracts.

## Tokens

Every theme must declare all 23 semantic color tokens: `bg`, `surface`, `surface_2`, `border`, `border_soft`, `fg`, `fg_muted`, `fg_dim`, `accent`, `accent_soft`, `status_ok`, `status_warn`, `status_bad`, `status_info`, `glyph_review`, `glyph_recall`, `glyph_conflict`, `glyph_dream`, `glyph_due`, `glyph_memory`, `selection_gutter`, `palette_bg`, and `palette_match`.

## Presets

Embedded presets: `default-warm-dark`, `default-light`, `kanagawa`, `gruvbox-dark`, `catppuccin-mocha`, and `tokyo-night`.

## Loading

`Loader::resolve(Some(name), None)` loads an embedded preset. `Loader::resolve(None, Some(path))` loads a user TOML file. Missing tokens and unknown presets are hard errors; there are no silent token defaults.

## Resolution

`Resolver` detects terminal color capability from `MEMORUM_FORCE_COLOR`, `COLORTERM`, and `TERM`, then lowers OKLCH tokens to truecolor RGB, xterm-256 indexes, ANSI 16 names, or monochrome.

## Hot reload

`HotReload::start(path, initial)` watches a theme TOML file with notify 8 and publishes successful parses on a `tokio::sync::watch::Receiver<Theme>`. Invalid edits do not advance the receiver; `last_error()` exposes the parse or validation error for UI display.
