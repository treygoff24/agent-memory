# Plan: 2026-05-04 — Streams F/G/H/I Dogfood Readiness

> **Goal:** close the remaining real gaps surfaced by five parallel sonnet read-only investigators (F, G, H, I, integration) on 2026-05-04 so Trey can dogfood Memorum on his own machine without surprises and so the streams can carry "shipped" honestly. Snapshot baseline: `4e87c70 Fix post-shipping audit findings`.
>
> This plan is **Codex-shaped**. Subagent type names (`heavy_worker`, `cli_developer`, `mcp_developer`, `test_hardener`, `performance_engineer`, `security_auditor`, `reviewer`, `plan_reviewer`, `code_mapper`, `docs_researcher`, `refactor_pilot`, `desloppify_*`), slash skills (`/clean-code`, `/refactor`), `update_plan`, and the per-task worktree workflow are intentional Codex conventions. See `docs/reference/codex-inventory-2026-05-03.md` for the live agent + skill inventory.

---

## Authoritative inputs

- **Spec gate** for any spec touch: `docs/reference/codex-inventory-2026-05-03.md` §3 globally-active skill `spec-quality-checklist`.
- **Live specs**: `docs/specs/stream-{a-v1.1,e-v0.5,f-v0.2,g-v0.1,h-v0.1,i-v0.1}.md`. System-level: `docs/specs/system-v0.2.md`.
- **Recent reviews driving this plan** (read these before any Phase ≥1 work):
  - `docs/reviews/2026-05-02-post-shipping-audit.md` (the audit doc; F-001…F-013 IDs are referenced throughout this plan)
  - `docs/reviews/stream-{f,g,i}-claude-{fresh-eyes,review,reviews}.md` (per-stream adversarial reviews)
  - `docs/reviews/stream-h-final-*` (eval-harness reviews)
- **Project gate** is `bash scripts/check.sh` with `BENCH_PROFILE=darwin-arm64` on Trey's machine. **Never** inside a per-task worktree — only on integrated `main` after `integrate-task-worktree.sh`.
- **CLAUDE.md** §"Critical invariants" — this plan must not violate any of them. Every write keeps `ClassificationOutcome`, `secret` is never persisted, embedding triple `(provider, model_ref, dimension)` is identity, baselines are human-only, etc.

---

## Decisions (resolved 2026-05-04)

Trey accepted all v2 default proposals on 2026-05-04. All decisions are locked; Codex executes against these values.

| # | Decision | Resolution | Rationale |
|---|---|---|---|
| D1 | T17 (lease re-entrancy) status | **DEFER to v1.1.** Spec-amend per Task 4.2. Catalog headline becomes "17 active + 2 deferred." | Solo dogfooding doesn't exercise re-entrancy enough to justify shipping the feature now. T17 stays in catalog as documentation of what'd be tested when the feature lands. |
| D2 | T18 (key rotation) status | **DEFER to v1.1.** Spec-amend per Task 4.2. | Key rotation is a 3–5 day Stream D cryptographic surface change. Solo dogfooding for the next ~3 months won't need it. Revisit when scaling to multi-device or sharing. |
| D3 | EncryptAtRest dream candidate policy | **REFUSE with explicit reason.** Pass 2 emits `candidate_refused: privacy_required_encryption`. `dream now` CLI groups refusal counts by reason. Implemented per Task 2.2. | Silent drops = surprise during dogfooding. Refusal makes the privacy boundary visible. Encrypt-at-rest in dreams would cross Stream D's intent that dream prose stay narrative-only — bigger redesign, deferred. |
| D4 | Default daemon coordination level | **STAY at Level 2.** Document explicitly that `conflicting_claim_locks` is `[]` at L1/L2 by design. Implemented per Task 3.3. | Conservative for solo dogfood. When a second device joins, flip to L3 explicitly via `concurrent_session_mode` config. |
| D6 | Scheduler scope | **macOS launchd template only.** No cron template. Implemented per Task 2.8. | Trey is darwin-arm64 single-platform for now. Add cron when Linux peer/server lands. |
| D7 | Live-LLM CI secrets | **Doc-only.** Runbook describes the env-var contract and how to wire GitHub secrets; actual secret setup is a manual admin step Trey does outside this plan. Implemented per Task 5.5. | Keeps the plan Codex-executable end-to-end. Trey can flip the switch in 10 minutes any day. |
| D8 | TUI recall-hits surface | **Dedicated 9th panel "Recall."** Histogram top + scrollable hit list bottom. Implemented per Tasks 1.2/1.3. | Recall is the headline feature of Stream G observability — deserves its own real estate. Timeline stays a mixed event view. |

(Decision D5 from v1 was "default-enable `stream-i-deps`" — removed in v2 because plan-reviewer found this is already true in `crates/memorum-eval/Cargo.toml:17`. D-numbers are otherwise stable to preserve task references.)

**Phase 0 decision gate is now satisfied.** Codex can proceed to Phase 1 after Phase 0.2 (plan review) completes — see Phase 0 status below.

---

## Roles

- **Coordinator (root Codex session):** owns the DAG, runs `update_plan`, integrates per-task worktrees onto `main`, runs `scripts/check.sh` on integrated trunk, owns `Cargo.lock` and `pnpm-lock.yaml` merges. No implementation work in the root session.
- **`plan_reviewer` (Codex, read-only, high reasoning, opus equivalent on Claude side):** runs Phase 0.2 adversarial review of this plan plus Phase 8.4 final read of integrated trunk.
- **Implementation agents (Codex):**
  - `heavy_worker` (workspace-write, high reasoning, danger-full-access) — TUI panel impl, Stream F policy implementation, cool-down registry refactor.
  - `worker` (workspace-write, low reasoning) — straightforward edits: `Eq` derives, attribute population, file deletions, doctor warnings.
  - `cli_developer` (workspace-write, high reasoning) — auth probe ergonomics, launchd plist template, install script, `--promote-canonical` enforcement.
  - `mcp_developer` (workspace-write, high reasoning) — any MCP-touching change (none expected; held in reserve for D8 panel data wiring).
  - `backend_arch` (workspace-write, high reasoning) — design doc for the TUI Recall panel (consumed by `heavy_worker`).
  - `test_hardener` (workspace-write, high reasoning) — close test gaps for new code; `claude-smoke` / `codex-smoke` live LLM tests.
  - `performance_engineer` (workspace-write, xhigh reasoning) — TUI bench replacement (real ratatui, not 144-byte mock).
  - `refactor_pilot` (workspace-write, high reasoning) — `is_tier1`/`is_tier3` rename, cleanup-bot LeaseGit refactor.
  - `code_mapper` (read-only, medium reasoning) — Phase 1.1 path/flow map for events_log mirror reader.
  - `docs_researcher` (read-only, medium reasoning) — getting-started + day-one runbook.
