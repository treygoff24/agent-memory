# Alpha Core Gap Closeout — audit and acceptance sweep

**Plan:** `docs/plans/2026-05-25-alpha-core-gap-closeout.md`
**Branch:** `main`
**HEAD commit at audit:** `d07641a`
**Author:** Claude (audit + closeout) via four parallel sonnet subagents
**Closeout date:** 2026-05-26

This artifact closes out the alpha-core-gap-closeout plan. The plan was authored in commit `d07641a` ("Prepare alpha dogfood readiness") and described 10 tasks closing 7 alpha-readiness gaps. No execution log was written, no task tracker was kept, and the plan's Final alpha acceptance checklist was committed with every item unchecked. That made the surface state of the plan misleading — substantial implementation had already landed in `d07641a` and the surrounding alpha-prep commits (`4f03876`, `a7371d7`, `1969a87`), but reading the plan alone would suggest nothing had been done.

This sweep grounded every acceptance signal in the plan against actual file:line evidence in the repo via four parallel sonnet audit subagents (Tasks 1+2+3, Tasks 4+7, Tasks 5+6, Tasks 8+9+10). Read-only audit — no gates were run as part of this sweep; that's the residual item below.

---

## Per-task verdicts

| # | Task | Verdict | Notes |
| --- | --- | --- | --- |
| 1 | Protocol and contract scaffolding | **Done** | Rust protocol DTOs complete; two TS-side gaps below. |
| 2 | Real daemon-backed ROI metrics | **Done** | `/api/roi` returns live substrate-derived metrics, no 501. |
| 3 | Daemon-backed notifications stream | **Done** | SSE path forwards `NotificationsRecent` with stable id + composite dedupe. |
| 4 | Replace synthetic dashboard/TUI status and detail fields | **Partial** | Real frontend gaps remain on `Entities.tsx` and `Peers.tsx`; architectural deviation on the daemon-side dashboard modules. |
| 5 | Broaden source grounding at daemon/MCP/CLI boundary | **Done** | `local_artifact` shipped through CLI + MCP; rich modes return typed unsupported with actionable copy. |
| 6 | Make visible web controls functional or explicitly disabled | **Done** | Top-bar search, recall CSV export, peers pair button, inspector approve/reject/forget all wired or aria-disabled with copy. |
| 7 | Daemon-backed, visible policy editor writes | **Done** | Validate/write through daemon with atomic write + event-log audit; Settings tab mounts with `writable`-gated save. |
| 8 | Make source and dashboard docs truthful | **Done** | No stale "deferred" language; alpha unsupported scope (browser-rendered, pairing, model privacy filter) called out explicitly. |
| 9 | Eval/release semantic coverage and CI hardening | **Done** | T17/T18 sentinels removed from test bodies; `--required-release-set alpha` flag real; `memoryd device rotate-keys` shipped. |
| 10 | Integrated gate, live dogfood smoke, and cleanup | **Partial** | `scripts/check-dogfood.sh` covers all five smoke categories; only gap was the unchecked plan checklist itself (closed by this sweep). |

**8 of 10 Done, 2 Partial.**

---

## Specific evidence highlights

### Task 1 — Rust DTOs complete; two TS-side gaps

All four `RequestPayload`/`ResponsePayload` pairs are present in `crates/memoryd/src/protocol.rs` (lines 65–75, 289–292). `SourceCapturePayload` and `CaptureSourceMode` enum (HttpStatic | LocalArtifact | PdfText | BrowserRendered | Screenshot | Authenticated | Unsupported) are defined at lines 323/336. Protocol-level rejection of `key_path`, `raw_key`, `key_material`, `allow_private_network`, `privacy_bypass` is tested in `crates/memoryd/tests/source_capture_contract.rs:160-174` via `RequestEnvelope::from_json_line`.

Frontend gaps:
- `DashboardRoiResponse` is not exported from `crates/memoryd-web/frontend/src/api/types.ts`; the frontend uses the pre-existing `RoiResponse` alias instead. Behaviorally fine, but the type name diverges from the plan's contract.
- `CaptureSourceMode` and `SourceCapturePayload` have no TypeScript counterparts in `types.ts` or `mutations.ts`. Not currently consumed by any frontend code (source capture is CLI/MCP only for alpha), but the plan called for the type surface to exist.

### Task 4 — Real gaps in synthetic-field removal

`/api/status`, `/api/sync-dashboard`, and the TUI panels correctly derive from daemon state and label unknown fields rather than inventing values. `DaemonSnapshot::sample()` usage is confined to test files; no production path falls back to it.

