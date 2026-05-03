# agent-memory

Implementation home for the agent-memory system. Stream A is the Rust substrate (canonical Markdown+YAML files, derived SQLite/FTS/vector indexes, per-device JSONL events, git as sync transport). Stream B is the local `memoryd` daemon and MCP bridge that fronts it. Stream C adds deterministic governance for structured writes, supersession, forgetting, and review visibility. Stream D adds the privacy classification + age-encryption boundary in front of Stream A writes. Stream E adds passive startup/delta recall through daemon/MCP/CLI surfaces. Stream F adds dreaming. Stream G adds human observability: TUI, localhost web dashboard, Reality Check, notifications, and trust artifact rendering. Stream H adds the eval harness. Stream I adds cross-session coordination: peer-update framing, peer presence, advisory claim locks, and peer admin CLI surfaces. Streams A-I are shipped; Stream H live real-harness success remains environment-dependent on authenticated Claude/Codex CLIs.

## Current status (as of 2026-05-02)

**Streams A-I are shipped.** Stream G's canonical benchmark baseline is `bench/stream-g-observability-results.darwin-arm64.json`; Stream H's eval harness now reports real assertion counts and runtime skip markers instead of fabricated pass/skip rows; Stream I's production-like benchmark baseline is `bench/stream-i-cross-session-results.darwin-arm64.json`. Stream H real-harness flows still require authenticated Claude/Codex CLI validation before claiming live LLM success for the auth-gated live LLM paths.

Stream A: Codex landed the full substrate in `d227dce` on `main` — all 13 tasks from the v0.3 plan integrated in a single commit (~41k LOC, 183 files). `docs/reviews/stream-a-final-review.md` records release-certification with no blocking findings after remediation; full release gate green.

Stream B: Claude landed the daemon + MCP bridge in `f9d9c2b` (2026-04-28), and the post-shipping F-001 remediation added the launchable stdio MCP server as `memoryd mcp --socket <path>` (2026-05-02). Current shape: substrate-backed Status/Doctor/Search/Get/WriteNote handlers, nine-tool MCP manifest/forwarder (`memory_search`, `memory_get`, `memory_write`, `memory_supersede`, `memory_forget`, `memory_reveal`, `memory_startup`, `memory_note`, `memory_observe`), newline-delimited JSON-RPC MCP stdio handshake (`initialize`, `notifications/initialized`, `tools/list`, `tools/call`), idle-frame timeout, watch::Receiver-driven graceful shutdown, panic-aware worker health, SIGINT/SIGTERM handling. Full workspace cargo gate was green at original Stream B shipment; post-audit MCP stdio tests cover the launchable MCP surface.

Stream C: Codex landed governance in `6f583ec` (2026-04-29): `crates/memory-governance`, disk policy loading with fail-closed validation, built-in policies only when no policy YAML exists, deterministic governance decisions, grounding/tombstone/contradiction/supersession/review queue modules, and `memoryd` wiring for `memory_write`, `memory_supersede`, `memory_forget`, and CLI review commands. Stream E now owns implemented startup recall on top of those governed read projections.

Stream D: Codex landed the privacy foundation in `17a0a04` and the Claude-review fix in `5f7d926` (2026-04-29). Final shape: `crates/memory-privacy/` (Layer 1 deterministic classifier, age-X25519 encryptor, file-backed key provider with 0700/0600 hardening + symlink rejection, in-session masking, optional Privacy Filter trait disabled by default), substrate integration (`api.rs`/`atomic.rs` honor `allow_encrypted_namespace` and reject plaintext under `encrypted/`; `Substrate::record_encrypted_content_revealed` + `EventKind::EncryptedContentRevealed` added under explicit Stream D scope — minor Stream A surface touch matching spec §4), memoryd handler wiring (every governance write/supersede/note runs through `DeterministicPrivacyClassifier` over body+title+summary+source_ref+tags+privacy_descriptors before any disk effect, secret/high-risk identity material is refused before any disk write, detected PII and caller personal/confidential content route to `write_encrypted`), safe descriptor indexing for encrypted records via `safe_index_projection` + `safe_plaintext_fragment` double-filter, explicit `memory_reveal` exposed as the 8th MCP tool with bounded reason validation and audit emission, CLI admin commands (`memoryd privacy …`, `memoryd privacy-filter …`, `memoryd device …` — none exposed via MCP), and e2e tests covering all of the above. The Claude-review fix decoupled tier from storage routing via `PrivacyLabel::storage_action()` returning `Plaintext|EncryptAtRest|Refuse`: URL/date stay plaintext, phone/email/address/person/account encrypt at rest without `Personal` tier elevation, SSN/Luhn-valid card/credential-like labels refuse before disk. Reviews on disk: Codex self-reviews (`docs/reviews/stream-d-{correctness,performance,security}-review.md`), Claude adversarial pass (`docs/reviews/stream-d-claude-review.md`). 345 tests pass across 72 suites; clippy + rustdoc clean.