- **Review agents:**
  - `reviewer` (Codex, read-only, medium reasoning) — per-task diff review, runs after each implementation task before merge.
  - `security_auditor` (Codex, read-only, xhigh reasoning) — Phase 7.3 final security pass on Stream F policy and auth probe changes.
  - `claude-review` skill — cross-system second opinion, dispatched from coordinator at Phase 8.4.

**CPU discipline reminder** (per CLAUDE.md): each subagent runs **only its narrow gate** — `cargo check -p <pkg>` and `cargo test -p <pkg> -- --test-threads=2`. Coordinator runs `scripts/check.sh` once on integrated trunk. **Cap concurrent workspace-write subagents at 2** when each runs cargo gates; read-only agents can fan wider.

---

## Status legend

- 🔵 pending
- 🟡 in_progress (implementation)
- 🟠 in_review (Codex `reviewer` or Claude opus reviewer)
- 🔁 revising (after review)
- 🟢 done (impl + review approved + merged)
- 🔴 blocked
- 🟣 deferred (with explicit ticket)

---

## Live progress update (2026-05-04 Codex implementation pass)

This plan has been reconciled against the live tree rather than treated as all-pending. Current state after this implementation pass:

- Phase 1: done — Recall TUI panel, daemon socket request, panel tests, and panic restore hook are implemented.
- Phase 2: done — Stream F privacy refusal reason, auth probe diagnostics, `RepoPath::try_new` cleanup path, production Echo override hardening, `CleanupGit` seam, and launchd scheduling assets are in place.
- Phase 3: done — Stream I tier helper rename, Level 1/2 `conflicting_claim_locks` docs, and startup cool-down sharing are in place.
- Phase 4: done except human bench-promotion policy is enforced by script rather than historical-commit rewriting. Stale `.proposed` baselines were deleted; Specgate orphan warnings are resolved by removing obsolete Rust ownership stubs and adding the one JS ownership spec the installed Specgate can actually see.
- Phase 5: done for dogfood scope — T17/T18 are explicitly deferred, JSON reports carry `deferred`, live-harness smoke entry points exist and skip honestly without env vars, and CI secret docs exist.
- Phase 6: done — installer, launchd scheduler path, doctor harness warnings, and day-one runbook exist.
- Phase 7: partially satisfied locally — targeted clean-code/security review was performed on touched surfaces and recorded in final reports; the planned subagent swarm was not used in this single Codex pass.
- Phase 8: integrated in the main worktree (no task worktrees were spawned). Targeted gates are recorded in `docs/reviews/2026-05-04-final-gate-report.md`. Full `scripts/check.sh` was attempted once, failed on stale panel-range coverage, and was not rerun after the narrow fix per Trey instruction.

## DAG

```
Phase 0: Trey decisions D1–D8 → plan_reviewer → Trey approval
                  │
                  ▼
   ┌──────────────┴──────────────┐
   ▼                             ▼
Phase 1 (parallel):           Phase 2 (parallel):
  TUI recall observability      Stream F dogfood hardening
  (1.1 → 1.2 → 1.3, 1.4 par)    (2.1 → 2.2; 2.3, 2.4, 2.5, 2.6, 2.7, 2.8 par)
   │                             │
   └──────────────┬──────────────┘
                  ▼
Phase 3 (parallel after Phase 2):
  Stream I correctness (3.1, 3.2, 3.3, 3.4 par)
                  │
                  ▼
Phase 4 (parallel):
  Honesty & process (4.1, 4.2, 4.3, 4.4, 4.5 par; 4.2 needs D1/D2)
                  │
                  ▼
Phase 5 (parallel; depends on D1, D2, D7):
  Test truthfulness (5.4 par; 5.2/5.3 only if D1/D2 = implement; 5.5 par)
                  │
                  ▼
Phase 6 (serial-ish; depends on auth probe from 2.3):
  Onboarding (6.1 → 6.2 → 6.3)
                  │
                  ▼
Phase 7 (cross-cutting cleanup):
  desloppify-deep (7.1) → clean-code on new code (7.2) → security_auditor (7.3)
                  │
                  ▼
Phase 8 (integration):
  Coordinator merge → scripts/check.sh → bench review → final adversarial review
```

---

## Phase 0 — Plan adoption (gate)

| ID | Owner | Scope | Status |
|---|---|---|---|
| 0.1 | Trey | Answer D1–D8. | 🟢 done 2026-05-04 — accepted v2 defaults across the board (see "Decisions (resolved)" table above). |
| 0.2 | Claude `plan-reviewer` (opus) | Adversarial read against `docs/reference/codex-inventory-2026-05-03.md` + project context. | 🟢 done 2026-05-04 — verdict APPROVE-WITH-FIXES; 3 blockers + 6 risks/nits surfaced; all addressed in plan v2 (see Plan Revision History). |
| 0.3 | Trey | Final approval to start Phase 1. | 🟢 approved 2026-05-04. |

**Gate cleared.** Codex may begin Phase 1.

(Optional: a second-pass `plan_reviewer` (Codex, read-only, high reasoning) read against this v2/v3 plan can be done as a sanity check before Codex begins. Not required — Claude opus plan-reviewer already covered the same surface, and v2/v3 changes were scope-narrowing, not scope-expanding.)

---

## Phase 1 — TUI recall observability (Stream G headline gap)

**Why:** F-004 in the post-shipping audit is only half-fixed. The web dashboard has `/api/recall-hits`, but the TUI is dark. The TUI is the headline observability surface. Without this, dogfooding sees recall hits only via web, which inverts the "human visibility into agent recall" story.

**Owned files:** `crates/memoryd-tui/`, narrow additions to `crates/memoryd/src/recall_hit_query.rs` (new), tests under `crates/memoryd-tui/tests/`. **No** edits to `crates/memory-substrate/`.

| ID | Owner agent | Skills to load | Worktree | Status |
|---|---|---|---|---|
| 1.1 | `code_mapper` | — | `wt/task-1.1-events-log-mirror-map` (read-only, no commits) | 🔵 |
| 1.2 | `backend_arch` | `writing-plans`, `spec-quality-checklist` | `wt/task-1.2-tui-recall-design` (writes design doc only) | 🔵 |
| 1.3 | `heavy_worker` | `/clean-code`, `/rust-engineer` | `wt/task-1.3-tui-recall-impl` | 🔵 |
| 1.4 | `worker` | `/clean-code` | `wt/task-1.4-tui-panic-handler` | 🔵 |

### 1.1 — Map the events_log mirror reader path

