# Wedge-origin trace (F3 pre-work, recorded late)

**Date:** 2026-06-28
**Branch:** `foundation/runtime-loop-closure`
**Scope:** The read-only pre-work the plan required before Wave C / F3
(`docs/plans/2026-06-25-runtime-loop-foundation-implementation.md:111`,
spec `docs/specs/memorum-runtime-loop-foundation-v0.1.md` §8.1). It was never
recorded; Wave C's audit flagged the gap. This reconstructs it from the shipped
code and the live `~/memorum` store.

---

## 1. Origin verdict: IMPORT flow (governance-contradiction quarantine)

**Verdict: the 06-23 quarantine came from the IMPORT flow, not a crashed
reconcile / merge driver.** Spec §8.1's leading hypothesis (import) is
**confirmed**; the crashed-reconcile alternative is **refuted**.

### Live ground truth (`~/memorum`, read-only)

Two memories carry `status: quarantined` / `trust_level: quarantined`:

- `projects/Policy/decisions/mem_20260619_40edd13334a43d72_000536.md`
  (`feedback-pager-persuasion-craft`)
- `projects/Policy/decisions/mem_20260619_40edd13334a43d72_000432.md`
  (`pact-policy-paper-toolchain`)

Their frontmatter is the smoking gun:

| Field | Live value | Meaning |
| --- | --- | --- |
| `author.harness` / `source.harness` | `memoryd-import` | written by the import flow |
| `governance_reason` | `governance quarantine` | governance-decision quarantine |
| `_merge_diagnostics.human_reason` | `governance quarantine` | not a merge diagnostic |
| `review_state` | `quarantined` | governance value, **not** the merge-driver's `pending` |
| body | **no** `<<<<<<<` / `>>>>>>>` markers | no unresolved git merge |
| `created_at` == `updated_at` | `2026-06-19T18:2x` | written once, never re-touched by a merge |

Corroborating store state:

- `git -C ~/memorum reflog` shows **only** two `Initialize Stream A memory
  substrate` commits — **no `git merge` ever ran.**
- `git -C ~/memorum remote -v` — **no remote** (single-device, no-remote
  install, as §2 expects).
- No `.git/MERGE_HEAD` and no `startup-reconcile.required` marker present today.

### Code chain (matches the live frontmatter exactly)

1. Import runs the full governed-write pipeline and **explicitly handles a
   quarantine outcome** — `crates/memoryd/src/import/pipeline/execute.rs:190-200`
   (`GovernanceStatus::Quarantined => …`). Quarantine is an expected, handled
   import result, not an unhandled crash.
2. Contradiction detection decides quarantine —
   `crates/memory-governance/src/engine.rs:302-313`: a
   `ContradictionDecision::Contradiction` (non-supersede policy) or `Unclear`
   maps to `GovernanceWriteDecision::Quarantined { reason: "contradiction" |
   "contradiction_unclear" }`.
3. The pipeline lands it quarantined —
   `crates/memoryd/src/handlers/governance/pipeline.rs:612-617`:
   `GovernedLifecycle::new(MemoryStatus::Quarantined, TrustLevel::Quarantined, …)`.
4. `to_memory` stamps the exact strings seen on disk —
   `crates/memoryd/src/handlers/governance/meta.rs:463-464`
   (`governance_reason = "governance quarantine"`),
   `:528-535` (`_merge_diagnostics.human_reason = "governance quarantine"`),
   `:457-459` (`review_state = "quarantined"`), and the Import provenance block
   `:437-444` (`harness = "memoryd-import"`).

### Why the merge-driver / crashed-reconcile path is refuted

- The merge driver is the **only** writer that quarantines on a `git merge`
  conflict (`crates/memory-substrate/src/merge/three_way.rs:221-225`), and it
  leaves a distinct signature the live files **lack**: `review_state = "pending"`
  (not `"quarantined"`), a body conflict marker
  (`<!-- merge quarantine … -->` plus git `<<<<<<<`/`>>>>>>>`), and
  merge-specific `_merge_diagnostics` (`unparsed_sides`, `conflicting_fields`).
- Reconcile **never writes** quarantine — it only *reads* it. `reconcile.rs`
  phase 6 (`reindex_and_scan_conflicts`, `:461-534`) parses frontmatter and
  *collects* already-quarantined paths; a crash mid-reconcile cannot mint a
  `status: quarantined` file.
- On a no-remote install `fetch_and_merge` is the only `git merge` caller
  (`crates/memory-substrate/src/git/sync.rs:101-103`); with no remote it never
  reaches the merge, and the empty reflog confirms it never ran.

The stranded `MERGE_HEAD` mentioned in §8.1 is a **separate** `recovery_required`
signal (`reconcile.rs:171-177`), not produced by import or reconcile; it is
absent now and irrelevant to the `blocking_conflicts` wedge that drove the
96-duplicate notification symptom.

---

## 2. Distinct-path count: **2**

`blocking_conflicts` is the set of repo paths whose frontmatter is quarantined,
collected in `reindex_and_scan_conflicts`
(`crates/memory-substrate/src/runtime/reconcile.rs:505-506` predicate,
`:519-520` push, sorted+deduped `:531-532`). The live store yields exactly **two**
distinct paths (both under `projects/Policy/decisions/`). This is the count that
should have sized the F3 dedup design: **N = 2 distinct paths**, not N = 1
re-emitted path.

