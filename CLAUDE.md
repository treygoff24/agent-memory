# agent-memory

Implementation home for the agent-memory system. Stream A is the Rust substrate (canonical Markdown+YAML files, derived SQLite/FTS/vector indexes, per-device JSONL events, git as sync transport). Stream B is the local `memoryd` daemon and MCP bridge that fronts it. Stream C adds deterministic governance for structured writes, supersession, forgetting, and review visibility. Stream D adds the privacy classification + age-encryption boundary in front of Stream A writes.

## Current status (as of 2026-04-30)

**Streams A, B, C, and D are shipped. Stream E passive recall is next.**

Stream A: Codex landed the full substrate in `d227dce` on `main` — all 13 tasks from the v0.3 plan integrated in a single commit (~41k LOC, 183 files). `docs/reviews/stream-a-final-review.md` records release-certification with no blocking findings after remediation; full release gate green.

Stream B: Claude landed the daemon + MCP bridge in `f9d9c2b` (2026-04-28). Substrate-backed Status/Doctor/Search/Get/WriteNote handlers, seven-tool MCP forwarder (now eight after Stream D added `memory_reveal`), idle-frame timeout, watch::Receiver-driven graceful shutdown, panic-aware worker health, SIGINT/SIGTERM handling. 24 tests across 8 suites; full workspace cargo gate green debug + release.

Stream C: Codex landed governance in `6f583ec` (2026-04-29): `crates/memory-governance`, disk policy loading with fail-closed validation, built-in policies only when no policy YAML exists, deterministic governance decisions, grounding/tombstone/contradiction/supersession/review queue modules, and `memoryd` wiring for `memory_write`, `memory_supersede`, `memory_forget`, and CLI review commands. `memory_startup` still returns structured `not_implemented` because startup recall block assembly belongs to Stream E.

Stream D: Codex landed the privacy foundation in `17a0a04` and the Claude-review fix in `5f7d926` (2026-04-29). Final shape: `crates/memory-privacy/` (Layer 1 deterministic classifier, age-X25519 encryptor, file-backed key provider with 0700/0600 hardening + symlink rejection, in-session masking, optional Privacy Filter trait disabled by default), substrate integration (`api.rs`/`atomic.rs` honor `allow_encrypted_namespace` and reject plaintext under `encrypted/`; `Substrate::record_encrypted_content_revealed` + `EventKind::EncryptedContentRevealed` added under explicit Stream D scope — minor Stream A surface touch matching spec §4), memoryd handler wiring (every governance write/supersede/note runs through `DeterministicPrivacyClassifier` over body+title+summary+source_ref+tags+privacy_descriptors before any disk effect, secret/high-risk identity material is refused before any disk write, detected PII and caller personal/confidential content route to `write_encrypted`), safe descriptor indexing for encrypted records via `safe_index_projection` + `safe_plaintext_fragment` double-filter, explicit `memory_reveal` exposed as the 8th MCP tool with bounded reason validation and audit emission, CLI admin commands (`memoryd privacy …`, `memoryd privacy-filter …`, `memoryd device …` — none exposed via MCP), and e2e tests covering all of the above. The Claude-review fix decoupled tier from storage routing via `PrivacyLabel::storage_action()` returning `Plaintext|EncryptAtRest|Refuse`: URL/date stay plaintext, phone/email/address/person/account encrypt at rest without `Personal` tier elevation, SSN/Luhn-valid card/credential-like labels refuse before disk. Reviews on disk: Codex self-reviews (`docs/reviews/stream-d-{correctness,performance,security}-review.md`), Claude adversarial pass (`docs/reviews/stream-d-claude-review.md`). 345 tests pass across 72 suites; clippy + rustdoc clean.

A small Stream A FTS5 sanitization fix landed alongside Stream B in `946d75f` — `Substrate::query_chunks` now sanitizes free-form user query text into AND-ed phrase tokens so hyphenated queries (`end-to-end`) and FTS5 keyword queries no longer surface raw SQL errors. Caught in flight by the new memoryd e2e test; Trey explicitly authorized the Stream A touch.

## Who's doing what