Produce `docs/dev/recall-hit-event-flow.md` (≤300 lines) documenting:
- Where `EventKind::RecallHit` is emitted (caller → substrate).
- The `events_log` projection rows (columns, ordering invariants).
- The current daemon-side query path used by `/api/recall-hits` (`crates/memoryd-web/src/routes/recall_hits.rs`).
- Whether the existing query helper in `memoryd-web` is reusable from `memoryd-tui` directly, or needs to be lifted into `crates/memoryd/src/` as a shared module.

Output: `docs/dev/recall-hit-event-flow.md`. No code changes.
**Narrow gate:** `cargo check -p memoryd -p memoryd-web --tests` (must already pass; this task only reads).
**Acceptance:** Trey can read the doc and approve "this is the reusable shape." Hand off to 1.2.

### 1.2 — Design the Recall panel

Per **D8** = "dedicated 9th panel `Recall`". Produce `docs/dev/stream-g-tui-recall-panel-design.md` covering:
- Panel layout (ratatui widgets): time-bucket histogram top, scrollable hit list bottom with `mem_id`, `score`, `harness_source_id`, `surfaced_in_session`.
- Refresh model: poll-on-tab-activate vs. background subscriber. Recommend poll-on-activate + 5s interval while active, no background work when inactive (matches existing panels).
- Empty-state copy ("No recall hits yet — try a startup or supersede" — adjust as needed).
- Failure modes: unreachable daemon → red footer indicator (matches existing TUI convention); empty events_log → empty-state.
- Test plan: unit tests for the query module, a TUI snapshot test for the panel rendering with a fixture set of hits.

**Narrow gate:** none (design doc only).
**Acceptance:** doc exists, references concrete ratatui APIs, references concrete daemon protocol calls. `reviewer` approves.

### 1.3 — Implement the Recall panel

Implement per the design from 1.2.

**Important context from plan-reviewer:** `crates/memoryd/src/recall_hits.rs` already exists and the daemon already exposes `RequestPayload::RecallHits` over its socket. The web route at `crates/memoryd-web/src/routes/recall_hits.rs` is a socket-forwarder, not a duplicate query. The TUI should use the same socket request pattern via `DaemonClient`. **Do not** create a new shared module; **do not** assume the web route has duplicated logic to delete.

Files expected to be created:
- `crates/memoryd-tui/src/panels/recall.rs` — new panel module that issues `RequestPayload::RecallHits` via `DaemonClient` and renders the response.
- `crates/memoryd-tui/tests/recall_panel.rs` — unit + snapshot tests using a fixture daemon or recorded response.

Files expected to be edited:
- `crates/memoryd-tui/src/app.rs` — wire 9th panel, keybindings, status footer counter, replace the fixture string at `:804` with the real binding.

Files **NOT** to touch:
- `crates/memoryd/src/recall_hits.rs` — already correct; consume as-is.
- `crates/memoryd-web/src/routes/recall_hits.rs` — already correct; do not refactor.

**Narrow gate:** `cargo check -p memoryd-tui --tests --locked` and `cargo test -p memoryd-tui -- --test-threads=2`. Skip workspace gate.
**Acceptance:** `reviewer` (Codex, read-only) approves the diff against the design doc; tests pass; the existing fixture string at `app.rs:804` is replaced with real data binding wired to `DaemonClient`.

### 1.4 — Panic handler restores terminal

Add `std::panic::set_hook` in `crates/memoryd-tui/src/main.rs` that calls `TerminalGuard::restore_blocking()` (or equivalent — design as part of this task) before delegating to the default hook. The current `Drop` impl alone does **not** fire on panic across all code paths.

Add a `tests/panic_restore.rs` integration test that spawns the binary in a pty, induces a panic via a hidden `--inject-panic` debug flag (cfg-gated to `#[cfg(any(test, debug_assertions))]`), and asserts the terminal isn't left in raw mode.

**Narrow gate:** `cargo check -p memoryd-tui --tests --locked` and `cargo test -p memoryd-tui -- --test-threads=2`.
**Acceptance:** terminal restoration verified by the integration test; the debug flag is **not** present in release builds (compile-fail test confirms this).

---

## Phase 2 — Stream F dogfood hardening

**Why:** Five real surprises in dreaming during dogfooding (silent encrypted-at-rest drops, brittle auth probe, no scheduler, EchoCli reachable in production, RepoPath panic path, dropped Eq derive, cleanup-bot bypassing LeaseGit). All five would manifest within the first week of running dreams on a real machine.

| ID | Owner | Skills | Worktree | Status |
|---|---|---|---|---|
| 2.1 | `worker` | `spec-quality-checklist`, `write-human`, `writing-plans` | `wt/task-2.1-encrypted-at-rest-policy-spec` | 🔵 |
| 2.2 | `heavy_worker` | `/clean-code`, `/rust-engineer` | `wt/task-2.2-encrypted-at-rest-policy-impl` | 🔵 |
| 2.3 | `cli_developer` | `/clean-code` | `wt/task-2.3-auth-probe-ergonomics` | 🔵 |
| 2.4 | `worker` | `/clean-code` | `wt/task-2.4-repo-path-try-new` | 🔵 |
| 2.5 | `worker` | `/clean-code` | `wt/task-2.5-remove-echo-cli-prod` | 🔵 |
| 2.6 | — | — | — | 🟣 REMOVED in v2 — `Eq` already implemented in `crates/memory-substrate/src/config/mod.rs:34,153` |
| 2.7 | `refactor_pilot` | `/refactor`, `/clean-code` | `wt/task-2.7-cleanup-bot-cleanupgit` | 🔵 |
| 2.8 | `cli_developer` | `/clean-code`, `write-human` | `wt/task-2.8-launchd-template` | 🔵 |

### 2.1 — Spec-amend the EncryptAtRest dream policy (D3 dependency)

`worker` with `spec-quality-checklist` + `write-human` + `writing-plans` skills loaded. (v1 assigned `prompt_engineer` which is read-only per the inventory — wrong agent for a spec patch; corrected in v2.)

Per **D3** default = refuse. Patch `docs/specs/stream-f-dreaming-v0.2.md` adding a new §X.Y "Pass 2 candidate privacy policy" with:
- Pass 2 candidates run the full deterministic privacy classifier (already true).
- `EncryptAtRest`-classified candidates are **refused** with `candidate_refused: privacy_required_encryption` reason in `PassOutcome.candidate_results`.
- `Refuse`-classified candidates remain refused (already true).
- `Plaintext`/`Trusted` candidates may proceed (already true).
- Operator-visible: dream summary should show refusal count by reason.

Bump spec to **v0.3** (`docs/specs/stream-f-dreaming-v0.3.md`); leave v0.2 on disk per versioning convention. Add Revision goal block at top.

**Narrow gate:** none (spec only).
**Acceptance:** `plan_reviewer` (Codex) approves the spec patch; references Stream D's `storage_action()` semantics correctly.

### 2.2 — Implement EncryptAtRest refusal