---

## 3. Correctness-bug decision: **NO** (with one design note)

**There is no import/reconcile *correctness* bug that re-wedges on the next
import.** The wedge was an operational artifact of *missing recovery/observability
machinery* (exactly what F3/F4 add), layered on a legitimate governance outcome —
not a logic defect in import or reconcile.

- The import flow worked **as designed**: contradiction detection judged two
  semantically-adjacent Policy memories contradictory/unclear and quarantined
  them for human review (engine.rs:302-313). That is the intended governance
  routing, handled explicitly by import (execute.rs:190-200) — distinct from the
  §3.2 *declined-enrichment* concern (that is about not batch-enriching frozen
  imports; this is about governance provenance).
- Reconcile worked **as designed**: it surfaced the existing quarantines; it did
  not create them.
- The *pathological* wedge — 96 duplicate notifications, an un-resolvable
  quarantine, a blind doctor — was the absence of (a) dedup, (b) a re-reconcile
  trigger to clear, and (c) doctor visibility. Wave C (F3) and Wave D (F4) supply
  all three. In the post-F3/F4 world a future import quarantine is **self-handling**:
  it appends once (per-path dedup key), names its path, is resolvable via
  `quarantine resolve <id>` (no conflict markers ⇒ the
  `has_git_conflict_markers` refusal at `quarantine.rs:25,109-111` does not fire),
  and the rescan clears D2 within the daemon lifetime. So it does **not** recur as
  a wedge.

**Conclusion: Wave C does NOT need to grow to fix an import/reconcile bug.** F3 is
contractually complete for this origin.

### Design note (judgment call, not a blocker)

`reindex_and_scan_conflicts` treats **any** `status/trust == Quarantined` memory
as a `blocking_conflict` (`reconcile.rs:505-506,519-520`), and the field is
documented as paths "whose **merge** was quarantined" (`reconcile.rs:107-109`).
That conflates two provenances:

- **merge-driver** quarantine — an unresolved `git merge` (`review_state:
  pending`, body conflict markers) — a genuine sync-blocking state, and
- **governance-contradiction** quarantine from import (`review_state:
  quarantined`, `governance_reason: governance quarantine`, no markers) — a
  routine review-queue item.

Both render as "Sync is blocked by a merge conflict" and both trip F4/D2 as
**fatal** ("loop broken"). The spec's own F4/D2 design deliberately makes any
quarantine fatal-visible, so this is defensible as-specified — but it means every
future import that produces a contradiction quarantine will paint D2 red even
though nothing is wrong with sync. If that proves too aggressive in dogfood, the
clean fix is to discriminate provenance in the phase-6 predicate (only count a
path as a `blocking_conflict` when it carries unresolved git markers /
merge-driver `_merge_diagnostics`, and route governance quarantines through the
review/quarantine queue instead). Recommended as **optional follow-up**, not a
Wave C blocker. The discriminator already exists in the data: `review_state`
(`pending` = merge, `quarantined` = governance) and the conflict-marker test.

---

## 4. Is F3's shipped dedup design correct for path count 2? **YES**

The plan's correctness bar: for N distinct paths, either aggregate to one "N
conflicts" notice **or** include the path in `passive_message` so distinct paths
render distinctly (a per-path key alone, with a flattened identical message,
would pass `restart_does_not_duplicate_*` on a technicality while N identical
lines still pile up).

The shipped code meets the bar via the second option:

- Per-path dedup key —
  `crates/memoryd/src/notifications/dispatcher.rs:73-79`
  (`blocking_merge_conflict:<path>`); `append_with_key` skips a duplicate key, so
  a restart does not multiply entries (I-F3.1).
- The path **is interpolated into the user-visible message** —
  `dispatcher.rs:53-55`: `"Sync is blocked by a merge conflict in {path}."` So
  the two live paths render as **two distinct lines**, not two identical ones.
- Clearing is path-keyed and provenance-correct —
  `crates/memoryd/src/handlers/quarantine.rs:113-124`
  (`prune_resolved_blocking_notifications`) recomputes the live quarantine set
  and clears keys no longer present, satisfying I-F3.3.

For N = 2 the per-path-key + path-in-message design is correct: distinct keys,
distinct messages, restart-stable, and self-clearing on resolve.

---

## Summary

- **Origin:** import flow, governance-contradiction quarantine (engine.rs:302-313
  → pipeline.rs:612-617 → meta.rs:463-535), confirmed against live frontmatter
  (`harness: memoryd-import`, `governance_reason: governance quarantine`,
  `review_state: quarantined`, no conflict markers) and an empty/merge-free
  reflog. Crashed-reconcile / merge-driver refuted.
- **Distinct paths:** 2.
- **Import/reconcile correctness bug:** NO. The wedge was missing F3/F4 machinery
  over a legitimate governance quarantine, not a defect that mints bad state;
  post-F3/F4 a future import quarantine self-handles and does not re-wedge. One
  optional design note: phase 6 conflates merge vs. governance quarantines into
  `blocking_conflicts` (defensible per the F4/D2 spec; flag for dogfood).
- **F3 dedup design:** correct for path count 2 — per-path key + path in
  `passive_message` renders distinct paths distinctly and is restart-stable.
