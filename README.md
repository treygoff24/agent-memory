# agent-memory

Local memory substrate, daemon, MCP bridge, and Stream C governance workspace.

## Local gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
pnpm run format:check
pnpm run lint
./scripts/rust-boundary-check.sh
./scripts/two-clone-convergence.sh --smoke
```

`bash scripts/check.sh` runs the canonical local checkpoint gate. It includes Specgate when the CLI is installed, runs a real smoke two-clone merge-driver convergence check, and uses `BENCH_PROFILE` or `scripts/detect-bench-profile.sh` for smoke perf output.

## Project skill

This repo carries a project-local Rust skill at `.codex/skills/rust-engineer`. Root agents and subagents doing Stream A work must use `clean-code`, `tdd`, and `rust-engineer`.

## Implemented stream docs

- Stream A substrate API: `docs/api/stream-a-public-api.md`
- Stream C governance API: `docs/api/stream-c-governance-api.md`
- Stream D privacy API: `docs/api/stream-d-privacy-api.md`
- Stream E passive recall API: `docs/api/stream-e-passive-recall-api.md`
- Governance review runbook: `docs/runbooks/governance-review.md`
- Privacy leak response runbook: `docs/runbooks/privacy-leak-response-placeholder.md`

Stream C governs `memory_write`, `memory_supersede`, and `memory_forget`
through `memoryd`, plus CLI review queue operations. Stream D privacy classifies
daemon writes, refuses secrets/high-risk identity numbers, routes detected PII
and personal/confidential content to encrypted Stream A writes, and exposes
explicit `memory_reveal` for audited user-directed decrypt access. Stream E
ships passive recall: `memory_startup` forwards through the daemon, `memoryd
recall startup-block` and `memoryd recall delta-block` emit XML for hooks, and
status responses include additive recall counters.