Per spec v0.3 from 2.1.

Files expected to be edited:
- `crates/memoryd/src/dream/pass2.rs` (or wherever `candidate_storage_action` lives — `code_mapper` should pre-establish if unclear).
- `crates/memoryd/src/dream/types.rs` — add `EncryptedAtRestRefusal` reason variant.
- Tests: `crates/memoryd/tests/dream_pass2_privacy.rs` — at minimum, one fixture observation that classifies `EncryptAtRest` and asserts the refusal flows through with the correct reason.

**Narrow gate:** `cargo check -p memoryd --tests` + `cargo test -p memoryd dream -- --test-threads=2`.
**Acceptance:** spec-implementation parity verified by `reviewer`; refusal reason surfaces in `dream now` CLI output.

### 2.3 — Auth probe ergonomics

Stream F's auth probe (`claude config get auth.user`) silently fails when the daemon's PATH or env doesn't see the CLI the user thinks is installed. Make it loud and diagnosable.

Files expected to be edited:
- `crates/memoryd/src/dream/harness.rs` — replace silent boolean with a typed result `enum AuthProbeResult { Ok, CliMissing { which: &'static str }, AuthFailed { exit_code: i32, stderr_tail: String }, Timeout }`.
- `crates/memoryd/src/cli.rs` `dream now` command — print the typed result with operator-readable text. If `CliMissing`, suggest `which claude` and the `PATH` the daemon sees.
- `crates/memoryd/src/handlers.rs` doctor handler — same typed result feeds doctor output (Phase 6 will further surface this).

**Narrow gate:** `cargo check -p memoryd --tests` + `cargo test -p memoryd dream -- --test-threads=2`.
**Acceptance:** running `memoryd dream now --scope me` with `claude` not on PATH prints a clear, copy-pasteable error including the daemon's effective `PATH`.

### 2.4 — `RepoPath::new` → `try_new` in cleanup

Per Claude Stream F review B6 partial fix: `crates/memoryd/src/dream/cleanup.rs:638` (or wherever `repo_path_from_relative` is now) still uses `RepoPath::new` — confirm via `code_mapper` if the line moved.

Replace with `try_new` and propagate the error. Add a unit test that injects a malformed device-id-shaped path and asserts the error is returned, not a panic.

**Narrow gate:** `cargo check -p memoryd --tests` + `cargo test -p memoryd dream::cleanup -- --test-threads=2`.
**Acceptance:** no remaining `RepoPath::new` callsite in `crates/memoryd/src/dream/`.

### 2.5 — Remove `EchoCli` from production builds

`crates/memoryd/src/dream/harness.rs` (or wherever `EchoCli` lives). Wrap the variant and `--cli echo` CLI option in `#[cfg(any(test, feature = "dev-fixtures"))]`. Ensure `Cargo.toml` `dev-fixtures` feature is dev-only and not bundled into release.

Add a compile-fail test (or a `#[test]` that asserts `EchoCli` is unreachable from a `--release`-equivalent feature set).

**Narrow gate:** `cargo check -p memoryd --tests` + `cargo build -p memoryd --release` (must not include EchoCli string in `--help` output).
**Acceptance:** `cargo run --release -p memoryd -- dream now --cli echo` errors with "unknown harness" rather than running.

### 2.6 — REMOVED in v2

Plan-reviewer found `Eq` is already implemented for both `SyncedConfig` and `DreamsConfig` at `crates/memory-substrate/src/config/mod.rs:34` and `:153`. R12 is closed. No action needed.

### 2.7 — cleanup-bot via `CleanupGit` trait

Per Stream F review R9: `crates/memoryd/src/dream/cleanup.rs:534` and `:551` invoke `Command::new("git")` directly. This bypasses the test seam.

`crates/memoryd/src/dream/cleanup.rs:524` already defines a `CleanupGit` trait, with `RealCleanupGit` at `:530`. The fix is to route the direct `Command::new("git")` calls at `:534` and `:551` through the trait — either by adding the missing operations to `CleanupGit` and implementing them on `RealCleanupGit`, or by moving the existing `Command::new` calls inside `RealCleanupGit::*` methods so cleanup paths consume them through the trait.

**Do not** confuse `CleanupGit` with `LeaseGit` (which lives at `crates/memoryd/src/dream/git.rs:12` and serves a different surface). The agent should pre-confirm via `code_mapper` if needed.

Add a fixture-based test that asserts the cleanup commit follows `memoryd cleanup-bot` author conventions and is exercised through the `CleanupGit` trait (substituting a fake impl).

**Behavior-preserving** at the user-observable level (commit message format, files touched). This is a `refactor_pilot` task. Skip workspace gate; run `cargo test -p memoryd dream::cleanup -- --test-threads=2`.

### 2.8 — launchd plist template (D6)

Per **D6** = launchd-only. Add `scripts/templates/com.memorum.dream-scheduled.plist.template` with `{{REPO_PATH}}` and `{{RUNTIME_PATH}}` placeholders, intended schedule = daily 03:00 local.

Add `scripts/install-launchd.sh` that interpolates the placeholders from CLI args, copies to `~/Library/LaunchAgents/`, and `launchctl load`s it. Idempotent on re-run.

Document in `docs/runbooks/dream-scheduling.md` (new): when to install, how to inspect, how to uninstall (`launchctl unload`).

**Narrow gate:** `bash scripts/install-launchd.sh --dry-run --repo /tmp/foo --runtime /tmp/bar` produces the expected plist on stdout. No `cargo` work.
**Acceptance:** dry-run output is correct; runbook explains all three operations.

---

## Phase 3 — Stream I correctness fixes

**Why:** Four real correctness/clarity gaps in cross-session coordination. None block dogfooding (Stream I is dormant on a single device), but they'll bite the moment a second device joins, and one (entity-recall empty attribute) is a silent contract drift that'll surprise downstream consumers.

| ID | Owner | Skills | Worktree | Status |
|---|---|---|---|---|
| 3.1 | `refactor_pilot` | `/refactor`, `/clean-code` | `wt/task-3.1-tier-naming-rename` | 🔵 |
| 3.2 | — | — | — | 🟣 REMOVED in v2 — `<entity-recall entities="…">` already populated correctly via `crates/memoryd/src/recall/startup.rs:176` and `render.rs:295-304` |
| 3.3 | `worker` | `/clean-code`, `write-human` | `wt/task-3.3-conflicting-claim-locks-doc` | 🔵 |
| 3.4 | `heavy_worker` | `/clean-code`, `/rust-engineer` | `wt/task-3.4-cooldown-shared-registry` | 🔵 |

### 3.1 — Rename `is_tier1`/`is_tier3`

