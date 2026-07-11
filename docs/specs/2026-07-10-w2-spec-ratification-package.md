# W2 spec ratification package — abstraction + cue substrate

Status: **RATIFIED AS-IS by Trey 2026-07-10** (r4; both deviations accepted — abstraction updated_at+sha256 tie-break, and companion W3 spec r3's journal torn-tail-resume — via in-session AskUserQuestion; recorded in BUILD-STATE approval record #8). (r4, 2026-07-10: Sol convergence re-read — entering-servable matrix row added to §A3; merge staging registered as a §A4 write entrypoint and generation context. Companion: W3 spec r3.) Plan: `docs/plans/2026-07-10-memora-lessons-memorum-upgrades.md` §W2 (r5, approved 2026-07-10, DP1/DP2 granted). On ratification the coordinator applies §A below as `docs/specs/stream-a-core-substrate-v1.2.md` (full versioned copy of v1.1 + these deltas), §B as a dated amendment to `docs/specs/stream-f-dreaming-v0.3.md`, §C as an additive amendment to `docs/api/memoryd-cli-contract-v1.md`, and §D as a cross-reference edit to `docs/specs/stream-e-ambient-recall-v4.0.md`. Stream E's own version bump (fusion contract) is deliberately deferred to the W4 spec task — nothing in W2 changes recall behavior.

## Coordinator deviation flag (ratify explicitly)

The plan (r5 §W2 task 1) says the `abstraction` merge rule is **"ours-wins."** Ours-wins is Git-side-dependent: clone A merging B keeps A's value, clone B merging A keeps B's — opposite merge directions diverge, violating two-clone convergence (repo invariant #6, spec §13.6.1). Three review rounds carried this; the plan's own convergence test requirement covers only `cues`. **Drafted instead:** true 3-way; same-field conflict selects the side with later `updated_at`; **equal timestamps break the tie by lexicographically greater `sha256(NFC(value))`** (the shipped `summary` rule falls through to ours on equality — a pre-existing convergence gap now logged in `docs/issues.md`; the new field does not inherit it). Loser preserved in `_merge_diagnostics`. It still loses at most one generation of abstraction (the dream-repair argument the plan used for ours-wins applies unchanged). Ratifying this package ratifies the substitution.

---

## §A — Stream A v1.1 → v1.2 delta (DP1: version bump granted)

**Revision goal (v1.2):** add optional `abstraction` and `cues` frontmatter fields with canonical serialization, validation, and merge semantics; add derived abstraction/cue embedding row kinds with full lifecycle; index schema 5→6 (additive); classification contract composes over the new fields. No behavior change for memories that lack the new fields.

### A1. Frontmatter (§6.2 additions)

Two new known nullable/collection fields, appended to the §6.2 table and to canonical serialization order (after `_merge_diagnostics`, before `_extras`):

| Field | Type |
| --- | --- |
| `abstraction` | string or null |
| `cues` | array of strings |

Defaults when absent on read: `abstraction: null`, `cues: []` (standard §6.2 permissive-parser materialization + `AutoPopulatedNullableField` warning). File `schema_version` stays `1`: both fields are optional, and pre-v1.2 parsers preserve them via `_extras` round-trip. (Known mixed-version wart, accepted for single-device dogfood: the **shipped** merge driver's `_extras` path resolves divergent values silently ours-wins (`field_rules.rs` `three_way_value` fallthrough) — which itself diverges from the written §14.4 quarantine wording, a pre-existing spec/code drift logged in `docs/issues.md`. Divergent cue edits across mixed-version devices are side-dependent until both devices run v1.2. Noted, not fixed.)

**Validation (§9 additions):**

- `abstraction`: ≤ 8 words (whitespace-split), ≤ 120 chars, single line, no control chars, NFC-normalized, trimmed, internal whitespace collapsed. Violation = hard error on write, `ValidationWarning` + field dropped to `null` on read of a hand-edited file (permissive read, canonical rewrite repairs).
- `cues`: 0–3 entries after normalization; each ≤ 6 words, ≤ 64 chars, single line, no control chars, NFC, trimmed, whitespace-collapsed; duplicates under case-fold are removed (first in side-independent total order kept). Same hard-on-write / repair-on-read posture.

**Merge driver (§14.4 additions; MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION unchanged — schema stays 1):**

| Field | Rule |
| --- | --- |
| `abstraction` | true 3-way; same-field conflict selects the side with later `updated_at`; equal timestamps → **any non-null beats null; null vs null is a no-op; both non-null → lexicographically greater `sha256(NFC(value))` wins** (null/empty-after-trim are equivalent and normalize to null before comparison); loser preserved in `_merge_diagnostics` (see deviation flag above) |
| `cues` | set union of both sides → NFC canonicalize → sort by the strict total order **`(case_fold(NFC(value)), NFC(value) bytes)`** where `case_fold` is **Unicode default (full) case folding — never locale-aware lowercasing** (`I`/`İ`, `ß` hazards) → dedup under case-fold equality keeping the first entry in that order (canonical casing = the byte-lexicographically smaller spelling; never insertion- or side-order) → keep first 3. Worked example: union `{OAuth, oauth}` → both fold to `oauth`, `O` < `o` byte-wise → keep `OAuth`; identical in both merge directions. Two-clone convergence fixtures required for opposite merge directions with overflowing unions **and with casing-only duplicates** |

### A2. Index schema 5→6 (DP2: this plan owns 6; ambient-recall v4 P2 re-points to 6→7)

Migration 6 is **additive-only**: `CREATE TABLE IF NOT EXISTS` with table-exists guards; no ALTER of existing tables; no data rewrite. Doctor gains cross-checks that (a) the four new tables exist iff schema ≥ 6, and (b) none of v4-P2's trigger-index tables exist (guard against the old double-claim). Runbook requirement: pre-migration DB file copy; rollback = restore copy (the migration writes nothing into schema-5 tables, so restore is clean).

New tables (mirroring the chunk-lane shapes):

All hashes below are canonical `sha256:<hex64>`: `abstraction_hash = sha256(NFC(abstraction))`, `cue_hash = sha256(NFC(cue_text))`, aux-job/meta `content_hash` = the hash of the text actually embedded (abstraction_hash or cue_hash). `source_body_hash` is the memory's body content hash **at abstraction mint time** — the **generation-freshness** signal. Freshness semantics are one-directional: a body-only edit **keeps** the mint-time `source_body_hash` (and the abstraction text/hash) so `abstraction_compile` can detect staleness; `source_body_hash` refreshes **only** when the abstraction itself is (re-)minted or explicitly written. Cue `target_id` encoding is pinned: `format!("{memory_id}:{ordinal}")`, ordinal `0..=2`, parsed from the rightmost `:`.

```sql
CREATE TABLE IF NOT EXISTS memory_abstractions (
  memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
  abstraction TEXT NOT NULL,
  abstraction_hash TEXT NOT NULL,
  source_body_hash TEXT NOT NULL
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
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  PRIMARY KEY (row_kind, target_id, provider, model_ref, dimension)
);
-- attempts/last_error mirror pending_embedding_jobs: one retry/backoff policy across both queues.
CREATE INDEX IF NOT EXISTS idx_aux_pending_jobs_enqueued ON aux_pending_embedding_jobs(enqueued_at);
```

Vector tables per (row kind × triple). **Vector-table identity is `(row_kind, provider, model_ref, dimension)` — the shipped `sqlite_vec::vector_table_name` helper hashes only the triple and MUST NOT be reused unmodified**: the row kind must enter the digest (or an equivalent disambiguator), else abstraction/cue vectors land in the chunk table. Names like `vec_abstractions__<provider>__<model>__<dim>` are illustrative only. A (row kind, triple) pair never shares a table with any other pair.

**Derived-data posture (Stream I):** `memory_abstractions`, `memory_cues`, aux meta/jobs, and all vector tables are derived from canonical frontmatter and are **rebuildable; they do not sync**. Reindex from files reconstructs them fully.

### A3. Embedding lifecycle for the new row kinds (§10.2.1/10.2.2 extensions)

Identity = `(row_kind, target_id, content_hash)` against the embedding triple — triple identity semantics unchanged (invariant #3; `DimensionMismatch` / `UnknownEmbeddingTriple` behave identically for aux rows).

- **Enqueue:** indexing a memory whose `retrieval_policy.index_embeddings == true` upserts `memory_abstractions`/`memory_cues` rows and enqueues aux jobs for the active triple when no matching `aux_embedding_meta` exists for the current hash. API-lane eligibility fence (`EmbeddingLaneEligibility`) applies to aux rows exactly as to chunks: plaintext-only lanes hold `confidential`/`personal` aux rows local, fail-closed.
- **Hash-change invalidation (atomic with the index write):** when an abstraction/cue text changes, the indexer — in the same SQLite transaction that updates `memory_abstractions`/`memory_cues` — deletes the stale `aux_embedding_meta` rows for the old hash and **replaces** (not or-ignores) the pending job with one for the new hash; the stale vector is deleted best-effort inline and guaranteed by reconciliation. Cue-set re-canonicalization that shifts or removes ordinals deletes the meta/job rows (and best-effort vectors) for every `(memory_id, ordinal)` whose `cue_hash` no longer matches. Query helpers and reconcile reject any vector whose meta `content_hash` differs from the current `abstraction_hash`/`cue_hash`.
- **Worker:** Stream B drains **all** row kinds from both queues (chunk + aux) — a drain pass is not complete while aux jobs remain. `update_embedding`-equivalent for aux rows validates target existence + `content_hash` match; mismatch = stale-fence rejection, no vector write.
- **Status-lifecycle matrix (per transition, covering rows / jobs / vectors / query visibility):**

  | Transition | `memory_abstractions`/`memory_cues` | aux jobs | aux vectors + meta | servable via aux query APIs |
  | --- | --- | --- | --- | --- |
  | edit — abstraction/cues change | upsert to new values (`source_body_hash` refreshed iff abstraction re-minted) | replaced per hash-change rule | stale deleted, fresh written on drain | yes (current hash only) |
  | edit — body only | abstraction/cue rows and `source_body_hash` **unchanged** (stale-by-design freshness signal; chunk lane handles the body) | aux jobs unchanged | aux vectors unchanged | yes |
  | supersede / archive / tombstone / quarantine (status leaves `{active, pinned}`) | rows deleted | jobs deleted | vectors + meta deleted (inline best-effort, reconcile-guaranteed) | no |
  | **enter servable set** (status enters `{active, pinned}` from outside it — W3 merge activation `candidate→active`, W3 rollback restore `superseded→{active,pinned}`) | upserted for current values | enqueued per the Enqueue rule (stale-hash fence applies) | written on drain | yes (current hash only) |
  | physical delete | CASCADE | deleted | reconcile-guaranteed | no |
  | reindex from files | rebuilt | re-enqueued as needed | reconciled | yes |

- **Reconcile (both directions, per §10.2.1):** orphan aux vectors deleted; meta-without-vector re-enqueued; jobs whose targets/hashes are gone dropped; vectors for non-`{active,pinned}` memories deleted.
- **Triple switch:** re-enqueues all row kinds for the new active triple; old aux vector tables remain queryable until `drop_embedding_model`, which drops all row kinds' tables for the triple.
- **Sensitivity upgrade revocation:** when a memory's `sensitivity` rises into `{confidential, personal}`: (a) **all API-lane vectors** for that memory — chunk, abstraction, cue — are deleted unconditionally; (b) the post-upgrade `retrieval_policy.index_embeddings` decides what remains — under the §6.2 default it flips to `false`, so local vectors/meta/jobs are **deleted too, with no re-enqueue** (the memory is metadata-retrievable only, per the existing contract); only an explicit operator override that keeps `index_embeddings=true` re-enqueues held-local jobs for a local lane. Lane switches (API→local, local→API) follow the existing triple/lane rules — a lane switch alone never resurrects vectors for ineligible sensitivities. Test required for both the default-delete and override-re-enqueue paths.
- **Doctor/status:** per-row-kind counts (indexed, pending, held-local) alongside chunk counts.
- **Query path (this is the whole point):** `query_abstraction_vectors` / `query_cue_vectors` on the Substrate index API — same triple addressing, same placeholder bucketing as chunk queries, KNN over the respective vector table returning `(memory_id [, ordinal], distance)`. **No recall-lane wiring in W2** — the APIs exist and are tested; W4 consumes them.

### A4. Classification contract composition (§8.7 extension)

**One shared pipeline for every write entrypoint** — `write`, `write-note`, supersede, import execute, `review approve`, `quarantine resolve --edited`, dream fragment→memory, `abstraction_compile`, backfill, merge staging (W3) — in this fixed order: **normalize/cap-validate → classify → drop/refuse/encrypt decision → validate the final payload → persist/index/enqueue.** No canonical file write, no index row, no aux row, and no embedding job may be created before the pipeline completes. The combined payload = title + summary + body + `abstraction` + every `cues` entry; strictest outcome controls the whole write. `secret` anywhere (including in a cue) refuses before any disk effect (invariant #1).

**Generation-context drop semantics (dual classification + outcome rebind):** in generation contexts (dream compile, backfill, W3 merge staging — which additionally floors the result at the strictest source classification, applied *after* the rebind), the classify step runs **twice** — once over the combined payload, once over the body-only payload (title + summary + body). If the combined outcome is stricter than the body-only outcome (the strictness caused *by* the generated fields, now well-defined), the pipeline drops `abstraction`/`cues` and **rebinds the write to the body-only `ClassificationOutcome`** (and its sensitivity) before persisting — without the rebind, the write would still carry `RequiresEncryption` and the plaintext path would refuse, losing the body. `secret` in either classification still refuses the whole write. Acceptance: a sensitive-cue-on-public-body dream compile commits the body with `abstraction: null`/`cues: []`, creates no aux rows or jobs, and the write's persisted outcome is the body-only one. Interactive writes do not drop — they refuse/error per the existing contract.

Entrypoint enumeration and the hardcoded-`Trusted` audit are W2 implementation deliverables (plan task 3); the contract here is the pipeline they must all route through — including the shipped substrate path (`api/write.rs`), which today classifies before frontmatter validation and must be reconciled to this order.

### A5. New/updated acceptance signals

Secret-in-cue refusal; sensitive-abstraction-on-public-body drops fields keeps body (nothing reaches disk/index/queue); upgrade-revocation deletes API-lane vectors across all row kinds and, under default policy, local ones too (both default-delete and override-re-enqueue paths); two-clone cue-merge convergence (opposite directions, overflowing unions, casing-only duplicates, identical result); abstraction conflict resolves by `updated_at` with sha256 tie-break and loser in diagnostics; migration 6 up + rollback on a copied live DB; reindex-from-files rebuilds all derived tables; aux stale-write fence + hash-change invalidation (no stale-hash vector ever servable); status-transition matrix fixtures (supersede/tombstone/quarantine clear aux state; enter-servable re-materializes rows + jobs and the memory becomes aux-retrievable again); triple-switch re-enqueue counts include aux kinds; doctor per-kind counts.

---

## §B — Stream F amendment: `abstraction_compile` dream job

Dated additive amendment to `stream-f-dreaming-v0.3.md` (no version bump: new optional job type, no change to existing pass behavior — flagging per convention; bump instead if Trey prefers).

- New dream job `abstraction_compile`: selects active/pinned memories lacking `abstraction` (or whose `abstraction_hash` predates current body hash per repair policy), mints `abstraction` (≤8 words) + `cues` (0–3, Memora `[Main Entity] + [Key Aspect]` guidance) via the **existing harness-CLI dream machinery** — no daemon-resident LLM.
- Output is untrusted input: machine-verified against §A1 caps/charset before use; malformed output = skip item, log, continue (the `malformed_pass_2_json` lesson).
- Application = **governed supersede** through the standard write path, carrying a fresh `ClassificationOutcome` per §A4 (drop-fields-keep-body on sensitive generation).
- Structural fallback when no harness CLI is available: `abstraction` = `summary` truncated to caps, no cues, marked `source: structural` in the job report.
- This job is the single generation mechanism for W4-prep (eval corpus), W5 (live backfill), and ongoing dream repair.

## §C — CLI contract v1: additive meta fields

`memoryd write` / `write-note` accept `abstraction` and `cues` via meta; protocol DTO + generated schema + envelope tests updated together; validation order at the trust boundary follows the §A4 pipeline. `skills/using-memorum/SKILL.md` gains cue-authoring guidance (adapted from Memora's `cue_index_generator.py` patterns: `[Main Entity] + [Key Aspect]`, 2–4 words, 0–3 cues) in the same change, with an acceptance signal that the skill documents abstraction/cue usage. Additive change, in-version per the contract's amendment convention.

## §D — ambient-recall v4.0 cross-reference edit

P2's index migration re-numbered **6→7**; add a pointer that schema 6 is owned by this arc (this package §A2). One-paragraph edit, no other v4 content touched.

---

## Ratification checklist for Trey

1. §A as Stream A v1.2 — including the **abstraction merge-rule substitution** (updated_at-newer-wins, not ours-wins).
2. §A2 table shapes (aux tables split from chunk tables; additive-only migration).
3. §B as an in-version dated amendment to Stream F v0.3 (or direct a v0.4 bump).
4. §C / §D as written.
