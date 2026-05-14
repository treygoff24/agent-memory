# agent-memory Codex Instructions

## Current project frame

- This repo is Memorum (`agent-memory`): a local-first, daemon-backed shared memory layer for agent harnesses.
- Treat `README.md`, `docs/specs/system-v0.2.md`, `docs/getting-started.md`, `docs/mcp-wiring.md`, and any user-named current plan/spec as the best orientation points.
- Older stream-specific plans/specs are historical unless the user explicitly names one as the active contract for the current task.
- Do not resurrect Stream A/F/G/H/I execution assumptions just because those docs still exist in the tree.

## Project skills

- Use the project-local `rust-engineer` skill for Rust/Cargo work in this repository.
- For implementation, review, QA, security, or performance work that touches Rust, daemon behavior, MCP protocol surfaces, storage, recall, privacy, governance, or eval behavior, use or instruct subagents to use: `Mandatory skills: clean-code, tdd, rust-engineer`.
- If a subagent has a narrower specialty, keep `rust-engineer` in addition to that specialty when Rust or repo implementation is in scope.
- Prefer vertical TDD for behavior changes: one failing behavior test, minimal implementation, green test, then refactor while green.

## Repo workflow

- Do not overwrite uncommitted spec or plan files unless Trey explicitly asks.
- Preserve dirty worktrees. Inspect before editing and keep unrelated user changes out of commits/patches.
- For docs, launch materials, and non-code artifacts, ground claims in live repo docs rather than stale rollout memory.
- Use the tiered gate policy; do not run full gates repeatedly during normal implementation.
  - Inner loop: run targeted tests/checks plus `pnpm run check:fast` from the repo root, or `pnpm run check:fast` from `crates/memoryd-web/frontend` for dashboard-only work.
  - Before claiming a task, plan step, or milestone complete: run `pnpm run check:local` unless the work is explicitly narrow and you report the narrower substitute.
  - Final/pre-merge/CI or high-confidence validation only: run `pnpm run check:full` (alias for `bash scripts/check.sh`) and any extra surface-specific full gates.
  - For frontend routing, visual, a11y, perf, or e2e-covered flows, add the relevant targeted Playwright/Vitest command; use gentle variants such as `pnpm run test:gentle` and `pnpm run test:e2e:gentle` while iterating.
  - If a gate fails, fix the issue and rerun the narrow failing gate first rather than rerunning every gate.
  - Always report which gates ran and which expensive gates were intentionally skipped, with the reason. Multiple agents may be active, so keep local gates capped and avoid unnecessary CPU saturation.

## Durable product facts

- The canonical MCP bridge shape is `memoryd mcp --socket <PATH>`.
- `memoryd serve` is the local daemon entrypoint; clients connect through MCP rather than writing memory files directly.
- Canonical memory is Markdown plus YAML frontmatter in a user-owned repo. SQLite, FTS, embeddings, and event-log mirrors are derived or rebuildable unless the current docs say otherwise.
- Memorum is local-first and harness-agnostic. Do not imply a hosted cloud backend, multi-user SaaS, or replacement for Claude/Codex/Cursor.