`crates/memorum-coordination/src/session.rs:104-110`. Current naming is inverted relative to the spec's "Tier 1/2/3 capable harness" mental model. Rename to `is_full_coordination_harness()` and `is_observe_only_harness()` (or naming TBD by the agent — propose two alternatives in the diff message).

Update all callsites including `crates/memorum-coordination/src/gate.rs:48-50`. Behavior-preserving.

**Narrow gate:** `cargo check -p memorum-coordination --tests` + `cargo test -p memorum-coordination -- --test-threads=2`.
**Acceptance:** `reviewer` approves; `git grep is_tier1\|is_tier3` returns nothing.

### 3.2 — REMOVED in v2

Plan-reviewer found this is already correct in current code: `crates/memoryd/src/recall/startup.rs:176` passes `salient_entities` into `StartupCoordinationRender`, and `render.rs:295-304` populates the attribute. The original I-R3 finding was resolved in `d9628cb`. No action needed.

(If you want a regression test guarding the populated path, `test_hardener` can add one as a small follow-up after Phase 1 — not blocking for dogfood readiness.)

### 3.3 — `conflicting_claim_locks` scope (D4)

Per **D4** = stay at L2 default. Document explicitly. No code change to `crates/memoryd/src/handlers.rs:604-614` other than expanding the inline comment to reference the spec's L3-only contract. Update `docs/api/stream-i-cross-session-api.md` "Status response" section to call out that `conflicting_claim_locks` is `[]` at coordination levels 1 and 2 by design.

**Narrow gate:** none (doc + comment only); `cargo check -p memoryd` to confirm comment didn't introduce a syntax error.

### 3.4 — Shared cool-down registry

`crates/memoryd/src/recall/startup.rs:283,309`. The same `startup_context.clone()` is used for both same-device and cross-device passes; `record_surfaced_peer_write` mutations to the clone are discarded.

Refactor to either:
- (a) lift the cool-down registry into `Arc<Mutex<…>>` owned by the parent context, both passes hold a clone of the Arc; or
- (b) run passes serially and pass `&mut startup_context` through.

Option (a) is preferred because passes can stay logically parallel. `heavy_worker` proposes option in diff message; `reviewer` ratifies.

Add a regression test: a single memory that scores above threshold in both same-device and cross-device categories should appear **once** in a single startup, not twice.

**Narrow gate:** `cargo check -p memoryd --tests` + `cargo test -p memoryd recall::startup -- --test-threads=2`.

---

## Phase 4 — Honesty & process

**Why:** Stale `.proposed` files, F-011 (observed_at typed promotion), F-013 (specgate orphaned_specs warnings), the d9628cb violation that bypassed the `--promote-canonical` safeguard, and T17/T18 formal deferral are all open from the post-shipping audit. None block runtime; all erode signal-to-noise in the gate and in the test catalog.

| ID | Owner | Skills | Worktree | Status |
|---|---|---|---|---|
| 4.1 | `worker` | — | `wt/task-4.1-delete-proposed-baselines` | 🔵 |
| 4.2 | `worker` (driven by D1/D2) | `spec-quality-checklist`, `write-human` | `wt/task-4.2-t17-t18-deferral-spec` | 🔵 |
| 4.3 | — | — | — | 🟣 REMOVED in v2 — F-011 already fixed; `crates/memory-substrate/src/index/query.rs:917-919` already reads from typed `Frontmatter::observed_at` |
| 4.4 | `worker` (with `code_mapper` pre-step) | `/clean-code` | `wt/task-4.4-specgate-warnings` | 🔵 |
| 4.5 | `cli_developer` + `test_hardener` | `/clean-code` | `wt/task-4.5-promote-canonical-enforcement` | 🔵 |
| 4.6 | `docs_researcher` | `spec-quality-checklist`, `write-human` | `wt/task-4.6-f003-ratification-audit` (read-only) | 🔵 (new in v2) |

### 4.1 — Delete stale `.proposed` baselines

```
bench/stream-g-observability-results.darwin-arm64.json.proposed
bench/stream-i-cross-session-results.darwin-arm64.json.proposed
```

Both have no canonical purpose; both were left on disk by Codex's autonomous run and never promoted/deleted. Confirm via `git log --oneline -- <path>` that no commit references the `.proposed` content.

**Narrow gate:** none (file deletion only).
**Acceptance:** `ls bench/*.proposed 2>/dev/null` returns empty. Commit message: "Remove stale `.proposed` baselines from Codex autonomous run".

### 4.2 — T17/T18 formal deferral (D1, D2)

Per **D1** + **D2** defaults = defer. Patch:
- `docs/specs/stream-h-eval-harness-v0.1.md` — add §"Deferred to v1.1" listing T17 (lease re-entrancy) and T18 (key rotation) with the required upstream prerequisites (`Stream F same-device re-entrant lease` + `key rotation flow design`).
- `docs/api/stream-h-eval-api.md` — update "19-test catalog" claim to "17 active + 2 deferred" or similar honest framing. Trey's call on naming.
- `crates/memorum-eval/src/orchestrator.rs` — the existing `MEMORUM_EVAL_SKIP:…` skip markers stay in place. Add catalog-level `deferred: true` field on T17/T18 in the test definition struct so JSON reports distinguish "deferred" from "skipped due to env."

If **D1** or **D2** = implement, this task is replaced by 5.2/5.3 (see Phase 5) and 4.2 becomes a no-op.

**Narrow gate:** `cargo check -p memorum-eval --tests` if test definitions are touched; otherwise none.
**Acceptance:** spec carries deferred-list; eval JSON output distinguishes deferred from skipped.

### 4.3 — REMOVED in v2

Plan-reviewer found F-011 is already fixed: `crates/memory-substrate/src/index/query.rs:917-919` reads directly from the typed field via `memory.frontmatter.observed_at.as_ref().map(chrono::DateTime::to_rfc3339)`. The integration agent's flag of `observed_at_for_index(memory)` at `:569` was a stale reference; the actual function body at `:917-919` is correct.

This task was the plan's only authorized Stream A touch. Removing it means **this plan touches no Stream A modules** — Phase 4.3 authorization in the constraints section is dropped.

### 4.4 — F-013: specgate orphaned_specs warnings

Per audit F-013. `bash scripts/check.sh` emits six specgate `orphaned_specs` warnings against Stream A modules.

**Pre-step (`code_mapper`, read-only):** before any edits, map specgate's matching logic — why globs match files that exist but the linker still treats them as orphaned. Output: a one-page memo identifying the root cause (module-spec.yml drift, glob mismatch, registry-key mismatch, etc.). Hand off to `worker` with the conclusion.

**Implementation (`worker`):** triage each warning:
- If the spec module is now obsolete (superseded by a different module): delete with rationale in commit message.
- If the spec module is current but not referenced: add the reference in the canonical owning module's `module-spec.yml` (or whatever the linker actually consumes per the `code_mapper` memo).