A small Stream A FTS5 sanitization fix landed alongside Stream B in `946d75f` — `Substrate::query_chunks` now sanitizes free-form user query text into AND-ed phrase tokens so hyphenated queries (`end-to-end`) and FTS5 keyword queries no longer surface raw SQL errors. Caught in flight by the new memoryd e2e test; Trey explicitly authorized the Stream A touch.

Stream E: Codex implemented passive recall from plan revision v0.4 against spec v0.5 on 2026-04-30. Final shape: Stream A gained additive `MemoryQuery` filters plus `query_recall_index` over indexed status/namespace/passive-recall/governance fields; Stream D exposes `safe_plaintext_fragment`; `memoryd` now supports `RequestPayload::Startup`, `ResponsePayload::Startup`, in-process recall counters on `StatusResponse.recall`, MCP forwarding for `memory_startup`, and `memoryd recall startup-block` / `memoryd recall delta-block` hook commands. Startup recall emits stable `<memory-recall version="stream-e-v0.5">` XML sections; delta no-match emits exactly `<memory-delta empty="true" />`. Candidate/quarantine items affect pending-attention counts without leaking claim text, encrypted/body-disabled rows are omitted from factual recall, and release performance evidence is recorded in `bench/stream-e-recall-results.darwin-arm64.json` plus `docs/reviews/stream-e-performance-review.md`. Review reports live under `docs/reviews/stream-e-*review.md` and the API surface is documented in `docs/api/stream-e-passive-recall-api.md`.

Stream F: Claude authored the spec across two revisions — v0.1 was reviewed by Codex (`docs/reviews/stream-f-codex-spec-review.md`), and v0.2 incorporated all twelve contract changes from that review (separate `memory_observe` MCP tool as the 9th agent-facing surface, JSONL Pass 3 outputs with explicit `entities` arrays for masked entity matching, explicit Pass 2 evidence catalog, stdin-only prompt transport, MaskingSession API aligned to shipped `new`/`mask`/`restore`+Drop, dream files explicitly excluded from canonical memory parsing/indexing, `PassOutcome.candidate_results` with refusal reasons, scheduled-run retry window with manual fail-fast, `<pending-attention>` caps tightened to 2/scope and 6 total, path encoding without colons, daemon-authored git commit conventions for `memoryd lease-bot`/`memoryd cleanup-bot`, six previously-missing config keys defined). Architecture cheat: dreaming delegates LLM calls to whichever harness CLI is installed (`claude -p`, `codex exec`, etc.) rather than building a generic provider — if the harness isn't installed, that device doesn't dream. Codex authored and executed `docs/plans/2026-04-30-stream-f-dreaming.md` with 17 tasks across 5 phases plus 5 review gates. Claude's plan-reviewer pass surfaced 4 blockers (missing `ReadError::NotACanonicalMemory` variant + spec calls wrong API for path-based reads; missing `WriteFailureKind::DreamProseAsSource` test owner; missing `no_entity_match` counter key in Task 13; exit-code-5 plumbing split between Tasks 9 and 14 with no clear test owner) plus risks around `grounding_rehydration_required` field provenance, bench-file overwrite vs baseline policy, prompt determinism test weakness, prompt template runtime loading, and Stream E hot-path regression coverage. Codex patched the plan against those findings and shipped Stream F after final clean-code, API-contract, security, performance, and test-hardening reruns all passed. The Stream F implementation adds `memory_observe` while `memory_note` remains unchanged, uses `prompt_transport` disclosures for `ClaudeCodeCli`, `CodexCli`, and disabled/deferred Gemini, and lands Stream A surface additions (six new top-level path families: `substrate/`, `encrypted/substrate/`, `dreams/journal/`, `dreams/questions/`, `dreams/cleanup/`, `leases/`; merge-driver semantics for each; `EventKind::SubstrateFragmentWritten`; tree-validator + canonical-isolation rules) explicitly authorized by spec §1.1. Final gate evidence is in `docs/reviews/stream-f-final-gate-report.md`.

