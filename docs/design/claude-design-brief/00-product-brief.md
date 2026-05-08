# Memorum — Product Brief for Dashboard Design

This is the context you need to design the **localhost web dashboard** for Memorum. Read `01-design-system.md` for visual tokens, `02-dashboard-views.md` for what to design, `03-data-shapes.md` for real data, and open `04-tui-reference.html` in a browser to see the aesthetic anchor.

## What Memorum is

Memorum is a **personal, local-first memory system for AI agents**. It runs as a daemon on the user's machine. AI agents (Claude Code, Codex CLI, etc.) read and write durable memories through it via MCP. The user — one human, one machine — owns it, syncs it across their own devices over git, and is the only reviewer.

Mental model: **a second brain that agents share with you, with receipts.** Every memory has provenance (which session, which agent, what grounding), confidence, recall history, and a privacy classification. Some memories are written automatically; some need user confirmation; conflicts surface for review; encrypted memories stay encrypted at rest. The system also "dreams" — it runs background passes to find patterns, contradict itself, and queue questions for the user.

## Who uses it

For v1: **the operator themselves.** Trey, the person who built it, dogfooding. Power user. Comfortable with terminals. Wants density, not hand-holding. Reads the dashboard the way a sysadmin reads a monitoring console — quickly, scanning for what needs attention.

The dashboard is **not** for end-users-of-the-system in any consumer sense. There is no signup, no account, no remote access, no team features in v1. It binds to `127.0.0.1` only.

## Why a dashboard at all

The TUI (`memoryd ui`, also being designed in parallel — see `04-tui-reference.html`) handles the **inline, in-session, keyboard-driven** workflow: triage, Reality Check, quick inspection. It's amazing for "I'm in a terminal, something needs my attention."

The web dashboard handles the **deeper, spatial, multi-pane review** workflow:

- Reading provenance graphs across many memories
- Comparing supersession chains
- Browsing the recall ledger with rich timeline + filters
- Reviewing dream outputs (questions, conflicts, patterns)
- Inspecting trust artifacts with their full structure visible
- Watching peer activity across the user's devices

Web is better for these because layouts are richer, mouse + keyboard work, and large amounts of structured data render faster and with more affordances than a TTY allows.

## What's already built (don't redesign these)

Streams A–I shipped. The daemon, indices, governance, privacy classification, recall, dreaming, eval harness, and cross-device coordination all work. The dashboard has a defined HTTP API with stable JSON shapes (see `03-data-shapes.md`). The dashboard ships **CSRF-protected**, **localhost-only**, **statically-bundled** (no CDN, fonts self-hosted).

You are designing **the presentation layer** on top of those shipped APIs.

## Voice and tone

- **Trustworthy, not precious.** This is an operator console for someone's second brain. It needs to feel solid, like a well-engineered tool. Not playful, not whimsical, not gamified.
- **Calm, not loud.** Restraint with color. Notifications are subtle. Numbers are tabular. Empty states are honest, not chirpy.
- **Dense, but readable.** This user wants to see a lot at once. Don't pad. Don't oversize. Don't use cards-with-lots-of-whitespace as a default container.
- **Family resemblance to the TUI.** Same warm-dark amber palette, same iconographic spirit, same restraint about animation. Web idioms where they help (hover, focus rings, real layout), TTY discipline where it earns (tabular numbers, monospaced IDs, no surprise motion).

## Design constraints baked in from day 1

- **Themable end-to-end.** Same tokenization story as the TUI: every color, font, accent is a named token. Six theme presets (warm-dark default, warm-light, high-contrast, monochrome, plus two more). Users can customize. Don't hardcode a single palette into components.
- **Accessible.** WCAG AA contrast minimum on every theme. Reduced-motion respected. Full keyboard navigation. Focus rings always visible.
- **Localhost-only, single-user.** No login screen, no permission UI, no multi-tenancy. The user is implicitly the owner. Design accordingly.
- **CSRF-protected mutations.** Any action button that writes (approve, reject, correct, forget) sends a token. Design feedback for the rare 403 case.
- **Empty states are common.** A user who just installed Memorum has zero memories. Design for emptiness as a first-class state, not an afterthought.

## What is explicitly out of scope for v1

- **No remote access.** Don't design for "logging in from another device." It binds to localhost.
- **No multi-user.** No avatars, no @mentions, no role pickers.
- **No policy editor UI.** That's deferred to v1.1+. Routes return 501 today.
- **No sync dashboard UI.** Also v1.1+.
- **No memory editor.** Edits go through `$EDITOR` from the TUI or governance flow, not a web textarea (privacy + governance reasons).

## What this means for your job

Design **a calm, dense, themable, keyboard-first console** that makes a power user feel they're in command of their second brain. The TUI mockup in `04-tui-reference.html` is the aesthetic anchor — the dashboard should feel like its richer, web-native sibling.