**Narrow gate:** `bash scripts/check.sh` produces zero specgate warnings (run from the worktree once at the very end, since this task affects gate output specifically). This is the **one** Phase 4 task that should run a fuller check, scoped to specgate output only.
**Acceptance:** zero `orphaned_specs` warnings.

### 4.5 — Enforce `--promote-canonical` end-to-end

Per audit F-006. `4e87c70` added `--promote-canonical` to bench writers, but `d9628cb` (a fix commit landed earlier in the same batch) updated the canonical bench file directly, violating the safeguard the next commit was about to introduce.

- `cli_developer` half: confirm every bench writer in `crates/memory-substrate/src/bin/` and any Stream G/H/I bench binary checks the flag and refuses to overwrite canonical without it. Add an explicit error message naming the file path.
- `test_hardener` half: add a CI lint script `scripts/check-baseline-discipline.sh` that fails when `bench/baseline.*.json` or `bench/*-results.*.json` (canonical names) appear in a commit that doesn't have a `[bench-update]` tag in the commit message, and fails when both canonical and `.proposed` are committed in the same commit. Wire into `scripts/check.sh` as an optional pre-gate step.

**Narrow gate:** `bash scripts/check-baseline-discipline.sh` returns 0 against current `main`. Targeted dry-run against a synthetic violating commit returns non-zero.

### 4.6 — F-003 ratification audit (new in v2)

Per audit F-003. Two public Stream A APIs were added without explicit spec authorization during the autonomous run: `update_encrypted_memory_metadata` (`crates/memory-substrate/src/api.rs:685`) and `query_recall_index_including_metadata_only` (`crates/memory-substrate/src/api.rs:1148`). Commit `4e87c70` ratified them via doc additions, but no one has verified the ratification is **thorough**.

`docs_researcher` (read-only) audits the ratification:
- Read the post-v1.1 additions section in `docs/specs/stream-a-core-substrate-v1.1.md`.
- Read `docs/api/stream-a-public-api.md`.
- Diff against the actual API surface in `crates/memory-substrate/src/api.rs:685` and `:1148` — function signatures, error variants, behavior contracts, invariants.
- Check whether the spec should bump to `v1.2` (new public-API surface arguably warrants a minor version bump per the spec/plan versioning convention).

Output: `docs/reviews/2026-05-04-f003-ratification-audit.md` with verdict (`fully ratified` / `partially ratified, gaps listed` / `not ratified`) and a recommendation on the v1.1 → v1.2 question.

If the audit surfaces material doc gaps, escalate to Trey for a follow-up plan; do **not** edit `crates/memory-substrate/` from this task. This is read-only.

**Narrow gate:** none (read-only doc task).
**Acceptance:** the audit doc exists; verdict is one of the three values; gaps (if any) are line-numbered.

---

## Phase 5 — Test truthfulness

