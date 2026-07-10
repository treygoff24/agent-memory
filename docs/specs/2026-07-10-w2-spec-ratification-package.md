# W2 spec ratification package — abstraction + cue substrate

Status: **DRAFT — awaiting Trey ratification.** Plan: `docs/plans/2026-07-10-memora-lessons-memorum-upgrades.md` §W2 (r5, approved 2026-07-10, DP1/DP2 granted). On ratification the coordinator applies §A below as `docs/specs/stream-a-core-substrate-v1.2.md` (full versioned copy of v1.1 + these deltas), §B as a dated amendment to `docs/specs/stream-f-dreaming-v0.3.md`, §C as an additive amendment to `docs/api/memoryd-cli-contract-v1.md`, and §D as a cross-reference edit to `docs/specs/stream-e-ambient-recall-v4.0.md`. Stream E's own version bump (fusion contract) is deliberately deferred to the W4 spec task — nothing in W2 changes recall behavior.

## Coordinator deviation flag (ratify explicitly)

The plan (r5 §W2 task 1) says the `abstraction` merge rule is **"ours-wins."** Ours-wins is Git-side-dependent: clone A merging B keeps A's value, clone B merging A keeps B's — opposite merge directions diverge, violating two-clone convergence (repo invariant #6, spec §13.6.1). Three review rounds carried this; the plan's own convergence test requirement covers only `cues`. **Drafted instead:** true 3-way; same-field conflict selects the side with later `updated_at`, loser preserved in `_merge_diagnostics` — identical to the shipped `summary` rule, side-independent, and it still loses at most one generation of abstraction (the dream-repair argument the plan used for ours-wins applies unchanged). Ratifying this package ratifies the substitution.

---

## §A — Stream A v1.1 → v1.2 delta (DP1: version bump granted)

**Revision goal (v1.2):** add optional `abstraction` and `cues` frontmatter fields with canonical serialization, validation, and merge semantics; add derived abstraction/cue embedding row kinds with full lifecycle; index schema 5→6 (additive); classification contract composes over the new fields. No behavior change for memories that lack the new fields.

### A1. Frontmatter (§6.2 additions)

Two new known nullable/collection fields, appended to the §6.2 table and to canonical serialization order (after `_merge_diagnostics`, before `_extras`):

| Field | Type |
| --- | --- |
| `abstraction` | string or null |
| `cues` | array of strings |

Defaults when absent on read: `abstraction: null`, `cues: []` (standard §6.2 permissive-parser materialization + `AutoPopulatedNullableField` warning). File `schema_version` stays `1`: both fields are optional, and pre-v1.2 parsers preserve them via `_extras` round-trip. (Known mixed-version wart, accepted for single-device dogfood: the pre-v1.2 merge driver's `_extras` add/add rule can quarantine divergent cue edits across devices instead of set-merging them. Noted, not fixed.)

**Validation (§9 additions):**

- `abstraction`: ≤ 8 words (whitespace-split), ≤ 120 chars, single line, no control chars, NFC-normalized, trimmed, internal whitespace collapsed. Violation = hard error on write, `ValidationWarning` + field dropped to `null` on read of a hand-edited file (permissive read, canonical rewrite repairs).
- `cues`: 0–3 entries after normalization; each ≤ 6 words, ≤ 64 chars, single line, no control chars, NFC, trimmed, whitespace-collapsed; duplicates under case-fold are removed (first in side-independent total order kept). Same hard-on-write / repair-on-read posture.

**Merge driver (§14.4 additions; MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION unchanged — schema stays 1):**

| Field | Rule |
| --- | --- |
| `abstraction` | true 3-way; same-field conflict selects the side with later `updated_at`, loser preserved in `_merge_diagnostics` (see deviation flag above) |
| `cues` | set union of both sides → NFC canonicalize → case-fold dedup (keep first occurrence in the total order) → **side-independent total order** (lexicographic byte order of the case-folded NFC form) → keep first 3. No ours/theirs priority anywhere. Two-clone convergence fixtures required for opposite merge directions with overflowing unions |

### A2. Index schema 5→6 (DP2: this plan owns 6; ambient-recall v4 P2 re-points to 6→7)

Migration 6 is **additive-only**: `CREATE TABLE IF NOT EXISTS` with table-exists guards; no ALTER of existing tables; no data rewrite. Doctor gains cross-checks that (a) the four new tables exist iff schema ≥ 6, and (b) none of v4-P2's trigger-index tables exist (guard against the old double-claim). Runbook requirement: pre-migration DB file copy; rollback = restore copy (the migration writes nothing into schema-5 tables, so restore is clean).

New tables (mirroring the chunk-lane shapes):

```sql
CREATE TABLE IF NOT EXISTS memory_abstractions (
  memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
  abstraction TEXT NOT NULL,
  abstraction_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_cues (
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,          -- 0..2, position in canonical cue order
  cue_text TEXT NOT NULL,
  cue_hash TEXT NOT NULL,
  PRIMARY KEY (memory_id, ordinal)
);

CREATE TABLE IF NOT EXISTS aux_embedding_meta (
  row_kind TEXT NOT NULL CHECK (row_kind IN ('abstraction','cue')),
  target_id TEXT NOT NULL,           -- memory_id, or memory_id || ':' || ordinal for cues
  content_hash TEXT NOT NULL,
  provider TEXT NOT NULL,
  model_ref TEXT NOT NULL,
  dimension INTEGER NOT NULL,
  embedded_at TEXT NOT NULL,
  PRIMARY KEY (row_kind, target_id, provider, model_ref, dimension)
);

CREATE TABLE IF NOT EXISTS aux_pending_embedding_jobs (
  row_kind TEXT NOT NULL CHECK (row_kind IN ('abstraction','cue')),
  target_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  provider TEXT NOT NULL,
  model_ref TEXT NOT NULL,
  dimension INTEGER NOT NULL,
  enqueued_at TEXT NOT NULL,
  PRIMARY KEY (row_kind, target_id, provider, model_ref, dimension)
);
CREATE INDEX IF NOT EXISTS idx_aux_pending_jobs_enqueued ON aux_pending_embedding_jobs(enqueued_at);
```

Vector tables per (row kind × triple), same adapter naming discipline as §10.2.2: `vec_abstractions__<provider>__<model>__<dim>`, `vec_cues__<provider>__<model>__<dim>`. A triple never shares a table across row kinds or with chunks.

**Derived-data posture (Stream I):** `memory_abstractions`, `memory_cues`, aux meta/jobs, and all vector tables are derived from canonical frontmatter and are **rebuildable; they do not sync**. Reindex from files reconstructs them fully.

### A3. Embedding lifecycle for the new row kinds (§10.2.1/10.2.2 extensions)

Identity = `(row_kind, target_id, content_hash)` against the embedding triple — triple identity semantics unchanged (invariant #3; `DimensionMismatch` / `UnknownEmbeddingTriple` behave identically for aux rows).

- **Enqueue:** indexing a memory whose `retrieval_policy.index_embeddings == true` upserts `memory_abstractions`/`memory_cues` rows and enqueues aux jobs for the active triple when no matching `aux_embedding_meta` exists for the current hash. API-lane eligibility fence (`EmbeddingLaneEligibility`) applies to aux rows exactly as to chunks: plaintext-only lanes hold `confidential`/`personal` aux rows local, fail-closed.
- **Worker:** Stream B drains **all** row kinds from both queues (chunk + aux) — a drain pass is not complete while aux jobs remain. `update_embedding`-equivalent for aux rows validates target existence + `content_hash` match; mismatch = stale-fence rejection, no vector write.
- **Delete:** memory delete/tombstone cascades aux rows; vector deletion best-effort inline, guaranteed by reconciliation.
- **Reconcile (both directions, per §10.2.1):** orphan aux vectors deleted; meta-without-vector re-enqueued; jobs whose targets/hashes are gone dropped.
- **Triple switch:** re-enqueues all row kinds for the new active triple; old aux vector tables remain queryable until `drop_embedding_model`, which drops all row kinds' tables for the triple.
- **Sensitivity upgrade revocation:** when a memory's `sensitivity` rises into `{confidential, personal}` while an API-transit lane is active, all API-lane vectors for that memory — chunk, abstraction, cue — are deleted and the rows re-enqueued held-local. Test required.
- **Doctor/status:** per-row-kind counts (indexed, pending, held-local) alongside chunk counts.
- **Query path (this is the whole point):** `query_abstraction_vectors` / `query_cue_vectors` on the Substrate index API — same triple addressing, same placeholder bucketing as chunk queries, KNN over the respective vector table returning `(memory_id [, ordinal], distance)`. **No recall-lane wiring in W2** — the APIs exist and are tested; W4 consumes them.

### A4. Classification contract composition (§8.7 extension)

Every write's `ClassificationOutcome` is computed over the **combined payload**: title + summary + body + `abstraction` + every `cues` entry. Strictest outcome controls the whole write. `secret` anywhere (including in a cue) refuses before any disk effect (invariant #1). A `RequiresEncryption`/sensitive outcome caused **solely** by generated abstraction/cues in a generation context (dream compile, backfill) resolves fail-closed as **drop abstraction/cues, keep body** — the memory persists without the new fields rather than escalating or refusing (generation-context behavior; interactive writes still refuse/error per the existing contract). Entrypoint enumeration and the hardcoded-`Trusted` audit (`review approve`, `quarantine resolve --edited`, dream fragment→memory, import execute, backfill) are W2 implementation deliverables (plan task 3); the contract here is the rule they must all satisfy.

### A5. New/updated acceptance signals

Secret-in-cue refusal; sensitive-abstraction-on-public-body drops fields keeps body; upgrade-revocation deletes API-lane vectors across all row kinds; two-clone cue-merge convergence (opposite directions, overflowing unions, identical result); abstraction conflict resolves by `updated_at` with loser in diagnostics; migration 6 up + rollback on a copied live DB; reindex-from-files rebuilds all derived tables; aux stale-write fence; triple-switch re-enqueue counts include aux kinds; doctor per-kind counts.

---

## §B — Stream F amendment: `abstraction_compile` dream job

Dated additive amendment to `stream-f-dreaming-v0.3.md` (no version bump: new optional job type, no change to existing pass behavior — flagging per convention; bump instead if Trey prefers).

- New dream job `abstraction_compile`: selects active/pinned memories lacking `abstraction` (or whose `abstraction_hash` predates current body hash per repair policy), mints `abstraction` (≤8 words) + `cues` (0–3, Memora `[Main Entity] + [Key Aspect]` guidance) via the **existing harness-CLI dream machinery** — no daemon-resident LLM.
- Output is untrusted input: machine-verified against §A1 caps/charset before use; malformed output = skip item, log, continue (the `malformed_pass_2_json` lesson).
- Application = **governed supersede** through the standard write path, carrying a fresh `ClassificationOutcome` per §A4 (drop-fields-keep-body on sensitive generation).
- Structural fallback when no harness CLI is available: `abstraction` = `summary` truncated to caps, no cues, marked `source: structural` in the job report.
- This job is the single generation mechanism for W4-prep (eval corpus), W5 (live backfill), and ongoing dream repair.

## §C — CLI contract v1: additive meta fields

`memoryd write` / `write-note` accept `abstraction` and `cues` via meta; protocol DTO + generated schema + envelope tests updated together; validation order at the trust boundary: length/charset/count caps first, then classification per §A4. Additive change, in-version per the contract's amendment convention.

## §D — ambient-recall v4.0 cross-reference edit

P2's index migration re-numbered **6→7**; add a pointer that schema 6 is owned by this arc (this package §A2). One-paragraph edit, no other v4 content touched.

---

## Ratification checklist for Trey

1. §A as Stream A v1.2 — including the **abstraction merge-rule substitution** (updated_at-newer-wins, not ours-wins).
2. §A2 table shapes (aux tables split from chunk tables; additive-only migration).
3. §B as an in-version dated amendment to Stream F v0.3 (or direct a v0.4 bump).
4. §C / §D as written.
