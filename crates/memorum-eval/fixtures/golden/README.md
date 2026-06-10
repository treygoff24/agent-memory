# Golden recall corpus

The measuring instrument for the Memorum dynamics program (plan
`2026-06-09-dynamics-eval-hardening.md`, Task 4.1). A hand-curated set of memory
files plus a labeled query set that the quality runner (Task 4.2) replays
through the real recall candidate-selection + ranking paths to compute
precision@K, recall@K, MRR, and nDCG against a committed baseline.

**Labeling correctness matters more than volume.** A single mis-graded case
silently miscalibrates every metric derived from it. Treat the labels as a spec,
not a sketch.

## Layout

```
golden/
├── README.md              # this file
├── _generate.py           # authoring tool: emits memories/ + _id_registry.json
├── _generate_queries.py   # authoring tool: emits queries.yaml from the registry
├── _id_registry.json      # path -> MemoryId map (generated; the queries source of truth)
├── memories/              # the corpus, mirroring the spec §6 tree layout
│   ├── me/                # identity, relationship facts, preferences, corrections, knowledge, episodic
│   ├── projects/          # atlas / orbit / quill — three fictional projects
│   └── agent/             # patterns, anti-patterns, heuristics, postmortems, playbooks, regressions
└── queries.yaml           # 56 labeled query cases
```

Every memory is real Markdown + YAML frontmatter conforming to the Stream A
schema (spec §6/§7). `tests/golden_fixtures_lint.rs` validates each file through
the actual `memory_substrate::frontmatter::parse_document` pipeline, so a file
that wouldn't be accepted by the daemon is rejected here too.

## The fictional team

One backend team at "Northwind Systems", owner **Dana Okafor** (the `me`
subject — staff engineer, tech lead of Atlas). Three projects:

- **Atlas** (`atlas/billing`) — payments / billing platform. Home of the
  multi-version **ledger migration saga** and the money-handling invariants.
- **Orbit** (`orbit/identity`) — internal auth / identity service. Home of the
  **session-cookies → stateless-JWT auth refactor**.
- **Quill** (`quill/docs`) — docs / CMS frontend. Home of the **flaky-CI hunt**
  (auto-retry band-aid → shared-test-DB root cause).

`agent`-scope memories are the cross-cutting lessons distilled from that work
(patterns, anti-patterns, postmortems, heuristics).

## Deliberately hard structure

These mirror the recall failure modes the quality metrics must detect:

| Structure | Where | What it tests |
|---|---|---|
| **Supersession chains** (6) | atlas ledger v1→v2→v3; orbit auth v1→v2; orbit rate-limit v1→v2; quill flaky v1→v2; me title v1→v2→v3; agent migration-heuristic v1→v2 | Only the *head* should recall; tails are traps. |
| **Near-duplicate pairs** (4) | me standup/morning-sync; atlas processor-retry/gateway-retry; atlas idempotency invariant/playbook; agent one-step-rollback/instant-revert | Recall should collapse, not double-surface. |
| **Cross-project entity collisions** (3) | `gateway` (atlas payment adapter vs orbit API gateway); `pipeline` (atlas billing pipeline vs quill CI pipeline); `Dana` (Dana Okafor the user vs Dana Wu on Quill) | Same surface form, different referent — scope/project must disambiguate. |
| **Stale-vs-fresh competing facts** | me language pref (2025 Go vs 2026 Rust); me job title chain | Fresher/head fact wins; stale loses. |
| **Tombstoned memories** (3) | me old-laptop; atlas PayFast vendor-pick; agent cache-everything | Must never surface as current. |
| **Privacy-sensitive** (2) | atlas confidential pricing; me personal focus-block | Exercise the masked / non-indexed path. |

## Labeling rubric

Each `queries.yaml` case grades memories into three disjoint sets:

- **`essential`** — a correct answer to the query is *incomplete* without this
  memory. If recall misses an essential id, recall@K for the case is penalized.
  Reserve this for the load-bearing answer(s); usually 1–2 ids.

- **`useful`** — relevant supporting context. It improves an answer but the
  answer is still correct without it. Graded lower (contributes to nDCG gain,
  not to the essential-recall floor). Examples: a related playbook, the
  postmortem behind a decision, a corroborating pattern.

- **`irrelevant_traps`** — memories that *look* relevant (lexical, entity, or
  topical overlap) but **must not surface**. A ranker that returns a trap is
  wrong, not merely imprecise. Traps are always one of:
  - a **superseded tail** when the query wants the current state,
  - a **wrong-project collision** when the query is scoped to one project,
  - a **tombstoned** memory, or
  - a **stale competing fact** the fresher memory replaced.

  Traps are the precision teeth of the corpus. Per-case precision penalizes any
  trap that appears in the top-K.

**Abstention cases** (`qNN-abstain-*`) have empty `essential` *and* `useful`:
the correct behavior is to surface nothing relevant. They measure
false-positive resistance — the counterpart to recall. Some abstention cases
still list traps (e.g. "what's the user's current machine?" → the tombstoned
old-laptop is a trap, and the right answer is "no current memory").

### Disjointness invariant

Within a case, `essential` / `useful` / `irrelevant_traps` are pairwise
disjoint. A memory can't be both load-bearing and a trap for the same query.
`tests/golden_fixtures_lint.rs::graded_sets_are_disjoint_per_case` enforces it.

### Query-case coverage

The 56 cases span: exact-identifier recall (`q01`–`q03`), entity queries incl.
collisions (`q04`–`q11`), topical queries (`q12`–`q23`), supersession-head
selection (`q24`–`q28`), cross-project isolation (`q29`–`q31`), agent
pattern/postmortem recall (`q32`–`q40`), near-duplicate collapse (`q41`–`q42`),
correction/tombstone traps (`q43`–`q46`), abstention (`q47`–`q50`), and short
keyword/entity search probes (`q51`–`q56`). The keyword probes give the bm25
`memory_search` seam nonzero dynamic range while the longer natural-language
cases continue to document FTS5 AND-of-phrases limitations.

`namespace_scope` uses `"project:<alias>"` to narrow to a single project; the
quality runner maps the alias (`atlas`/`orbit`/`quill`) to its
`canonical_namespace_id` at load time. Bare `project` / `me` / `agent` mean the
whole namespace is in scope.

## How to extend

1. **Add or change memories** in `_generate.py` — never hand-edit the emitted
   files (they're regenerated and your edit would be lost; the MemoryId hashes
   are derived). Keep new memories consistent with the team fiction and honor
   the validator rules (project scope requires `namespace` +
   `canonical_namespace_id`; `confidential`/`personal` sensitivity must set
   `index_body: false` and `index_embeddings: false`; tombstoned needs
   `tombstone_events`; superseded needs `superseded_by`). Re-run:
   `python3 _generate.py`.

2. **Add or change query cases** in `_generate_queries.py`, referencing
   memories by their path-key (not by raw id). Re-run: `python3
   _generate_queries.py`. The script self-checks disjointness before writing.

3. **Run the lint** (the coordinator's gate does this):
   `cargo test -p memorum-eval --test golden_fixtures_lint`.

4. When you add a new *kind* of hard structure, add a row to the table above and
   at least one query case that exercises it.

> **Invariant reminder:** `secret` is a runtime `ClassificationOutcome`, never a
> persisted `sensitivity` value. It must not appear in any frontmatter here; the
> lint test fails the build if it does.
