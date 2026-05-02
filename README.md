# agent-memory

Local Memorum memory substrate, daemon, MCP bridge, governance, privacy, recall, dreaming, and observability workspace. Streams A-F are shipped. Stream G is implemented in this worktree with the canonical observability benchmark baseline promoted; final Stream G shipped status is pending final review/gate signoff. Streams H/I are not claimed here.

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

## Stream and API docs

- Stream A substrate API: `docs/api/stream-a-public-api.md`
- Stream C governance API: `docs/api/stream-c-governance-api.md`
- Stream D privacy API: `docs/api/stream-d-privacy-api.md`
- Stream E passive recall API: `docs/api/stream-e-passive-recall-api.md`
- Stream F dreaming API: `docs/api/stream-f-dreaming-api.md`
- Stream G observability API: `docs/api/stream-g-observability-api.md`
- Stream G architecture: `docs/dev/stream-g-architecture.md`
- Governance review runbook: `docs/runbooks/governance-review.md`
- Reality Check runbook: `docs/runbooks/reality-check.md`
- Privacy leak response runbook: `docs/runbooks/privacy-leak-response-placeholder.md`

Stream C governs `memory_write`, `memory_supersede`, and `memory_forget`
through `memoryd`, plus CLI review queue operations. Stream D privacy classifies
daemon writes, refuses secrets/high-risk identity numbers, routes detected PII
and personal/confidential content to encrypted Stream A writes, and exposes
explicit `memory_reveal` for audited user-directed decrypt access. Stream E
ships passive recall: `memory_startup` forwards through the daemon, `memoryd
recall startup-block` and `memoryd recall delta-block` emit XML for hooks, and
status responses include additive recall counters. Stream G implements human
observability: `memoryd ui`, localhost web dashboard, trust artifact rendering,
notifications, Reality Check CLI/TUI/web surfaces, `NotificationEvent`, and the
`reality_check_due` pending-attention hook. The canonical Stream G observability
benchmark baseline is promoted at
`bench/stream-g-observability-results.darwin-arm64.json`; final Stream G shipped
status is pending final review/gate signoff.

## Stream F dreaming status

Dreaming uses whichever agent-harness CLI you have installed and authenticated on this device (Claude Code, Codex CLI, Gemini, etc.). Dream prompts are masked through the agent-memory privacy filter before they leave the daemon, but the masked text is processed by the harness CLI's upstream model provider. The data, retention, and training policies of that provider apply. Where this device's selected harness CLI accepts prompts on stdin, the prompt is not visible to other local processes; where it does not, the prompt may be visible via process listing tools (`ps`, `top`, `/proc/<pid>/cmdline`). `memoryd dream status` shows the prompt-transport mode for each installed harness adapter. Substrate fragments written via `memory_observe` are git-synced as low-level durable telemetry; this means the private git repo's raw-observation surface is larger than its canonical-memory surface, even though substrate is not searchable as memory. If you don't want dream content sent to a particular provider, set the per-scope CLI priority to exclude it, or run `memoryd dream disable` on this device.

Stream F v0.2 is shipped with a green final release gate recorded in `docs/reviews/stream-f-final-gate-report.md`. The API doc is `docs/api/stream-f-dreaming-api.md`. Stream F adds `memory_observe` for substrate fragments while `memory_note` is unchanged for canonical note writes. CLI/provider disclosure currently covers `ClaudeCodeCli`, `CodexCli`, and disabled/deferred Gemini; shipped v0.2 adapters use `prompt_transport: stdin`, and any future argv fallback must disclose the upstream data policy and local process-listing risk. The device-local `dream-disabled` sentinel remains the opt-out switch.
