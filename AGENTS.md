# agent-memory Codex Instructions

## Project skills

- Use the project-local `rust-engineer` skill for all Rust/Cargo work in this repository.
- Every implementation, review, QA, security, performance, and docs subagent spawned for Stream A must be told: `Mandatory skills: clean-code, tdd, rust-engineer`.
- If a subagent has a narrower specialty, keep `rust-engineer` in addition to that specialty; do not replace it with the specialty.
- Follow vertical TDD for implementation: one failing behavior test, minimal implementation, green test, refactor while green.

## Stream A execution

- Treat `docs/plans/2026-04-26-stream-a-core-substrate-implementation-plan-v0.3.md` and `docs/specs/stream-a-core-substrate-v1.1.md` as the active implementation contract unless superseded by a newer local plan/spec.
- Do not overwrite uncommitted spec or plan files unless Trey explicitly asks.
- For Rust gates, prefer: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and targeted `cargo test` before broader gates.
