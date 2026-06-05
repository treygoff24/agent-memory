# Elegance Audit — 2026-06-05

Judge-panel elegance audit (3 lenses, RUBRIC v2, Rust + TypeScript) run as Phase 6 of
the codebase-excellence campaign, against tree state `b6f78d4`.

## Result

- **Hard-flag gate: CLEAR. No blockers.** All four trust-breaking dimensions score well
  above the `<= 2` threshold: Contracts & Domain Modeling 4.0, Error & Invariant
  Integrity 5.0, Test & Behavioral Honesty 5.0, Dependency & Build Hygiene 5.0.
- **Overall (provisional, uncalibrated): 4.48 / 5 — A−, "Excellent."** Trust this for
  direction only; the hard-flag gate and the backlog are the trusted outputs.

| Dimension | Mean | Weight |
| --- | --- | --- |
| Naming & Readability | 5.00 | 9 |
| Error & Invariant Integrity | 5.00 | 9 |
| Test & Behavioral Honesty | 5.00 | 12 |
| Dependency & Build Hygiene | 5.00 | 6 |
| Module Depth & Information Hiding | 4.33 | 13 |
| Dependency Shape & Coupling | 4.33 | 11 |
| Right-Sized Abstraction | 4.33 | 11 |
| Complexity Locality & Special-Case Handling | 4.00 | 9 |
| Contracts & Domain Modeling | 4.00 | 12 |
| Consistency & Idiomatic Coherence | 4.00 | 8 |

All three lenses independently read the codebase as co-authored and disciplined: layered
acyclic dependency DAG, thiserror-per-crate error taxonomy, poison-recovering locks, ~40%
test LOC with zero `#[ignore]`/`should_panic`, real multi-phase CI gate. The gap to 5.0 is
residue, not rot — every sub-5 dimension has an identified, bounded refactor below.

## Backlog (deduped across lenses)

### Implemented in this campaign (Phase 7)

- **M1 (partial)** — `VectorError::Storage(String)` is genuinely a `serde_json` serialization
  failure (not a SQLite error), so it became a typed `Serialize(#[from] serde_json::Error)`
  variant rather than collapsing into `Sqlite`; the call site now uses `?`. `MergeError::Parse`
  turned out to be a *live* variant (the all-sides-unparseable carrier-selection failure, which
  the single-side `ParseSide` cannot model), so it was kept and its misleading "deferred /
  switch to ParseSide" doc corrected. The audit's "convert to existing typed replacements"
  premise only held for the vector half — a coordinator-review correction.
- **D6** — convert `memorum-coordination`'s hand-rolled `PeerHeartbeatError` and
  `Result<(), String>` config errors to thiserror / a typed `ConfigValidationError` (delegate
  lane, isolated worktree, integrated by cherry-pick after review).

### Deferred — design nuance found on inspection; better with a focused session or Trey's eyes

- **D3 — type `RepoPath::try_new`'s `Result<_, String>`.** Safe and compiler-enforced, but the
  return-type change ripples across three crates (substrate `api.rs`/`tree`/`ids`, the
  coordination bench, and memoryd `trust_artifact`/`dream/*`), each with its own `map_err`
  boundary (`ValidationError::Other`, `CleanupError::Serialization`, …). A clean, bounded
  follow-up — deferred only to avoid a broad cross-crate sweep in an unattended pass.
- **D2 — 3-way merge `ThreeWaySides` context struct.** Highest-value design item (2/3
  corroborated), behavior-preserving, but it edits the merge driver — invariants 5–6 (merge
  schema gate, two-clone convergence). Worth doing with full attention + a deliberate
  convergence-test validation, not a 5am autonomous pass.
- **M2 — collapse `write_memory`'s 5× `guard_with_refusal_audit` boilerplate.** Sensitive to
  the spec §8.7 step-6 `WriteRefused` audit-event ordering; a closure extraction must not
  reorder or drop a gate. Bounded but ordering-critical — left for a focused pass.

### Deferred — need Trey's call

- **D1 — unify the three parallel `SourceKind`/`MemoryId` models** (substrate validated
  newtype vs governance `type MemoryId = String` + its own enums vs the daemon's
  `GovernanceSourceKindMeta`). Highest *insight* item — it explains the only real score
  divergence (Contracts: 3/4/5). **Why deferred:** the first step (governance `MemoryId =
  String` → substrate's validated newtype) requires adding a `memory-governance →
  memory-substrate` dependency edge. `memory-governance` deliberately has no such edge today
  — the String-typed `MemoryId` and 4-variant `SourceKind` may be an intentional
  bounded-context boundary, not an oversight. Unifying it is an architecture decision (and a
  DAG-acyclicity risk) that wants your judgment, not a 4am autonomous call.

- **D4 — replace `WriteOutcome`'s `committed`/`indexed`/`event_recorded` bool triple with a
  phase enum.** `WriteOutcome` is a serialized public DTO consumed by the daemon + dashboards,
  so flipping the wire shape likely needs a **spec version bump** — out of bounds for
  autonomous work per CLAUDE.md.

- **D5 — split the 2437-line `api.rs` into read/write/reconcile submodules.** Low impact
  (it already carries `#![allow(file_too_long)]` as a conscious deferral), pure mechanical
  move, but high churn-conflict risk against in-flight Stream A worktrees. Best done when the
  branch landscape is quiet, ideally via `refactor-pilot`.