Real gaps:
- `crates/memoryd-web/frontend/src/views/entitiesView/EntityTable.tsx` (referenced as `Entities.tsx` in the plan): `normalizeKind` keyword-classifies entity kind by string-matching `node.kind + node.label` against literals including `'acme'`, `'pnpm'`, `'rust'`, `'home'`, `'office'`, with a catch-all default to `'project'`. This is exactly the heuristic the plan invariant flagged — daemon mode inventing a categorical field rather than showing "unknown."
- `crates/memoryd-web/frontend/src/views/Peers.tsx`: `normalizePeer` hardcodes `eventsIn24h: 0`, `eventsOut24h: 0`, `locksPending: 0`, `devicePubkeyShort: 'unknown'` because the daemon `PeerSessionStatus` doesn't supply those counters. Displaying `0` instead of `null`/"unknown" reads as a daemon fact and is the same class of issue.
- `EntityDetailResponse::fixture()` in `crates/memoryd-web/src/routes/entity_graph.rs:104-131` contains fabricated `first_seen: Some("2026-05-01T11:02:00Z")`, `confidence: Some(0.95)`, and fake memory IDs. This is only reached when `state.dashboard_data().is_some()` (explicit fixture mode), not as a fallback for failed live fetches, so the invariant "daemon mode never invents" is preserved at the route level. Leave or refactor based on whether fixture mode is a long-term operator demo or a short-term scaffold.

Architectural deviation (non-blocking): the plan called for `crates/memoryd/src/dashboard/status.rs` and `crates/memoryd/src/dashboard/entities.rs` as daemon-side modules. Neither exists. `dashboard/mod.rs` declares only `pub mod roi;`. The route logic lives directly in `crates/memoryd-web/src/routes/{status,entity_graph,sync_dashboard}.rs` instead. Behavior is correct; module layout diverges from the plan's recommendation.

### Task 7 — minor inconsistency

`crates/memoryd-web/src/routes/policy_editor.rs` contains a `FIXTURE_POLICY_YAML` constant (a 9-line inline string) returned when `state.policy_dir()` is `None` and `state.dashboard_data().is_some()`. Inconsistent with the daemon's builtin policy set shape; reachable only in fixture mode. Non-blocking.

### Task 10 — gate script complete, only the checklist lagged

`scripts/check-dogfood.sh` (221 lines) covers all five required smoke categories:
- `/api/roi` daemon smoke (lines 152–155): asserts `window_days:90`, rejects deferred/placeholder language.
- `/api/notifications/stream` SSE heartbeat (lines 158–168): asserts `event: heartbeat`.
- Source capture local artifact (lines 170–184): asserts `webcap:` source ref and `local_artifact` capture method.
- Policy GET writable + validate + write in temp repo (lines 186–210): asserts `"writable":true` and `"accepted":true`.
- Eval `--required-release-set alpha` dry run (lines 212–215): asserts no `"deferred": true` and no `feature_deferred`.

Only gap on Task 10 was the unchecked Final alpha acceptance checklist, which this sweep closes.

---

## Final alpha acceptance checklist — closeout state

Mapping from the plan's checklist to evidence:

- [x] `/api/roi` returns daemon data, not `501`. — Task 2.
- [x] `/api/notifications/stream` returns daemon passive notifications. — Task 3.
- [x] No visible dashboard button silently does nothing. — Task 6.
- [ ] **Dashboard status, peers, entities, and TUI panels never invent daemon facts.** — Task 4 partial. Status, sync-dashboard, TUI: correct. Entities `normalizeKind` and Peers hardcoded counters still invent.
- [x] Source capture supports alpha modes through CLI, daemon, and MCP; unsupported rich modes fail clearly. — Task 5.
- [x] Policy editor GET shows daemon policy files/YAML as writable when allowed, and POST validates plus atomically writes daemon repo policies. — Task 7.
- [x] Eval catalog has no required alpha deferred tests; RC/release CI fails on missing required semantic coverage. — Task 9.
- [x] Privacy remains deterministic-first by explicit alpha decision. — Explicit scope exclusion; no implementation work required.
- [ ] **`bash scripts/check-dogfood.sh`, workspace clippy, workspace tests, frontend typecheck/lint/Vitest all pass.** — Not exercised by this structural audit. Gate script exists and covers the right surfaces; whether it currently runs green is a separate verification step.

---

## Recommended follow-ups

1. **Close Task 4 partials.** Two small frontend fixes plus an optional refactor:
   - `EntityTable.tsx normalizeKind`: drop the keyword heuristic; default unrecognized kinds to `'unknown'` and let the table render the literal value the daemon supplied.
   - `Peers.tsx normalizePeer`: render `null`/"unknown" for counter fields the daemon doesn't supply, rather than `0`.
   - Optional: extract `EntityDetailResponse::fixture()` into a test-only module and stop importing it from production fixture mode, or move fixture mode behind a build flag.

2. **Close Task 1 TS-side gaps.** Either rename `RoiResponse` → `DashboardRoiResponse` (preferred — matches protocol naming end-to-end), or amend the plan to document the deliberate divergence. Add `CaptureSourceMode` and `SourceCapturePayload` to `types.ts` if any frontend will ever consume source capture (currently CLI/MCP only).

3. **Run the gate.** `bash scripts/check-dogfood.sh` + the workspace gates listed in the checklist's item 9 should be exercised to confirm green before claiming alpha-ready. The audit was structural; runtime confirmation is the missing piece.

4. **Live dogfood smoke per Task 10 Step 3.** Run the full `install-memorum.sh` + `memoryd serve` + MCP/web/TUI smoke loop on a clean machine. The plan's Step 3 is the explicit dogfood gate; structural audit doesn't substitute.

Nothing here blocks alpha-readiness *substantively*. The remaining frontend cleanups are small and the gate confirmation is a routine run.
