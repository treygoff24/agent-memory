# agent-memory

Stream A core memory substrate implementation workspace.

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
