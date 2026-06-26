# agent-memory Codex Instructions

## Current project frame

- This repo is Memorum (`agent-memory`): a local-first, daemon-backed shared memory layer for agent harnesses.
- Treat `README.md`, `docs/specs/system-v0.2.md`, `docs/getting-started.md`, `docs/mcp-wiring.md`, and any user-named current plan/spec as the best orientation points.
- Older stream-specific plans/specs are historical unless the user explicitly names one as the active contract for the current task.
- Do not resurrect Stream A/F/G/H/I execution assumptions just because those docs still exist in the tree.

## Project skills

- Use the project-local `rust-engineer` skill for Rust/Cargo work in this repository.
- For implementation, review, QA, security, or performance work that touches Rust, daemon behavior, MCP protocol surfaces, storage, recall, privacy, governance, or eval behavior, use or instruct subagents to use: `Mandatory skills: clean-code, rust-engineer`.
- If a subagent has a narrower specialty, keep `rust-engineer` in addition to that specialty when Rust or repo implementation is in scope.

## Repo workflow

- Do not overwrite uncommitted spec or plan files unless Trey explicitly asks.
- Preserve dirty worktrees. Inspect before editing and keep unrelated user changes out of commits/patches.
- For docs, launch materials, and non-code artifacts, ground claims in live repo docs rather than stale rollout memory.
- For Rust gates, **scope to the crate you touched** — `cargo check -p <pkg>`, `cargo clippy -p <pkg> --all-targets -- -D warnings`, `cargo test -p <pkg> -- --test-threads=2`. The whole-workspace gate (workspace clippy, full tests, bench) runs **only** via `bash scripts/check.sh` on the integrated trunk when the task is fully done — never mid-task. See "Build / lint / test CPU discipline" below for why, and never run a bare `cargo clippy` / `cargo test --workspace`.

## Build / lint / test CPU discipline (read before running ANY cargo gate)

Running cargo across the whole workspace spawns **one compiler process per crate** — 12 of them — at once. On macOS every freshly built binary is re-validated by `syspolicyd` on launch, so a 12-wide swarm pegs a core and roasts the machine. This is the single most common way an agent cooks Trey's laptop. Treat it as a hard rule, not a preference.

**Inner loop — always scope to the crate you touched** (it's the directory under `crates/` you edited):

- `cargo check -p <crate>` — fastest; "does it still compile."
- `cargo clippy -p <crate> --all-targets -- -D warnings` — lint that crate.
- `cargo test -p <crate> -- --test-threads=2` — test that crate.

**Leaf vs foundational.** Scoping to a leaf crate (e.g. `memoryd`, which nothing depends on) is complete. If you edit a foundational crate (`memory-substrate`, `memory-source`, …), `-p` on it alone won't catch breakage in its dependents — `cargo tree -i <crate>` lists them, and the final full gate covers the rest.

**Full gate — `bash scripts/check.sh`, and only this — runs ONLY** on the integrated trunk after `integrate-task-worktree.sh` fast-forwards `main`, **once**, when the task is completely done. Never inside a task worktree, never mid-task. It already carries the macOS syspolicyd mitigation and ends with perf-sensitive benchmarks, so don't throttle it or run it speculatively.

**Never, mid-task:** bare `cargo clippy`, `cargo test` / `cargo test --workspace`, `cargo nextest run --workspace`, or `bash scripts/check.sh`. Claude Code enforces this with a `PreToolUse` hook in the main checkout; **Codex gets no such hook** — in worktrees and under Codex this discipline is entirely self-enforced, so hold it deliberately.

## Installing Memorum for a user

When a user asks you to install or onboard Memorum, read and follow the canonical agent onboarding guide:

**`docs/agent-onboarding.md`**

That guide covers the detect → propose → consent → run → verify → restart loop, all flags grounded against the real CLI surface, how to interpret `SetupReport` JSON, and the mandatory restart instruction.

## Durable product facts

- The canonical MCP bridge shape is `memoryd mcp --socket <PATH>`.
- `memoryd serve` is the local daemon entrypoint; clients connect through MCP rather than writing memory files directly.
- Canonical memory is Markdown plus YAML frontmatter in a user-owned repo. SQLite, FTS, embeddings, and event-log mirrors are derived or rebuildable unless the current docs say otherwise.
- Memorum is local-first and harness-agnostic. Do not imply a hosted cloud backend, multi-user SaaS, or replacement for Claude/Codex/Cursor.