**Why:** Two real gaps in eval coverage: T17/T18 (decided in D1/D2) and zero live-LLM assertions. (T19's feature flag — original v1 D5 — was already default-enabled in main; verified by plan-reviewer.)

| ID | Owner | Skills | Worktree | Status |
|---|---|---|---|---|
| 5.1 | — | — | — | 🟣 REMOVED in v2 — already default-enabled in `crates/memorum-eval/Cargo.toml:17` |
| 5.2 | `heavy_worker` | `/rust-engineer`, `/tdd` | `wt/task-5.2-t17-impl` | 🟣 (only if D1 = implement) |
| 5.3 | `heavy_worker` | `/rust-engineer`, `/tdd` | `wt/task-5.3-t18-impl` | 🟣 (only if D2 = implement) |
| 5.4 | `test_hardener` | `/tdd`, `/clean-code` | `wt/task-5.4-claude-codex-smoke` | 🔵 |
| 5.5 | `cli_developer` | `write-human` | `wt/task-5.5-real-harness-ci-doc` | 🔵 |

### 5.1 — REMOVED in v2

Plan-reviewer found `stream-i-deps` is already in `default = [...]` in `crates/memorum-eval/Cargo.toml:17`, and `.github/workflows/stream-h-eval.yml` no longer carries an explicit `--features stream-i-deps` flag. T19 already runs in the standard build path. No action needed.

### 5.2/5.3 — T17/T18 implementation (deferred unless D1/D2 = implement)

Skipped under default decisions. If invoked, expand into a follow-up plan; do not inline the design here.

### 5.4 — `claude-smoke` and `codex-smoke` live LLM tests

A small `cargo test --features live-harness -p memorum-eval -- live::` subset that:
- Skips when `MEMORUM_EVAL_CLAUDE_KEY` (resp. `…CODEX_KEY`) env var is absent (matches the existing skip pattern).
- When present, runs **one** end-to-end: T13 substrate sharing for `claude-smoke`, T15 privacy refusal+retry for `codex-smoke`. Asserts the recall-block XML round-trips and that the privacy refusal surfaces correctly.
- Uses a temporary repo + runtime under `tempfile::tempdir()` so it never writes to a user's real Memorum state.

This is **not** the full T13/T15 suite — that requires more setup. This is a sanity smoke that proves the wire is alive end-to-end with a real CLI.

**Narrow gate:** `cargo check -p memorum-eval --features live-harness --tests`. Test execution skips without env vars; locally verify with both env vars set.
**Acceptance:** `MEMORUM_EVAL_CLAUDE_KEY=… cargo test -p memorum-eval --features live-harness -- live::claude_smoke` passes against a real authenticated Claude CLI on Trey's machine.

### 5.5 — Real-harness CI documentation (D7)

Per **D7** = doc only, no actual secret addition. Author `docs/runbooks/eval-real-harness-ci.md` describing:
- Required GitHub secrets (`MEMORUM_EVAL_CLAUDE_KEY`, `MEMORUM_EVAL_CODEX_KEY`).
- How `.github/workflows/stream-h-eval.yml` consumes them.
- The `if: ${{ secrets.MEMORUM_EVAL_CLAUDE_KEY != '' }}` guard pattern.
- Cost expectations (rough order of magnitude per run).
- Recommended cadence (the existing daily 3 AM scheduled run is fine; do not bombard).

**Narrow gate:** none (doc only).
**Acceptance:** runbook exists, links from `docs/api/stream-h-eval-api.md`.

---

## Phase 6 — Onboarding

**Why:** A first-run user (Trey, day one) should hit zero cliffs. Currently: missing harness CLI → `dream now` confusing failure (fixed by 2.3); no install script; doctor is silent on harness CLI status. This phase closes those.

| ID | Owner | Skills | Worktree | Status |
|---|---|---|---|---|
| 6.1 | `cli_developer` | `/clean-code` | `wt/task-6.1-install-script` | 🔵 |
| 6.2 | `worker` | `/clean-code` | `wt/task-6.2-doctor-harness-warn` | 🔵 (depends on 2.3) |
| 6.3 | `docs_researcher` | `write-human`, `writing-plans` | `wt/task-6.3-day-one-runbook` | 🔵 (depends on 6.1) |

### 6.1 — `scripts/install-memorum.sh`

Single-script installer. Acceptance: a fresh user with Rust toolchain runs it and ends with a working daemon, an MCP-ready socket, and a printed MCP client snippet.

Steps the script performs (idempotent on re-run):
1. `cargo install --path crates/memoryd` (skip if `memoryd --version` already matches `Cargo.toml`).
2. `mkdir -p $REPO $RUNTIME` (default `~/memorum`, `~/memorum/.memoryd`).
3. `memoryd serve --init --repo $REPO --runtime $RUNTIME --socket /tmp/memoryd.sock` in the background, wait for socket readiness via `memoryd status` polling (5s timeout).
4. Print MCP client config snippet to stdout, ready to paste.
5. If `claude` or `codex` CLI on PATH, print "✓ harness CLI detected: claude" or similar; if neither, print a soft warning explaining that dreams will be inactive until one is installed (link to `docs/runbooks/dream-scheduling.md`).
6. Optionally invoke `scripts/install-launchd.sh` (Phase 2.8) when `--with-scheduler` is passed.

**Narrow gate:** shellcheck-clean; dry-run against a temp HOME succeeds.
**Acceptance:** Trey runs it on his machine and gets a working daemon end-to-end with no follow-up configuration.

### 6.2 — `memoryd doctor` warns on missing harness CLI

Extend `memoryd doctor` (in `crates/memoryd/src/handlers.rs`) to call the typed auth probe from 2.3 and surface its result alongside existing checks:
- `claude CLI: ✓ authenticated` / `✗ not on PATH (dreams disabled)` / `✗ auth probe failed: <stderr tail>`
- Same for `codex CLI`.
- Exit code 0 if at least one harness is available; exit code 1 only when **all** doctor checks fail (preserve existing semantics — missing harness alone is a warning, not a failure).

**Narrow gate:** `cargo check -p memoryd --tests` + `cargo test -p memoryd doctor -- --test-threads=2`.

### 6.3 — Day-one runbook

`docs/runbooks/dogfooding-day-one.md`. Walks Trey through:
1. Install (`scripts/install-memorum.sh`).
2. Wire MCP client (snippet from install script).
3. First memory: `memoryd write-note "I dogfooded Memorum on 2026-05-04."`
4. First search: `memoryd search "dogfood"`.
5. Optional: install scheduler (`scripts/install-launchd.sh`).
6. Optional: run a manual dream (`memoryd dream now --scope me`).
7. View web dashboard (`memoryd web enable && open http://127.0.0.1:7137`).
8. Run weekly Reality Check (`memoryd reality-check run`).
9. View TUI (`memoryd ui`) — should now show the Recall panel from Phase 1.
10. Troubleshooting: most common errors and what they mean.

**Narrow gate:** none (doc only).
**Acceptance:** Trey reads it and feels confident this is the path.

---

## Phase 7 — Cross-cutting cleanup

| ID | Owner | Skills | Worktree | Status |
|---|---|---|---|---|
| 7.1 | `desloppify-deep` coordinator + 8 axis subagents | `desloppify-deep`, all 8 axis skills | `wt/task-7.1-desloppify-deep` (single worktree, 8 subagents) | 🔵 |
| 7.2 | `worker` | `/clean-code` | `wt/task-7.2-clean-code-new-surfaces` | 🔵 |
| 7.3 | `security_auditor` | — | `wt/task-7.3-security-audit-final` (read-only) | 🔵 |

### 7.1 — `desloppify-deep` 8-axis sweep

Run the `desloppify-deep` coordinator skill against the streams F/G/H/I crates touched by Phases 1–6:
- `crates/memoryd/`
- `crates/memoryd-tui/`
- `crates/memoryd-web/`
- `crates/memorum-coordination/`
- `crates/memorum-eval/`
- `crates/memory-governance/` (Stream C, in case Phase 4.3 touched it)

**CPU discipline reminder per CLAUDE.md:** all 8 axis subagents must be briefed `"do not run scripts/check.sh; do not run cargo test --workspace; run only cargo check -p <pkg>"`. Coordinator runs `bash scripts/check.sh` once at the end.

Pre-existing canonical 8 axes: dedup, types, dead-code, cycles, weak-types, defensive, legacy, comments. The coordinator skill knows the order and dependencies.

**Narrow gate:** coordinator's single `bash scripts/check.sh` after merge of all 8 axis branches.
**Acceptance:** zero new clippy warnings; reviewer approves the integrated diff.

### 7.2 — `clean-code` pass on Phase 1 surfaces

Specifically: the new TUI Recall panel and shared `recall_hit_query` module from Phase 1.3, plus the panic handler from 1.4. Apply `/clean-code` patterns Codex's clean-code skill enforces.

**Narrow gate:** `cargo check -p memoryd -p memoryd-tui --tests` + the targeted test invocations from Phase 1 narrow gates.

### 7.3 — Final security pass

Read-only `security_auditor` review focused on:
- Stream F EncryptAtRest refusal flow from 2.2 — does it accidentally surface the refused candidate's content in a refusal reason or log line?
- Auth probe stderr handling from 2.3 — is the daemon copying user-supplied error text into operator-facing output without sanitization?
- Install script from 6.1 — any path injection or environment-variable trust issues?
- Doctor output from 6.2 — leak of sensitive path or env content?

Output: `docs/reviews/2026-05-04-final-security-audit.md` with verdict + per-finding severity. Coordinator gates on no `Blocker`-severity findings.

---

## Phase 8 — Integration & gate

| ID | Owner | Status |
|---|---|---|
| 8.1 | Coordinator | 🔵 |
| 8.2 | Coordinator | 🔵 |
| 8.3 | Coordinator (human gate) | 🔵 |
| 8.4 | `plan_reviewer` (Codex) + Claude `plan-reviewer` (opus) + `claude-review` skill | 🔵 |

### 8.1 — Merge to `main`

For each task branch (in dependency order from the DAG), run `scripts/integrate-task-worktree.sh`. Coordinator owns `Cargo.lock` merges. Workers must not have touched it.

### 8.2 — Full release gate on integrated trunk

```bash
BENCH_PROFILE=darwin-arm64 bash scripts/check.sh
```

Must pass with:
- Zero clippy warnings.
- Zero rustfmt diffs.
- Zero rustdoc warnings.
- Zero specgate `orphaned_specs` warnings (per Phase 4.4).
- All `cargo test --workspace --locked` green.
- Bench gate against committed canonical baselines (per Phase 4.5).

If anything fails, do **not** paper over — fix at root and re-run.

### 8.3 — Bench re-baseline review (human gate)

Per CLAUDE.md "performance baselines are updated only by explicit human-authored commits." Phases 1.3, 7.1, and 7.2 may shift Stream G TUI bench numbers (Phase 1 adds a real panel; if Phase G replaces synthetic bench, this gate matters extra). Trey reviews the new `.proposed` numbers and decides whether to promote to canonical via a human-authored commit.

**Optional sub-task** (not required for dogfood readiness): replace the TUI synthetic bench (`tui_panel_switch: 0.001ms` placeholder) with a real ratatui-driven bench. If chosen, owned by `performance_engineer`, gated by Trey's bench review here. Otherwise document the synthetic shortcut in spec v0.2 (already done by `cf9736f` per the Stream G investigator).

### 8.4 — Final adversarial review

Three parallel reads:
- `plan_reviewer` (Codex, read-only, high reasoning) — DAG closure check, gate coverage, invariant compliance on integrated `main`.
- Claude `plan-reviewer` (opus) — same brief, fresh context, second-opinion read.
- `claude-review` skill from coordinator (Codex → Claude via `claudish`) — open-ended cross-system read.

Consolidate findings. Any Blocker-severity finding loops back to the appropriate phase; non-blockers go in `docs/reviews/2026-05-04-final-gate-report.md` for tracking.

**Final acceptance:** `docs/reviews/2026-05-04-final-gate-report.md` exists, all three reviewers have signed off, `bash scripts/check.sh` is green on `main`.

---

## Constraints for impl agents (recap)

1. **Do not run `scripts/check.sh`** inside any task worktree. Coordinator owns it.
2. **Run only narrow gates:** `cargo check -p <pkg> --tests --locked` and `cargo test -p <pkg> -- --test-threads=2`. Per CLAUDE.md CPU discipline: cap test threads at 2.
3. **Do not modify `crates/memory-substrate/`.** This plan authorizes **zero** Stream A touches. (v1 had Phase 4.3 as the sole authorized touch; plan-reviewer found that fix was already done in current code, so 4.3 was removed in v2.)
4. **Do not touch `Cargo.lock` or `pnpm-lock.yaml`.** Coordinator merges these at integration time. Touch only `Cargo.toml`.
5. **One commit per task** with a clear message. Reference the task ID (`Task 2.3:` etc.) and the audit/review ID where applicable (`F-011`, `R5`, etc.).
6. **No new dependencies without justification.** Add to the diff message if you add one.
7. **Behavior-preserving on bench fixtures.** Re-running benches is fine; arbitrarily revising what they measure is a design change requiring confirmation.
8. **Each agent reports a structured diff summary** at completion: file list, brief description per change, rationale, test additions, narrow gate output.

## Constraints for review agents (recap)

1. Look for: behavior preservation, scope discipline (no out-of-task edits), idiom (`/rust-engineer` patterns), edge cases the impl agent missed, test coverage of the fix.
2. Distinguish **must-fix** from **nice-to-have**. Impl agents do another revision pass only on must-fix.
3. Cap review at ~800 words per task. Be specific.
4. Cite line numbers in `crate/path/file.rs:LINE` form so Trey can jump to it.

---

## Open questions / known unknowns

- **Phase 8.3 TUI bench replacement:** real ratatui bench is a non-trivial subtask. Acceptable to defer to v1.1 explicitly with spec amendment; flag for Trey at Phase 8 if `performance_engineer` says it's > a day's work.
- **Phase 1.1 conclusion drives 1.2/1.3 shape.** Plan-reviewer confirmed the daemon already exposes `RequestPayload::RecallHits` and `crates/memoryd/src/recall_hits.rs` is the right consumer point. 1.1 should reach the same conclusion; if it doesn't, escalate before 1.3 starts.
- **Phase 4.6 escalation path:** if F-003 audit finds material doc gaps (signature drift, missing error-variant docs, missing behavior contracts), escalate to Trey for a follow-up plan rather than editing Stream A from this plan.
- **Phase 7.1 desloppify-deep risks finding cross-stream issues** that exceed task scope. If an axis subagent surfaces something out of scope, it stops and reports rather than expanding.

---

## Plan revision history

- **v3 (2026-05-04, Claude, post Trey decisions):** all 7 outstanding decisions (D1–D4, D6, D7, D8) resolved by Trey accepting v2 default proposals. "Decisions required" table re-titled "Decisions (resolved)" with chosen values + per-decision rationale captured. Phase 0 gate marked cleared (0.1 🟢 done by Trey accepting defaults; 0.2 🟢 done by the Claude opus plan-reviewer pass that drove v2; 0.3 🟢 approved). Optional Codex `plan_reviewer` second-pass noted as not required because v2/v3 changes were strictly scope-narrowing. Codex may now begin Phase 1.
- **v2 (2026-05-04, Claude, post plan-reviewer):** dropped four tasks the adversarial plan-reviewer found chasing already-fixed bugs, plus one risk:
  - **Removed 2.6** — `Eq` already implemented on `SyncedConfig` and `DreamsConfig` (`crates/memory-substrate/src/config/mod.rs:34,153`).
  - **Removed 3.2** — `<entity-recall entities="…">` already populated correctly via `crates/memoryd/src/recall/startup.rs:176` and `render.rs:295-304`.
  - **Removed 4.3** — F-011 already fixed; `query.rs:917-919` reads from typed `Frontmatter::observed_at`. Net effect: this plan now touches **zero** Stream A modules.
  - **Removed 5.1** — `stream-i-deps` already in `default = […]` in `Cargo.toml:17`.
  - **Decision D5 dropped** (no longer needed). D-numbers otherwise stable.
  - **Reframed 1.3** — daemon already exposes `RequestPayload::RecallHits` and `crates/memoryd/src/recall_hits.rs` is the consumer point; the TUI consumes the existing socket payload, no new shared module created, no web refactor.
  - **Fixed 2.7 brief** — refactor through `CleanupGit` trait at `cleanup.rs:524`, not `LeaseGit`. Direct calls live at `cleanup.rs:534,551`.
  - **Fixed 2.1 owner** — `worker` (write-capable) with skill load instead of `prompt_engineer` (read-only per inventory).
  - **Added 2.4 / 4.4 `code_mapper` pre-step** notes where root-cause analysis precedes implementation.
  - **Added Task 4.6** — F-003 ratification audit. The audit doc was not previously verified for thoroughness.
- **v1 (2026-05-04, Claude):** initial draft after five parallel sonnet investigators (F, G, H, I, integration) reported. Decisions D1–D8 surfaced; Phase 0 gates on Trey answering them. Codex idioms (subagent type names, slash skills, worktrees, narrow gates) followed per `docs/reference/codex-inventory-2026-05-03.md`.