- **Codex** owned Stream A. If Trey re-engages it for a new stream, the worktree/per-task-gate/orchestrator-merged-lockfile workflow described below is its idiom.
- **Claude (you)** owns Stream B (shipped 2026-04-28) and remains an architect/reviewer in this repo. Spec authorship, plan critique, plan-reviewer passes, sanity checks, and ad-hoc work Trey hands you. **Do not modify Stream A modules** unless Trey explicitly redirects (he did once, for the FTS5 sanitization fix in `946d75f`); the substrate is otherwise a frozen contract for downstream streams.
- **Trey** drives. He'll tell you what's next.

## Authoritative documents (use the latest, ignore older versions for current state)

- **Spec:** `docs/specs/stream-a-core-substrate-v1.1.md` is the live Stream A substrate contract. Older versions (`v0.1`, `v0.2`, `v1.0`) are kept for history; do not consult them for current behavior.
- **Plans:** `docs/plans/2026-04-26-stream-a-core-substrate-implementation-plan-v0.3.md` (Stream A, shipped), `docs/plans/2026-04-28-stream-b-daemon-mcp.md` (Stream B, shipped), and `docs/plans/2026-04-29-stream-c-governance.md` (Stream C governance).
- **Stream C docs:** `docs/api/stream-c-governance-api.md` and `docs/runbooks/governance-review.md` describe the implemented commands and response shapes.
- **Stream D docs:** `docs/specs/stream-d-privacy-v0.1.md` (spec), `docs/api/stream-d-privacy-api.md` (crate + daemon + CLI surface), `docs/reviews/stream-d-{correctness,performance,security}-review.md` (Codex self-reviews), `docs/reviews/stream-d-claude-review.md` (Claude's adversarial review with blockers and recommendations).
- **System spec:** `docs/specs/system-v0.1.md` is the parent system-level spec covering all streams.
- **Handoff:** `docs/handoff-2026-04-23.md` captures pre-repo design history.
- **Reference:** `docs/reference/handbook-v2.2.md` and `docs/reference/gpt-deep-research-2026-04-23.md` are background research, not implementation contracts.

When Trey says "the spec" or "the plan" without a version, he means the latest. When asked "where are we," check `git status`, `git worktree list`, `git log --oneline -20`, and the plan's task list — don't infer from older docs.

## Stream model (one-liners)

- **A** Core substrate (this repo). Canonical files, index, events, git, merge driver.
- **B** Daemon, MCP server, process lifecycle, embedding inference worker.
- **C** Governance: promotion, contradiction detection, grounding, tombstone matching.
- **D** Privacy filter: classification, age encryption, masked synthesis. Supplies `ClassificationOutcome` to A.
- **E** Recall block assembly, harness hooks.
- **F** Dreaming.
- **G** Human UI for review/repair.
- **H** Eval harness.
- **I** Live event subscriptions.

## Spec/plan conventions

- Spec and plan files are **versioned by suffix** (`-v1.1.md`, `-v0.3.md`). New versions supersede; old versions stay on disk for history. Never mutate an older version.
- Spec changes that affect the implementation contract get a version bump and a "Revision goal" entry at the top.
- Plan changes get a "Plan revision history" entry.
- The current spec ↔ plan pair is **v1.1 spec / v0.3 plan**. If they drift, that's a bug — surface it.

## Codex-isms in the plan (don't try to translate)

The plan was written by Codex for Codex execution. These are intentional and not Claude Code conventions:

- **Subagent type names** like `heavy_worker`, `cli_developer`, `backend_arch`, `code_mapper`, `plan_checker`, `test_hardener`, `performance_engineer`, `security_auditor`, `review_guard`, `reviewer`, `docs_editor`, `docs_researcher`, `fast_worker` — Codex's custom subagent system. Not Claude's.
- **Slash commands** `/clean-code` and `/tdd` — Codex skill invocation. Not Claude's.
- **`update_plan`** — Codex CLI's plan tracker.
- **"Spawn `<agent>`"** — Codex spawn syntax.

When reviewing the plan, treat all of the above as idiomatic for the target runtime; do not flag them as missing/wrong.

## Repository state strategy (Codex's, summarized)

- `main` is the only long-lived branch, fast-forward only.
- Each task runs in its own git worktree at `../agent-memory-wt/task-<NN>/` on a `stream-a/task-<NN>-<slug>` branch.
- Workers run only their per-task narrow gate; **`scripts/check.sh` runs only on the integrated trunk after `integrate-task-worktree.sh` fast-forwards `main`**, never inside a task worktree (stub modules from unstarted tasks would fail workspace tests for the wrong reason).
- `Cargo.lock` and `pnpm-lock.yaml` are orchestrator-merged. Workers update `Cargo.toml` only.
- Don't touch Codex's in-flight worktrees or branches without checking with Trey.

## Critical invariants (will fail review if violated)

These are spec-mandated, not preferences:

1. **`secret` is never persisted to disk.** It's a `ClassificationOutcome` value supplied per-write by Stream D; Stream A returns `WriteFailureKind::SecretRefused` before any disk effect (spec §8.7).
2. **Every write request carries a `ClassificationOutcome`.** No defaults. Plaintext writes with `RequiresEncryption` classification get `EncryptionRequired`; `Trusted` with sensitive frontmatter gets `ClassificationSensitivityMismatch`.
3. **Embedding triple `(provider, model_ref, dimension)` is identity, not flavor.** Mismatch returns typed errors (`DimensionMismatch`, `UnknownEmbeddingTriple`) — never silent fallback (spec §10.2.2).
4. **Device IDs live only in local runtime state**, never in the synced `config.yaml`. A fresh clone must regenerate device identity via `git::adopt_clone` before any write.
5. **`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`** is the single source of truth for the merge driver's schema gate. No magic numbers (spec §14.2).
6. **Two-clone convergence** is canonical-content equality per spec §13.6.1, not raw `git diff`.
7. **Performance baselines** at `bench/baseline.<profile>.json` are updated only by explicit human-authored commits — the bench harness never overwrites them (spec §17.6, §18.9).

## What's on disk (Streams A-D shipped; Stream E next, as of 2026-04-30)

- **`crates/memory-substrate/`** (Stream A) — public API (`api.rs` — Stream D added an `allow_encrypted_namespace` flag to `AtomicWrite` so plaintext writes still refuse `encrypted/` while supersede can update encrypted records), model + error taxonomy, frontmatter (parse/validate/serialize/defaults/schema), tree (layout/validate), config, IDs (sequence/repair), events (log/framing/sequence/recovery), index (schema/migrations/chunking/query — query.rs sanitizes FTS5 input, sqlite-vec adapter), git (init/adopt/preflight/commit/sync/command), watcher (subscription/filter/suppression), merge (three_way/quarantine), runtime (reconcile/faults), bench harness binary. ~30 integration test files including `spec_coverage_manifest.rs`, `crash_matrix.rs`, `startup_reconciliation.rs`, `vector_lifecycle.rs`, `fts_query_sanitization.rs`.
- **`crates/memoryd/`** (Streams B/C/D) — `cli.rs`, `client.rs`, `handlers.rs`, `main.rs`, `mcp.rs`, `protocol.rs`, `server.rs`, `workers.rs`. Daemon serves newline-delimited JSON over a Unix socket with a 64 KiB frame cap, idle-frame timeout, watch::Receiver-driven graceful shutdown, and (Stream D) owner-only socket chmod after bind; `serve_substrate_with(socket, substrate, options, shutdown_rx)` is the supervised entry; SIGINT/SIGTERM wired in `main`. MCP forwarder declares eight agent-facing tools (Search/Get/Note plus governed Write/Supersede/Forget/Reveal); admin commands (`privacy`, `privacy-filter`, `device`, `review`) are CLI-only and explicitly rejected from MCP. Stream D's `write_privacy_memory` runs `DeterministicPrivacyClassifier` over body+title+summary+source_ref+tags+privacy_descriptors before any disk effect; high-risk secrets refuse, PII encrypts at rest, and safe descriptors may be indexed for encrypted records. Startup remains structured `not_implemented` for Stream E.
- **`crates/memory-governance/`** (Stream C) — deterministic governance decisions, policy loader/built-in policies, grounding verification, contradiction/tombstone/supersession helpers, and review queue projection.
- **`crates/memory-privacy/`** (Stream D) — Layer 1 deterministic classifier (`classifier.rs`, `regex.rs`, `entropy.rs`), monotonic tier plus storage-action policy (`policy.rs`, `decision.rs`), `PrivacyEncryptor` over `age` X25519 (`crypto.rs`), file-backed key provider with 0700/0600 hardening + symlink rejection (`keys.rs`), in-session `MaskingSession` (`masking.rs`), optional Privacy Filter trait with disabled-by-default + fixture providers (`privacy_filter.rs`). Six contract test files in `tests/`; full crate ~1k LoC. URL/date labels detect without encrypting by default; phone/email/address labels encrypt at rest without tier elevation; SSN/Luhn-valid card/credential-like labels refuse.
- **`crates/memory-merge-driver/`** — CLI + `tests/merge_driver_cli.rs`.
- **`crates/memory-test-support/`** — `convergence.rs`, `perf.rs`, `bin/rust_boundary_check.rs`.
- **`fuzz/`** — `merge_driver` and `merge_swap_convergence` targets.
- **`scripts/`** — `check.sh` (full release gate), `two-clone-convergence.sh`, `durability-probe-gate.sh`, `bench-gate.sh`, `bench-regression-check.sh`, plus task-worktree helpers.
- **`bench/baseline.darwin-arm64.json`** — re-captured 2026-04-27 under realistic system load (post-query.rs perf fixes), with `captured_at`/`captured_method` provenance fields. Codex's original baseline was set on an idle machine and didn't survive contact with normal load (3x slower SQLite query p95s). **`bench/baseline.linux-x86_64.json`** — still `runs: 0` placeholder; first-release bootstrap path emits a `.proposed` file rather than failing. Per spec §17.6/§18.9, baselines are only updated by explicit human commits; the bench harness never overwrites them.
- **`.github/workflows/`** — `stream-a-ci.yml`, `stream-a-fuzz.yml`, `stream-a-perf.yml`.
- **`.dylint/custom_lints/`**, `.oxlintrc.json`, `.oxfmtrc.json`, `clippy.toml` — installed from agentlinters SHA `91446bb`.
- **`docs/api/stream-a-public-api.md`**, **`docs/dev/stream-a-architecture.md`**, **`docs/dev/stream-a-test-matrix.md`** — public surface, architecture, and test matrix references.
- **`docs/reviews/`** — `stream-a-final-review.md`, plus the `2026-04-25-buildout/` lane reviews + adversarial pass + SUMMARY, plus per-domain final review summaries (performance, security, test-coverage), plus the four Stream D reviews (Codex's correctness/performance/security + Claude's adversarial pass).
- **`docs/runbooks/`** — `operator-repair.md`, `privacy-leak-response-placeholder.md`.
- **`modules/stream-a-*.spec.yml`** — specgate module manifests.

The full release gate is `bash scripts/check.sh` (with `BENCH_PROFILE=darwin-arm64` on Trey's machine).

## Running review or sanity-check work

Standard recipe when Trey asks "review this" or "is this ready":

1. Read the live spec (v1.1) and plan (v0.3) sections relevant to the question.
2. Read the actual files in the repo, not just the plan's description of them.
3. For plan reviews, brief the `plan-reviewer` subagent with the Codex-conventions caveat (subagent types, slash commands, `update_plan` are intentional).
4. Report blockers vs risks vs nits separately. Trey wants real adversarial critique, not validation.

## What NOT to do

- Don't run `cargo test --workspace` inside Codex's task worktrees — see "Repository state strategy."
- Don't run `git pull` on `/Users/treygoff/Code/agentlinters` — the SHA is pinned at `91446bb` and assets are copied from there.
- Don't overwrite `bench/baseline.*.json` programmatically — they require explicit human commits.
- Don't bump spec or plan versions without Trey's explicit ask.
- Don't add `secret` as a frontmatter `sensitivity` value anywhere. It's a runtime `ClassificationOutcome` only.
- Don't use `cargo generate-lockfile` for any integration work — use `cargo build --workspace --locked` + targeted `cargo update -p <crate>`.

## Project-local agents and skills

- **Skills (project-active):** `clean-code` and `rust-engineer` are symlinked under `.claude/skills/` via `claude-skill add`. They auto-load each session. Reach for `rust-engineer` proactively for ownership/lifetime/async-tokio work in this repo; reach for `clean-code` when reviewing or hardening.
- **Agents:** None defined yet. If Trey asks for a per-project agent (e.g. a Rust-aware reviewer specific to substrate boundaries), the convention is `.claude/agents/<name>.md`.
