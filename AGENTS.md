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
- For Rust gates, prefer: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and targeted `cargo test` before broader gates.

## Installing Memorum for a user

When a user asks you to install or onboard Memorum, read and follow the canonical agent onboarding guide:

**`docs/agent-onboarding.md`**

That guide covers the detect → propose → consent → run → verify → restart loop, all flags grounded against the real CLI surface, how to interpret `SetupReport` JSON, and the mandatory restart instruction.

## Durable product facts

- The canonical MCP bridge shape is `memoryd mcp --socket <PATH>`.
- `memoryd serve` is the local daemon entrypoint; clients connect through MCP rather than writing memory files directly.
- Canonical memory is Markdown plus YAML frontmatter in a user-owned repo. SQLite, FTS, embeddings, and event-log mirrors are derived or rebuildable unless the current docs say otherwise.
- Memorum is local-first and harness-agnostic. Do not imply a hosted cloud backend, multi-user SaaS, or replacement for Claude/Codex/Cursor.