**Streams A-I are shipped.** Stream H shipped the eval harness with the caveat that real #13/#15 live Claude/Codex runs still require authenticated local/CI CLIs; Stream I shipped cross-session coordination docs and runtime surfaces after focused Gate D reruns cleared the prior blockers. Branding decision: the system ships as "Memorum" (Latin genitive plural of `memor`, "mindful"). Trey wrote the parent system spec at `docs/specs/system-v0.2.md`; Claude wrote stream-level specs (`stream-g-observability-v0.1.md`, `stream-h-eval-harness-v0.1.md`, `stream-i-cross-session-v0.1.md`) and Codex-style execution plans (`docs/plans/2026-05-01-stream-{g,h,i}-*.md`). Stream G implements the TUI, localhost web dashboard, Reality Check CLI/protocol/session state, `NotificationEvent` dispatcher, trust artifact rendering, `EventKind::RecallHit`/events_log covering index consumption, and `reality_check_due` pending-attention integration; the reviewed canonical benchmark baseline exists at `bench/stream-g-observability-results.darwin-arm64.json`. Deferred v1.1+ Stream G work is limited to policy-editor/sync-dashboard web sections, remote dashboard auth, and richer notification diagnostics. Stream H shipped the `memorum-eval` crate, 19-test catalog, JSON reporting, CI workflow, and T19 peer-update framing slot; authenticated live-harness success remains environment-dependent. Stream I shipped `crates/memorum-coordination`, daemon heartbeat/status/activity/release-lock surfaces, recall XML insertion for `<peer-update>` / `<peer-presence>`, per-project `concurrent_session_mode`, cross-device startup peer updates, and docs at `docs/api/stream-i-cross-session-api.md` / `docs/dev/stream-i-architecture.md`.

## Who's doing what

- **Codex** owned Stream A and implemented Streams G, H, and I. The worktree/per-task-gate/orchestrator-merged-lockfile workflow described below is its idiom.
- **Claude (you)** owns Stream B (shipped 2026-04-28). For H/I implementation, Claude is reviewer-only unless Trey explicitly redirects. Otherwise Claude remains an architect/reviewer in this repo: spec authorship, plan critique, plan-reviewer passes, sanity checks, and ad-hoc work Trey hands you. **Do not modify Stream A modules** unless Trey explicitly redirects (he did once, for the FTS5 sanitization fix in `946d75f`); the substrate is otherwise a frozen contract for downstream streams.
- **Trey** drives. He'll tell you what's next.

## Authoritative documents (use the latest, ignore older versions for current state)

- **Stream A spec:** `docs/specs/stream-a-core-substrate-v1.1.md` is the live substrate contract. Older versions (`v0.1`, `v0.2`, `v1.0`) are kept for history; do not consult them for current behavior.
- **Stream E spec:** `docs/specs/stream-e-passive-recall-v0.5.md` is the live passive-recall contract. `v0.1`–`v0.4` are kept for history; do not consult them for current behavior. v0.5's revision-goal block at the top documents what changed across each bump.
- **Stream F spec:** `docs/specs/stream-f-dreaming-v0.2.md` is the live dreaming contract. API docs live at `docs/api/stream-f-dreaming-api.md`. `v0.1` is kept for history (Codex reviewed it in `docs/reviews/stream-f-codex-spec-review.md`); do not consult it for current behavior. v0.2's revision-goal block at the top documents what changed.
- **Stream G spec:** `docs/specs/stream-g-observability-v0.1.md` is the live observability/UX contract (TUI + web dashboard + Reality Check + notifications). API/architecture/runbook docs live at `docs/api/stream-g-observability-api.md`, `docs/dev/stream-g-architecture.md`, and `docs/runbooks/reality-check.md`. Shipped with deferred v1.1+ sections documented explicitly; the canonical observability benchmark baseline is promoted.
- **Stream H spec:** `docs/specs/stream-h-eval-harness-v0.1.md` is the live eval-harness contract (19-test catalog + CI workflow + regression-as-test workflow). API docs live at `docs/api/stream-h-eval-api.md`. Shipped with Streams A-H; live real-harness validation still depends on authenticated Claude/Codex CLIs.
- **Stream I spec:** `docs/specs/stream-i-cross-session-v0.1.md` is the live cross-session/peer-coordination contract (peer-presence heartbeat, claim-locks, peer-update framing, three-level coordination model). API/architecture docs live at `docs/api/stream-i-cross-session-api.md` and `docs/dev/stream-i-architecture.md`. Shipped with Streams A-I.
- **Plans:** `docs/plans/2026-04-26-stream-a-core-substrate-implementation-plan-v0.3.md` (Stream A, shipped), `docs/plans/2026-04-28-stream-b-daemon-mcp.md` (Stream B, shipped), `docs/plans/2026-04-29-stream-c-governance.md` (Stream C, shipped), `docs/plans/2026-04-30-stream-e-passive-recall.md` (Stream E, shipped from plan revision v0.4 against spec v0.5), `docs/plans/2026-04-30-stream-f-dreaming.md` (Stream F, shipped), `docs/plans/2026-05-01-stream-g-observability.md` (Stream G implemented with canonical baseline promoted), `docs/plans/2026-05-01-stream-h-eval-harness.md` (Stream H, shipped), and `docs/plans/2026-05-01-stream-i-cross-session.md` (Stream I, shipped).
- **Plan reviews on disk for H/I and Stream G context:** `docs/reviews/stream-{g,h,i}-spec-review.md` (per-stream spec reviews), `docs/reviews/stream-{g,h,i}-plan-review.md` (per-stream plan reviews), `docs/reviews/stream-ghi-combined-plan-review.md` (pass 1, BLOCK), `docs/reviews/stream-ghi-combined-plan-review-pass-2.md` (pass 2, RISK with no blockers — greenlit). Read these before reviewing H/I implementation PRs or Stream G regressions — they document the four-blocker fix loop and the design rationale for `memory_supersession` as a derived projection, the events_log mirror's dual-write semantics + observability, and the NULL-`source_harness`-as-conservative-floor decision.
- **Stream C docs:** `docs/api/stream-c-governance-api.md` and `docs/runbooks/governance-review.md` describe the implemented commands and response shapes.
- **Stream D docs:** `docs/specs/stream-d-privacy-v0.1.md` (spec), `docs/api/stream-d-privacy-api.md` (crate + daemon + CLI surface), `docs/reviews/stream-d-{correctness,performance,security}-review.md` (Codex self-reviews), `docs/reviews/stream-d-claude-review.md` (Claude's adversarial review with blockers and recommendations).
- **System spec:** `docs/specs/system-v0.2.md` is the live parent system-level spec covering all streams (supersedes `system-v0.1.md`; brand decision "Memorum" is captured in §22). v0.1 is kept on disk for history.
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
- **G** Observability: TUI, localhost web dashboard, Reality Check, notifications, trust artifact rendering.
- **H** Eval harness.
- **I** Cross-session coordination: peer updates, presence, claim locks, and peer admin surfaces.

## Spec/plan conventions

- Spec and plan files are **versioned by suffix** (`-v1.1.md`, `-v0.5.md`, `-v0.4.md`). New versions supersede; old versions stay on disk for history. Never mutate an older version.
- Spec changes that affect the implementation contract get a version bump and a "Revision goal" entry at the top describing what changed and why.
- Plan changes get a "Plan revision history" entry. Plan revisions and spec revisions are independent counters; Stream E shipped from plan revision v0.4 against spec v0.5.
- Current live pairs:
  - **Stream A:** `stream-a-core-substrate-v1.1.md` ↔ `2026-04-26-stream-a-core-substrate-implementation-plan-v0.3.md` (shipped).
  - **Stream E:** `stream-e-passive-recall-v0.5.md` ↔ `2026-04-30-stream-e-passive-recall.md` (plan revision v0.4, shipped).
  - **Stream F:** `stream-f-dreaming-v0.2.md` ↔ `2026-04-30-stream-f-dreaming.md` (shipped).
  - **Stream G:** `stream-g-observability-v0.1.md` ↔ `2026-05-01-stream-g-observability.md` (implemented with canonical observability benchmark baseline promoted).
  - **Stream H:** `stream-h-eval-harness-v0.1.md` ↔ `2026-05-01-stream-h-eval-harness.md` (shipped; live real-harness validation remains auth-dependent).
  - **Stream I:** `stream-i-cross-session-v0.1.md` ↔ `2026-05-01-stream-i-cross-session.md` (shipped).
- If a spec and its plan drift apart on contract details (DTO shape, version string, deferral list, etc.), that's a bug — surface it.

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

## What's on disk (Streams A-E shipped, as of 2026-04-30)

- **`crates/memory-substrate/`** (Stream A) — public API (`api.rs` — Stream D added an `allow_encrypted_namespace` flag to `AtomicWrite` so plaintext writes still refuse `encrypted/` while supersede can update encrypted records), model + error taxonomy, frontmatter (parse/validate/serialize/defaults/schema), tree (layout/validate), config, IDs (sequence/repair), events (log/framing/sequence/recovery), index (schema/migrations/chunking/query — query.rs sanitizes FTS5 input, sqlite-vec adapter), git (init/adopt/preflight/commit/sync/command), watcher (subscription/filter/suppression), merge (three_way/quarantine), runtime (reconcile/faults), bench harness binary. ~30 integration test files including `spec_coverage_manifest.rs`, `crash_matrix.rs`, `startup_reconciliation.rs`, `vector_lifecycle.rs`, `fts_query_sanitization.rs`.
- **`crates/memoryd/`** (Streams B/C/D) — `cli.rs`, `client.rs`, `handlers.rs`, `main.rs`, `mcp.rs`, `mcp_stdio.rs`, `protocol.rs`, `server.rs`, `workers.rs`. Daemon serves newline-delimited JSON over a Unix socket with a 64 KiB frame cap, idle-frame timeout, watch::Receiver-driven graceful shutdown, and (Stream D) owner-only socket chmod after bind; `serve_substrate_with(socket, substrate, options, shutdown_rx)` is the supervised entry; SIGINT/SIGTERM wired in `main`. `memoryd mcp --socket <path>` is the launchable stdio MCP server: stdout is JSON-RPC protocol frames, stderr is logs, `tools/list` reflects `mcp::manifest()`, and `tools/call` routes through the daemon forwarder. The MCP forwarder declares nine agent-facing tools (Search/Get/Startup/Note/Observe plus governed Write/Supersede/Forget/Reveal); admin commands (`privacy`, `privacy-filter`, `device`, `review`, `web`, `reality-check`, peer admin) are CLI/socket-only and explicitly rejected from MCP. Stream D's `write_privacy_memory` runs `DeterministicPrivacyClassifier` over body+title+summary+source_ref+tags+privacy_descriptors before any disk effect; high-risk secrets refuse, PII encrypts at rest, and safe descriptors may be indexed for encrypted records. Startup recall is implemented for Stream E via daemon protocol, MCP forwarding, recall CLI hook commands, and additive status counters.
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
- **`docs/specs/stream-e-passive-recall-v0.{1,2,3,4,5}.md`** — Stream E spec history. v0.5 is the live contract; the others are kept on disk per the versioning convention. Each version's "Revision goal" block at the top documents what changed and why. **`docs/plans/2026-04-30-stream-e-passive-recall.md`** — Stream E implementation plan, shipped from plan revision v0.4 (see the file's Plan Revision History block). For current behavior, use the v0.5 spec plus `docs/api/stream-e-passive-recall-api.md`.

The full release gate is `bash scripts/check.sh` (with `BENCH_PROFILE=darwin-arm64` on Trey's machine).

## Running review or sanity-check work

Standard recipe when Trey asks "review this" or "is this ready":

1. Read the live spec and plan sections relevant to the question. For Stream A questions that's `stream-a-core-substrate-v1.1.md` + the v0.3 plan; for Stream E it's `stream-e-passive-recall-v0.5.md` + `docs/api/stream-e-passive-recall-api.md` and the shipped plan at v0.4 revision.
2. Read the actual files in the repo, not just the plan's description of them. Plan-reviewer caught three pre-build Stream E blockers (private `safe_plaintext_fragment` collision in `handlers.rs:1553`, missing `index_body` column for the recall-index API, and a doctor-vs-hot-path contradiction in §9.5) by reading the shipped code rather than trusting the plan's prose. Don't skip that step.
3. For plan reviews, brief the `plan-reviewer` subagent with the Codex-conventions caveat (subagent types, slash commands, `update_plan` are intentional). When the plan has been through prior reviews, tell plan-reviewer that explicitly so it doesn't waste cycles re-finding what's already fixed.
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
