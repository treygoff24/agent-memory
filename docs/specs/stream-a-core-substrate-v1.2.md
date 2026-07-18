# Stream A — Core Substrate Spec (v1.2)

**Status:** final implementation spec, 2026-04-26. Supersedes `stream-a-core-substrate-v1.0.md`; no open Stream A substrate decisions remain in this document.

**Parent:** `docs/specs/system-v0.1.md`. Stream A implements the canonical storage/index/event/git substrate that every later stream depends on.

**Revision goal (v0.2 → v1.0):** close the last substrate-contract gaps before implementation.

1. Device-sharded IDs must not rely on a collision-prone 16-bit shard.
2. Duplicate-ID repair must allocate from the repo-visible free ID set, not only local `seq.json`.
3. Human-edited files may omit known nullable/collection keys; the parser must auto-populate typed defaults and warn.
4. Committed-but-incomplete write states must have durable repair queues with idempotent replay.
5. Startup reconciliation must ingest valid offline human edits instead of treating all dirty worktrees as corruption.
6. Secret material must have a single rule: it is a classification result refused before disk, not a persisted frontmatter enum.
7. Vector/embedding consistency must be represented as durable embedding jobs plus stale-hash checks, not an impossible rollback claim.
8. Metadata-only encrypted records must not imply Stream A can decrypt them.
9. Add/add quarantine must preserve both logical memories in a structured, mechanically recoverable form.
10. Event kinds, config precedence, watcher races, and public async/blocking contracts must be explicit.

**Revision goal (v1.2):** add optional `abstraction` and `cues` frontmatter fields with canonical serialization, validation, and merge semantics; add derived abstraction/cue embedding row kinds with full lifecycle; index schema 5→6 (additive); classification contract composes over the new fields. No behavior change for memories that lack the new fields. (Ratified 2026-07-10, Memora-lessons arc; source: docs/specs/2026-07-10-w2-spec-ratification-package.md §A.)

**Revision goal (v1.0 → v1.1):** close substrate-contract gaps surfaced by Codex's implementation-plan review on 2026-04-25.

11. Sensitivity classification must have an explicit on-call contract: Stream A cannot classify content, so callers must pass a typed `ClassificationOutcome` and Stream A must enforce routing/refusal from it. Without this, `WriteFailureKind::SecretRefused` has no concrete trigger inside Stream A.
12. Embedding model/dimension changes must have explicit migration semantics: dimension is part of vector-table identity, mismatch never silently downgrades, and old chunks are queued for re-embedding under the new model.
13. `WatchSubscription` ownership semantics promised in §16.5 must be backed by an acceptance signal — substrate drop while a subscription is alive is a documented contract, not folklore.
14. The merge driver's "supported schema version" must be a named constant, not a magic number scattered through the binary.
15. Two-clone convergence must have a precise byte-level definition; without it the release-gate "converges byte-identically" claim is unverifiable.
16. Performance gates must specify a baseline source, regression detection method, and the provenance of the 10K-corpus vectors. Stream A cannot run a real embedding model (Stream B owns inference), so synthetic deterministic vectors must be sanctioned.
17. The release gate must distinguish CI-enforceable criteria from manual-evidence criteria. macOS arm64 is currently manual; the spec must say so honestly rather than imply uniform CI coverage.
18. Spec acceptance signals must be mechanically traceable to named tests via a coverage manifest, the same way event kinds and error variants already are.

**Post-v1.1 authorized additions (2026-05-02, F-003 ratification):** Stream G's review loop surfaced two substrate APIs that are intentionally part of the public Stream A surface:

19. `Substrate::update_encrypted_memory_metadata(&self, id: &MemoryId, mutate: impl FnOnce(&mut Memory)) -> Result<(), WriteFailure>` may update safe metadata for an encrypted canonical memory without decrypting or replacing ciphertext. The implementation must reject plaintext memories, preserve the encrypted ciphertext bytes and encryption envelope, preserve the encrypted path, validate the mutated frontmatter, and apply the same compare-and-swap atomic write semantics as other metadata writes.
20. `Substrate::query_recall_index_including_metadata_only(&self, query: RecallIndexQuery) -> SubstrateResult<Vec<RecallIndexRow>>` may include encrypted metadata-only rows in the recall-index projection for observability/scoring consumers. It must project only indexed safe metadata and auxiliary tables; it must not hydrate encrypted envelopes or expose plaintext body fragments from encrypted rows.

## 1. Purpose

Stream A produces:

1. `memory-substrate`, a Rust library crate that owns canonical filesystem writes, frontmatter parsing/validation, SQLite indexing, JSONL event logs, local git operations, and repo validation.
2. `memory-merge-driver`, a standalone Rust binary invoked by git for Markdown memory-file merges.
3. A fixture-heavy test suite proving durability, merge convergence, schema validation, query/index correctness, and clone/adoption behavior.

Stream A is **not a daemon**. Stream B owns process lifecycle and calls Stream A. Stream A remains the only layer allowed to mutate the memory repo, SQLite index, or event logs. Other streams may classify, govern, encrypt, or assemble recall, but they hand canonical mutations back to Stream A.

---

## 2. Scope and boundaries

### 2.1 In scope

- Memory tree layout under the synced repo root.
- Local per-device runtime state layout outside the repo.
- Synced config vs. local device config split.
- Frontmatter schema, type constraints, cross-field constraints, and cross-file validation hooks.
- Markdown read/write with crash-safe same-filesystem atomic replace.
- ID generation and duplicate-ID recovery.
- SQLite schema, migrations, indexer, chunk-level FTS, vector-storage contract, and query primitives.
- File watcher events and duplicate self-event suppression.
- Recoverable per-device JSONL event logs.
- Git init, clone adoption, auto-commit, fetch/merge/push, and merge-driver preflight.
- Semantic frontmatter merge driver.
- Public Rust API surface used by Stream B/C/D/E.

### 2.2 Out of scope

- Daemon startup, socket server, MCP protocol, and human UI surfaces: Stream B/G.
- Governance decisions such as promotion, contradiction detection, grounding verification, tombstone matching, and review queues: Stream C.
- Secret detection, Privacy Filter inference, age encryption, masked synthesis: Stream D.
- Recall block assembly and harness hooks: Stream E.
- Dreaming: Stream F.
- Full eval harness: Stream H.
- Live event subscriptions: Stream I.

### 2.3 Boundary rules

1. **Only Stream A writes repo files, SQLite, event logs, and git state.** If Stream D encrypts content, it returns ciphertext plus safe metadata to Stream A; Stream A writes it.
2. **SQLite is derived.** It may be deleted and rebuilt from canonical files, but while present it must be transactionally consistent with acknowledged writes.
3. **Event logs are durable audit, not the source of truth.** If an event append fails after a file write is durable, Stream A must reconcile on startup and emit a repair event.
4. **Human/editor changes are allowed.** The watcher/indexer must ingest them. Programmatic writes use compare-and-swap preconditions to avoid overwriting unseen human edits.
5. **No hidden second index.** Stream E consumes Stream A's chunk-level index; it must not need to build its own private search database to make passive recall work.

---

## 3. Platform assumptions

- macOS 14+ and Linux kernel 5.10+. Windows is explicitly out of scope for v1.0; atomic-rename and fsync semantics on NTFS require a separate write strategy.
- POSIX `rename(2)` atomicity only **within the same filesystem**.
- Git 2.40+.
- SQLite 3.45+ with JSON1 and FTS5.
- Filesystem case-sensitivity is not assumed. All path uniqueness checks compare both exact bytes and case-folded relative paths.

### 3.1 Durability tiers

Parent-directory fsync support is the gating signal for full write durability. Stream A probes for it at startup and pins a tier; behavior depends on the tier:

| Tier | Probe result | Behavior |
| --- | --- | --- |
| `Full` | `fsync(parent_dir_fd)` succeeds on the memory root and on `events/` | Default. Writes acknowledged only after the §8.3 sequence completes. |
| `BestEffort` | parent-dir fsync returns a documented non-fatal error (e.g. older glibc on certain remote filesystems) | Writes acknowledged with `WriteOutcome.durability = BestEffort`. Callers must explicitly opt in via `WriteRequest.allow_best_effort_durability = true`; otherwise `write_memory` returns `WriteFailureKind::DurabilityUnavailable`. |
| `Refused` | parent-dir fsync returns `EINVAL`/`ENOTSUP` or panics, or the probe cannot be run | `Substrate::open` returns `OpenError::DurabilityUnsupported`. Stream B may force-open with `InitOptions::force_unsafe_durability = true` for tests/CI only; that flag is logged and surfaced in every `WriteOutcome`. |

`DoctorReport.durability_tier` exposes the resolved tier. `Substrate::durability_tier()` is a public read accessor so Stream B can refuse to start if policy requires `Full`.

---

## 4. Component map

```text
memory-substrate
├── tree                 # layout, path rules, cross-file validation
├── config               # synced config + local device config + env overrides
├── ids                  # device-sharded IDs, sequence persistence, duplicate recovery
├── frontmatter          # schema, parser, validator, canonical serializer
├── markdown             # atomic read/write/delete with CAS
├── index                # SQLite schema, migrations, chunking, query helpers
├── watcher              # filesystem events and duplicate-notification suppression
├── events               # recoverable JSONL append/read/replay
├── git                  # init/adopt/commit/fetch/merge/push/preflight
└── merge                # shared semantic merge implementation

memory-merge-driver
└── invokes memory_substrate::merge on one path; never performs repo-level renames
```

Repo-level operations such as duplicate-ID repair, same-commit reference rewrites, and clone adoption live in `git`/daemon reconciliation, not in the path-local merge-driver binary.

---

## 5. Memory tree layout

### 5.1 Synced repo paths

The canonical repo root defaults to `~/.memory/` and is configurable.

```text
~/.memory/
├── .git/
├── .gitattributes
├── .gitignore
├── config.yaml                         # synced user/project config only; no device id
├── me/
│   ├── identity/
│   │   ├── role.md
│   │   └── principles.md
│   ├── relationship/
│   │   ├── facts/<entity-slug>.md
│   │   ├── preferences/<topic-slug>.md
│   │   ├── corrections/<memory-id>.md
│   │   └── patterns/<memory-id>.md
│   ├── knowledge/<topic-slug>.md
│   ├── episodic/<YYYY-MM-DD>.md
│   └── prospective/<memory-id>.md
├── projects/
│   └── <namespace-segment>/<sub>/
│       ├── state.md
│       ├── decisions/<YYYY-MM-DD>-<slug>.md
│       ├── open-questions/<memory-id>.md
│       ├── playbooks/<slug>.md
│       ├── entities/<entity-slug>.md
│       ├── episodic/<YYYY-MM-DD>.md
│       ├── invariants.md
│       └── regressions/<memory-id>.md
├── agent/
│   ├── patterns/<memory-id>.md
│   ├── playbooks/<slug>.md
│   ├── postmortems/<memory-id>.md
│   ├── anti-patterns/<memory-id>.md
│   ├── heuristics/<memory-id>.md
│   ├── regressions/<memory-id>.md
│   └── episodic/<YYYY-MM-DD>.md
├── dreams/
│   ├── journal/<YYYY-MM-DD>.md
│   ├── questions/<YYYY-MM-DD>.md
│   └── reports/<phase>/<YYYY-MM-DD>.md
├── substrate/
│   └── <device-id>/<YYYY-MM-DD>.jsonl
├── encrypted/
│   ├── me/...
│   ├── projects/...
│   └── agent/...
├── tombstones/
│   └── <YYYY-MM-DD>.jsonl
├── events/
│   └── <device-id>.jsonl
├── policies/
│   ├── me-strict.yaml
│   ├── project-standard.yaml
│   ├── agent-strict.yaml
│   └── dreaming-strict.yaml
└── leases/
    └── journal.lease
```

Git cannot track empty directories. `tree::init` creates all directories in the working tree, but the initial commit tracks only `.gitattributes`, `.gitignore`, `config.yaml`, and `.keep` placeholders where cloned directory existence is required. The spec must never rely on an empty directory existing after clone unless `git::adopt_clone` recreates it.

### 5.2 Local runtime paths

Per-device runtime state is **not synced**:

```text
~/.memoryd/
├── local-device.yaml                    # device id, device name, local paths
├── seq.json                             # per-device ID sequence state
├── event-seq.json                       # per-device event sequence state
├── pending/
│   ├── index-ops.jsonl                  # durable file→index repair queue
│   └── events.jsonl                     # durable event-after-commit repair queue
├── index.sqlite
├── index.sqlite-wal
├── index.sqlite-shm
├── socket                               # Stream B
├── pid                                  # Stream B
├── logs/memoryd.log
└── tmp/                                 # non-canonical scratch only; not used for atomic final renames
```

`config.yaml` in the repo contains portable configuration. `local-device.yaml` contains device identity and any local-only overrides. `event-seq.json` contains only `{ "device_id": <device-id>, "next": <u64> }` and is locked/fsynced with the same discipline as `seq.json`. A fresh clone must never inherit the previous machine's device identity or event sequence state.

### 5.3 Path constraints

- `<memory-id>` matches `^mem_\d{8}_[0-9a-f]{16}_\d{6}$`.
- The 16-hex shard is derived from the local `device_id` and is stable for that device.
- `<slug>` and `<entity-slug>` match `[a-z0-9][a-z0-9-]{0,62}`.
- `<YYYY-MM-DD>` is a UTC ISO calendar date.
- `<namespace-segment>` follows slug rules.
- Relative paths must be unique under case-folded comparison.
- ID-based filenames must match frontmatter `id`. Slug-based filenames need not match frontmatter `id`, but they must be stable unless explicitly renamed through Stream A.

### 5.4 Tree validator

`tree::validate(root) -> TreeValidationReport` checks:

- canonical path patterns;
- slug and date validity;
- case-folded collisions;
- ID filename/frontmatter mismatch for ID-based paths;
- duplicate frontmatter IDs anywhere in the tree;
- forbidden plaintext files under encrypted tiers;
- supersession graph acyclicity and existence of referenced IDs when the repo is fully synced;
- inverse supersession consistency when both endpoints exist;
- local git merge-driver config presence when validation mode is `StartupPreflight`.

Missing referenced IDs are warnings during `PartialSync` mode and errors during `FullySynced` mode.

### 5.5 Acceptance signals

- Fresh init creates the working-tree directories and commits the exact tracked bootstrap files.
- Fresh clone plus `git::adopt_clone` regenerates local device identity, event log file, merge-driver config, and missing directories.
- A fixture with duplicate IDs in different paths fails validation.
- A fixture with case-only path collisions fails validation on Linux and macOS.
- A fixture with a supersession cycle fails cross-file validation.

---

## 6. Frontmatter schema

### 6.1 Required fields

Every memory frontmatter block includes these fields in canonical order:

| Field | Type | Constraint |
| --- | --- | --- |
| `schema_version` | integer | currently `1` |
| `id` | string | `^mem_\d{8}_[0-9a-f]{16}_\d{6}$` |
| `type` | enum | `project`, `person`, `procedure`, `episode`, `claim`, `artifact`, `prospective`, `pattern`, `playbook`, `postmortem`, `anti-pattern`, `heuristic`, `regression`, `correction`, `invariant`, `decision`, `open-question` |
| `scope` | enum | `user`, `project`, `org`, `agent`, `subagent` |
| `summary` | string | 1-280 chars |
| `confidence` | float | 0.0 <= x <= 1.0; no implicit default |
| `trust_level` | enum | `trusted`, `untrusted`, `candidate`, `quarantined`, `pinned` |
| `sensitivity` | enum | `public`, `internal`, `confidential`, `personal` |
| `status` | enum | `candidate`, `active`, `pinned`, `superseded`, `archived`, `tombstoned`, `quarantined` |
| `created_at` | datetime | RFC3339 UTC `Z` |
| `updated_at` | datetime | RFC3339 UTC `Z`, >= `created_at` |
| `author` | object | structured principal, see §6.4 |

### 6.2 Known nullable/collection fields

Every memory's canonical serialization contains every key in this table. The serializer always emits them. The **parser** is permissive: when a known nullable/collection key is absent on read, the parser materializes the field-specific typed default below and emits `ValidationWarning::AutoPopulatedNullableField { field }`. The validator does **not** fail on missing nullable/collection keys; this preserves human-edit affordance while keeping round-trip output canonical.

v1.2 adds two known nullable/collection fields, last in canonical serialization order (after `_merge_diagnostics`, before `_extras`):

Wrong types, bad enums, and missing required *scalar/object* fields (§6.1) remain hard errors. If `schema_version` is higher than supported, Stream A refuses mutation for the whole file (§9.3); it does not attempt per-field validation of unknown future requirements.

| Field | Type |
| --- | --- |
| `namespace` | string or null |
| `canonical_namespace_id` | string or null; `^proj_[0-9a-f]{16}$` when set |
| `tags` | array of slugs |
| `entities` | array of `{id, label, aliases}` |
| `aliases` | array of strings |
| `source` | object, §6.4 |
| `evidence` | array of evidence objects, §6.5 |
| `requires_user_confirmation` | bool |
| `review_state` | `pending`, `approved`, `rejected`, or null |
| `observed_at` | datetime or null |
| `valid_from` | datetime or null |
| `valid_until` | datetime or null |
| `ttl` | ISO8601 duration or null |
| `supersedes` | array of memory IDs |
| `superseded_by` | array of memory IDs |
| `related` | array of memory IDs |
| `tombstone_events` | array of tombstone event objects |
| `retrieval_policy` | object, §6.6 |
| `write_policy` | object, §6.6 |
| `regression` | object or null, §6.7 |
| `prospective` | object or null, §6.8 |
| `privacy_scan` | object or null, §6.9 |
| `_merge_diagnostics` | object or null, §6.10 |
| `abstraction` | string or null |
| `cues` | array of strings |

Defaults when absent on read: `abstraction: null`, `cues: []` (standard §6.2 permissive-parser materialization + `AutoPopulatedNullableField` warning). File `schema_version` stays `1`: both fields are optional, and pre-v1.2 parsers preserve them via `_extras` round-trip. (Known mixed-version wart, accepted for single-device dogfood: the **shipped** merge driver's `_extras` path resolves divergent values silently ours-wins (`field_rules.rs` `three_way_value` fallthrough) — which itself diverges from the written §14.4 quarantine wording, a pre-existing spec/code drift logged in `docs/issues.md`. Divergent cue edits across mixed-version devices are side-dependent until both devices run v1.2. Noted, not fixed.)

Field-specific defaults for missing known nullable/collection keys:

| Field class | Default |
| --- | --- |
| nullable scalar datetime/string/duration | `null` |
| arrays (`tags`, `entities`, `aliases`, `evidence`, `supersedes`, `superseded_by`, `related`, `tombstone_events`) | `[]` |
| `source` | `{ kind: import, ref: null, harness: null, harness_version: null, session_id: null, subagent_id: null, device: null }` |
| `retrieval_policy` | generated from `scope` and `sensitivity`: `passive_recall=true`; `max_scope=scope` for `user/project/org/agent`, `max_scope=agent` for `subagent`; `mask_personal_for_synthesis=true` when `sensitivity in {confidential, personal}`; `index_body` and `index_embeddings` true only when `sensitivity in {public, internal}` |
| `write_policy` | `{ human_review_required: false, policy_applied: "default-v1", expected_base_hash: null }` |
| `regression`, `prospective`, `privacy_scan`, `_merge_diagnostics` | `null` |
| `abstraction` | `null` |
| `cues` | `[]` |

Unknown future fields are preserved in `_extras` by the parser and re-emitted after known fields. Unknown fields produce warnings when `schema_version <= supported`; files with higher `schema_version` are read-only (§9.3).

### 6.3 Lifecycle matrix

`status` represents lifecycle. `trust_level` represents evidentiary trust. Valid combinations:

| `status` | Allowed `trust_level` |
| --- | --- |
| `candidate` | `candidate`, `untrusted`, `quarantined` |
| `active` | `trusted`, `untrusted` |
| `pinned` | `pinned`, `trusted` |
| `superseded` | `trusted`, `untrusted`, `candidate` |
| `archived` | `trusted`, `untrusted`, `candidate` |
| `tombstoned` | any except `pinned` |
| `quarantined` | `quarantined` |

The validator rejects invalid combinations. If a merge quarantines a memory, both `status: quarantined` and `trust_level: quarantined` are set.

### 6.4 `author` and `source`

`author` is a structured object, not a string. Stringly-typed colon-delimited principals are a parser hazard the moment a session ID, harness name, or subagent ID contains a colon.

```yaml
author:
  kind: user | agent | subagent | dreaming | system
  user_handle: string | null            # hashed or slugged; never a raw email
  harness: string | null                # slug; examples: claude-code, codex, cursor, cli
  harness_version: string | null
  session_id: string | null
  subagent_id: string | null
  phase: string | null                  # for dreaming
  component: string | null              # for system
```

Validator rules:

- `kind == user` requires `user_handle`; all other harness fields must be null.
- `kind == agent` requires `harness` and `session_id`.
- `kind == subagent` requires `harness`, `session_id`, and `subagent_id`.
- `kind == dreaming` requires `phase`.
- `kind == system` requires `component`.
- `user_handle`, when set, matches `[a-z0-9][a-z0-9._-]{0,62}` and never embeds an `@`. Hashes are written as `sha256:<hex64>`.
- `harness`, when set, matches `[a-z0-9][a-z0-9._-]{0,62}`. The validator must not bake in a closed list of harnesses.

```yaml
source:
  kind: user | agent-primary | agent-subagent | tool | web | email | file | synthesis | import | system
  ref: string | null
  harness: string | null                # same slug rule as author.harness
  harness_version: string | null
  session_id: string | null
  subagent_id: string | null
  device: string | null
```

`source` is not overwritten on merge. On true 3-way conflict, the merged file preserves the winning `source` and records the losing source in `_merge_diagnostics.preserved_sources` unless the same source already exists in evidence provenance.

### 6.5 Evidence entries

```yaml
evidence:
  - id: ev_<ulid>
    quote: 'quoted support'
    quote_norm_hash: sha256:<hex>
    ref: 'file:line or artifact handle'
    weight: 1.0
    observed_at: 2026-04-24T12:00:00Z
    source: string | null
```

Evidence identity is `id`. For imported older records without IDs, merge identity is `(quote_norm_hash, ref)`, where normalization trims surrounding whitespace, collapses internal whitespace runs, normalizes Unicode to NFC, and normalizes line endings. If two evidence entries share normalized identity but differ in raw `quote`, preserve the earliest raw quote and append alternates to `_merge_diagnostics.evidence_near_duplicates`.

Tombstone event schema:

```yaml
tombstone_events:
  - id: tomb_<ulid>
    applied_at: datetime
    actor:
      kind: user | agent | system
      ref: string
    reason: duplicate | wrong | stale | privacy | user-request | policy | other
    reason_text: string | null
    reason_hash: sha256:<hex64>
    prior_status: candidate | active | pinned | superseded | archived | quarantined
```

Tombstone event identity is `id`. Imported records without IDs merge by `(applied_at, actor.ref, reason_hash)`.

### 6.6 Policy objects

```yaml
retrieval_policy:
  passive_recall: bool
  max_scope: user | project | org | agent
  mask_personal_for_synthesis: bool
  index_body: bool
  index_embeddings: bool

write_policy:
  human_review_required: bool
  policy_applied: string
  expected_base_hash: string | null
```

`index_body` and `index_embeddings` are false by default for encrypted/sensitive records unless Stream D supplies a safe masked index projection.

### 6.7 `regression`

```yaml
regression:
  detection_signature:
    error_string_regex: string | null
    stack_fingerprint: string | null
    tool_output_hash: string | null
    behavioral_marker: string | null
  fire_on_attempt: bool
  first_observed: datetime
  last_observed: datetime
  occurrence_count: integer
  occurrence_counter:
    <device-id>: integer
  occurrences:
    - id: occ_<ulid>
      observed_at: datetime
      device: <device-id>
      source_ref: string | null
```

`occurrence_count` is a derived convenience equal to the number of unique occurrence IDs, or the sum of `occurrence_counter` values if detailed occurrences were compacted. Merge never sums raw `occurrence_count` directly.

### 6.8 `prospective`

`prospective` is required when `type == prospective`.

```yaml
prospective:
  trigger:
    kind: time | event | condition
    at: datetime | null
    cron: string | null
    event_ref: string | null
    condition: string | null
  owner: user | agent | system
  state: pending | armed | fired | completed | cancelled | expired
  external_scheduler_ref: string | null
  last_checked_at: datetime | null
  fired_at: datetime | null
  completed_at: datetime | null
  verification:
    required: bool
    method: none | user_confirm | tool_check | external_event
    evidence_ref: string | null
```

Stream A does not schedule prospective memories, but it must be able to store and validate them without forcing a v1.x schema break.

### 6.9 `privacy_scan`

```yaml
privacy_scan:
  model: string
  ran_at: datetime
  spans_detected: integer
  labels: array
  span_details_ref: string | null
```

If merge sees different scan models, it preserves both in `_merge_diagnostics.privacy_scans_preserved` and keeps the newest scan in `privacy_scan`.

### 6.10 `_merge_diagnostics`

`_merge_diagnostics` is a known optional field, not an unknown extra.

```yaml
_merge_diagnostics:
  merge_id: merge_<ulid>
  created_at: datetime
  status: clean_with_warnings | quarantined
  conflicting_fields: array
  preserved_sources: array
  evidence_near_duplicates: array
  privacy_scans_preserved: array
  add_add_alternates: array
  unparsed_sides: array
  lifecycle_notes: array
  human_reason: string
```

`add_add_alternates[]` is used only for add/add same-path quarantine and contains mechanically recoverable losing blobs:

```yaml
add_add_alternates:
  - id: mem_20260424_a1b2c3d4e5f60718_000087
    original_path: projects/example/decisions/foo.md
    frontmatter_yaml_b64: string
    body_sha256: sha256:<hex64>
    body_b64: string | null
    body_artifact_ref: string | null
```

Exactly one of `body_b64` or `body_artifact_ref` is set. It is preserved through future merges by unioning array fields by stable IDs or normalized content hashes. It may be cleared only by a human/admin resolution command that emits an event.

`unparsed_sides[]` stores raw merge inputs that had identifiable frontmatter delimiters but invalid YAML:

```yaml
unparsed_sides:
  - side: base | ours | theirs
    path: string
    frontmatter_raw_b64: string
    body_b64: string
    parse_error: string
```

### 6.11 Cross-field constraints

Validated per file:

1. `updated_at >= created_at`.
2. `valid_until > valid_from` when both are set.
3. `scope in {project, org}` requires `namespace` and `canonical_namespace_id`; other scopes require both null unless explicitly allowed by a future schema.
4. `status == superseded` requires `superseded_by` non-empty.
5. `status == tombstoned` requires `tombstone_events` non-empty and `superseded_by` empty.
6. `status == quarantined` requires `_merge_diagnostics.status == quarantined` and `review_state == pending`.
7. `id` must not appear in `supersedes`, `superseded_by`, or `related`.
8. `supersedes` and `superseded_by` must not overlap.
9. If `superseded_by` is non-empty, status must be `superseded` unless status is `quarantined`.
10. `secret` is not a persisted `sensitivity` value. It is a `ClassificationOutcome` (§8.7) value supplied by Stream D on every write request; when present, Stream A returns `WriteFailureKind::SecretRefused` before any repo, SQLite, event-log, or temp-file write. Stream A does not perform classification itself, but it does enforce the routing implied by every value.
11. `type == regression` requires `regression` and at least one detection signature.
12. `type == prospective` requires `prospective`.
13. If `privacy_scan.labels` includes `private_credential`, Stream A refuses new writes unless Stream D supplies a non-secret redacted metadata record. Existing files with `private_credential` labels are validation errors unless `status == quarantined` and the body is a redacted stub.

Validated cross-file by `tree::validate`:

1. Supersession graph is acyclic.
2. If A lists B in `superseded_by`, B should list A in `supersedes` when both exist; mismatches are warnings in `PartialSync`, errors in `FullySynced`.
3. Tombstoned memories are not targets of `superseded_by`.
4. No active memory may reference a tombstoned memory in `related` unless a tombstone policy allows historical references.

### 6.12 YAML serialization

- Block style for maps/lists except empty arrays `[]`.
- LF line endings only.
- RFC3339 UTC timestamps with explicit `Z`.
- Canonical key order follows this section order.
- Deterministic sorting for sets: tags/aliases lexicographic; IDs lexicographic; evidence by `observed_at`, then `id`; entities by `id`; tombstone events by `applied_at`, then `id`.
- Round-trip canonical serialization is byte-stable.

### 6.13 Acceptance signals

- Positive and negative tests for every field and every cross-field rule.
- A prospective memory with time trigger, event trigger, and conditional trigger validates.
- A tombstoned memory with two tombstone events validates and round-trips.
- A quarantine file produced by the merge driver validates.
- Unknown v1.x optional fields parse, warn, preserve, and reserialize when `schema_version <= supported`; higher schema versions are read-only.
- Supersession cycle fixtures fail cross-file validation.

---

## 7. ID generation

### 7.1 Format

`mem_YYYYMMDD_<device-shard>_<seq>`.

Example: `mem_20260424_a1b2c3d4e5f60718_000087`.

- `YYYYMMDD` is UTC date at mint time.
- `device-shard` is the first 16 lowercase hex chars of SHA256(local `device_id`).
- `seq` is a six-digit per-device daily sequence from `000001` to `999999`.

This preserves human sortability while removing the broken random-offset collision strategy. A 64-bit shard makes accidental same-shard collisions negligible for the intended single-user/multi-device deployment. `git::adopt_clone` and startup preflight still scan known event logs and memory IDs for shard collisions; if the local shard collides with an existing different `device_id`, adoption regenerates `device_id` until the shard is unused.

### 7.2 Sequence allocation

`~/.memoryd/seq.json`:

```json
{
  "date": "2026-04-24",
  "next": 87,
  "device_id": "dev_a1b2c3d4e5f60718"
}
```

`ids::next(ctx)`:

1. Open `seq.json` under exclusive lock.
2. If local device ID differs, return `IdError::DeviceMismatch` and require clone adoption repair.
3. Scan the in-memory ID high-water mark loaded during `Substrate::open`; if existing IDs for `(today_utc, local_shard)` have sequence >= `next`, advance `next` to max existing sequence + 1 before minting.
4. If date changed, set `date=today_utc`, `next=max(1, max_existing_sequence(today_utc, local_shard)+1)`.
5. If `next > 999999`, return `IdError::SequenceExhausted { date }`.
6. Return ID with current seq, increment `next`, fsync file, fsync parent directory, release lock.

### 7.3 Duplicate-ID recovery

Duplicate IDs should happen only if a repo was cloned/copied without adoption or if a bug reused a device ID. The path-local merge driver does **not** repair duplicate IDs.

Repo-level reconciliation (`git::repair_duplicate_ids`) runs after fetch/merge and during startup validation:

1. Detect duplicate frontmatter IDs across paths.
2. Select canonical survivor by earliest `(created_at, git commit timestamp, device_id, path)`.
3. For each non-survivor, mint a new valid ID with `ids::mint_next_unused(date, local_shard, reserved_ids)`. `reserved_ids` is the full repo-visible ID set plus IDs already minted in the current repair transaction. The allocator advances `seq.json` past every existing ID for `(date, local_shard)` before returning.
4. Rename ID-based files to the new ID path; slug-based files keep path unless path collision exists.
5. Rewrite references in files changed in the same reconciliation transaction: `supersedes`, `superseded_by`, `related`, evidence refs that explicitly point to memory IDs, and known sidecars.
6. Emit `DuplicateIdRepaired` events with old/new IDs and affected paths.
7. Reindex affected files.

If references cannot be rewritten safely, quarantine the affected files with valid `status: quarantined` and do not silently drop either memory.

### 7.4 Acceptance signals

- 10,000 sequential IDs on one device are unique and monotonic by sequence.
- Two devices with different non-colliding 64-bit shards mint 50,000 IDs each for the same UTC day with zero collisions.
- Adoption detects a forced same-shard collision fixture and regenerates local device identity before any write.
- Sequence `999999` succeeds; `1000000` returns `SequenceExhausted`.
- A fixture with duplicate IDs from a copied device is repaired into valid IDs matching the regex, with references rewritten or quarantined.
- Duplicate repair where local `seq.json.next` lags existing same-shard IDs mints the next unused repo-visible ID, not another duplicate.

---

## 8. Markdown file I/O and durable write transaction

### 8.1 Read

`markdown::read(path) -> Result<Memory, ReadError>`:

1. Open file.
2. Parse frontmatter delimiters.
3. Parse YAML into `FrontmatterRaw` preserving unknown fields and comments only where supported by the serializer contract.
4. Normalize body line endings to LF in memory.
5. Return raw memory plus file hash, mtime, and path.

### 8.2 Write preconditions

Every mutating write takes `WriteRequest`:

```rust
struct WriteRequest {
    operation_id: Option<OperationId>,  // generated by Stream A if None
    memory: Memory,
    expected_base_hash: Option<Sha256>,
    write_mode: WriteMode,          // CreateNew | ReplaceExisting | AdminRepair
    index_projection: Option<IndexProjection>,
    event_context: EventContext,
    allow_best_effort_durability: bool,
}
```

- `CreateNew` fails if final path already exists.
- `ReplaceExisting` fails with `WriteFailureKind::StaleBase` if `expected_base_hash` does not match current file hash.
- `AdminRepair` bypasses CAS only for explicit repair operations and emits an admin repair event.

### 8.3 Atomic write sequence

For plaintext indexable writes:

1. Generate or validate `operation_id`.
2. Validate frontmatter and path.
3. Serialize canonical YAML + body.
4. Compute final path, final file hash, and ensure target parent exists.
5. Check CAS preconditions from §8.2.
6. Create temp file in the **same directory as the final file**: `<parent>/.<basename>.<op_id>.tmp` with `O_CREAT|O_EXCL`.
7. Write full buffer with short-write retry loop.
8. `fsync(temp_fd)`.
9. Insert an in-memory watcher suppression entry in state `InFlight { op_id, path, expected_final_hash }` before the rename. This entry expires if the process dies or the write aborts; it exists only to close the notification race between `rename` and committed-ledger insertion.
10. `rename(temp, final)` within the same directory.
11. `fsync(parent_dir_fd)` if `DurabilityTier == Full`; skip with explicit best-effort flag if `BestEffort` (see §3.1).
12. Promote the watcher suppression entry to `Committed { final_file_hash, committed_at, expires_at }`.
13. Apply SQLite index transaction directly for this operation.
14. Append and fsync event log entry.
15. Return success with `WriteOutcome.durability` set to the active tier.

If steps 6-11 fail, remove temp file if present, remove the in-flight suppression entry if present, and return `Err(WriteFailure { outcome: WriteOutcome::not_committed(), kind })`. If step 13 fails after the file is durable, write `~/.memoryd/startup-reconcile.required`, append a durable `PendingIndexOp` to `~/.memoryd/pending/index-ops.jsonl`, and return `Err(WriteFailure { outcome: WriteOutcome { committed: true, indexed: false, event_recorded: false, durability, repair_required: PendingIndex }, kind: IndexAfterCommitFailed })`. If step 14 fails after file/index durability, append a durable `PendingEventOp` to `~/.memoryd/pending/events.jsonl` and return `Ok(WriteOutcome { committed: true, indexed: true, event_recorded: false, durability, repair_required: PendingEvent })`. Callers must never retry a file write when `outcome.committed == true`.

If appending either pending repair record fails after the file is durable, Stream A fsyncs `~/.memoryd/startup-reconcile.required` with `repair_required: FullStartupScan` and returns `Err(WriteFailure { outcome: committed outcome, kind: RepairQueueFailed })`. If both the pending queue append and the marker write fail, Stream A returns `Err(WriteFailure { outcome: committed outcome, kind: RepairStateNotDurable })`; Stream B must stop accepting writes and surface operator repair. This is the only acknowledged state in which repair metadata may be incomplete, and it is covered by a fault-injection test.

> **Amendment (2026-06-10, operator-approved):** The shipped implementation diverges from the failure-mapping sentences above on the plaintext write path, and the shipped behavior is hereby blessed as canonical. (a) When the pending-queue append fails after a committed plaintext write, the implementation returns `kind: IndexAfterCommitFailed` (not `RepairQueueFailed`) — the failure kind names the degraded pipeline stage, and `outcome.repair_required` carries the repair state. (b) The marker write is the durable *fallback* when the pending-op append fails, rather than marker-and-pending-op being written together. Both mappings predate the repair-cascade extraction (which preserved them verbatim) and are pinned by the write-contract tests; `RepairQueueFailed` remains in use on the cascade paths that already returned it. Operator ruling: Trey, 2026-06-10 — spec text bends to shipped behavior; no caller-visible change.

Durable repair queue records:

```rust
struct PendingIndexOp {
    op_id: OperationId,
    kind: PendingIndexKind,      // UpsertPath | DeletePath
    path: RepoPath,
    memory_id: Option<MemoryId>,
    expected_file_hash: Option<Sha256>,
    enqueued_at: DateTime<Utc>,
    attempts: u32,
    last_error: Option<String>,
}

struct PendingEventOp {
    op_id: OperationId,
    event_id: EventId,
    event: Event,
    enqueued_at: DateTime<Utc>,
    attempts: u32,
    last_error: Option<String>,
}
```

Both queue files use the same framed JSONL discipline as §12.3 and are fsynced after append. Replay is idempotent: `PendingIndexOp` is keyed by `(op_id, expected_file_hash)` and no-ops if the indexed row already matches that hash; `PendingEventOp` is keyed by `event_id` and no-ops if that event already exists with the same checksum. Completed records are compacted into `*.compacted.jsonl` with fsync+rename+parent-fsync; compaction never runs until all records in the source file have been replayed or copied forward.

### 8.4 Sensitive/encrypted writes

Stream A never writes plaintext sensitive content to repo paths.

For `sensitivity in {confidential, personal}`:

1. Stream D classifies and encrypts content or produces an approved masked projection.
2. Stream D calls Stream A with `EncryptedWriteRequest { metadata_frontmatter, ciphertext, safe_index_projection }`.
3. Stream A validates metadata, writes ciphertext atomically under `encrypted/<original-relative-path>`, indexes only the safe projection, and emits events.
4. If no safe projection exists, the memory remains retrievable by ID/path metadata only; body FTS and embeddings are disabled.

If Stream D classifies content as `secret`, Stream A returns `WriteFailureKind::SecretRefused` before creating temp files, pending queues, SQLite rows, or events. `secret` is not a valid on-disk frontmatter sensitivity.

### 8.5 Delete/tombstone

Hard delete is admin-only. Normal forget operations write a tombstone event and update frontmatter to `status: tombstoned` with `tombstone_events[]`; they do not erase git history. Privacy leak runbooks live in Stream D/G.

### 8.6 Acceptance signals

- Atomic write tests stage in target parent and prove `EXDEV` cannot occur.
- Crash tests cover before write, during write, after temp fsync, after rename before parent fsync, after parent fsync before index, after index before event, and after event.
- Stale-base write returns `WriteFailureKind::StaleBase` and leaves the file unchanged.
- Confidential write never writes plaintext bytes to repo path or SQLite FTS/vector tables.
- Event-after-commit failure produces a committed outcome plus startup reconciliation marker.
- A `WriteRequest` with `classification = Secret` returns `WriteFailureKind::SecretRefused` and never creates a temp file, SQLite row, vector row, event, or pending-queue record. Verified by inspecting the filesystem and SQLite state after the call.
- A `WriteRequest` with `classification = RequiresEncryption` is refused with `WriteFailureKind::EncryptionRequired` from the plaintext write path; the same memory accepted via `write_encrypted` succeeds.
- A `WriteRequest` with `classification = Trusted` whose frontmatter `sensitivity in {confidential, personal}` is refused with `WriteFailureKind::ClassificationSensitivityMismatch`. Stream A never silently downgrades or upgrades the classification.

### 8.7 Classification contract

Stream A does not classify content. Sensitivity classification is Stream D's job, but Stream A is the only layer that can refuse a write before disk. The two are reconciled by making classification a typed input to every write request rather than something Stream A infers from frontmatter alone.

```rust
pub enum ClassificationOutcome {
    /// Caller asserts content is safe to write as plaintext under the
    /// frontmatter `sensitivity`. Allowed only when sensitivity is
    /// `public` or `internal`.
    Trusted,
    /// Caller asserts content must be encrypted before disk. Stream A
    /// refuses plaintext writes; only `write_encrypted` accepts this
    /// classification.
    RequiresEncryption,
    /// Caller asserts content is secret material (credentials, tokens,
    /// keys). Stream A refuses every write path before any disk effect.
    Secret,
}

pub struct WriteRequest {
    /* …existing fields per §8.2… */
    pub classification: ClassificationOutcome,
}
```

Enforcement rules:

1. `classification` is required on every `WriteRequest` and `EncryptedWriteRequest`. There is no default.
2. `Secret` always returns `WriteFailureKind::SecretRefused` before §8.3 step 6 (temp-file creation). No row, no event, no pending-queue record.
3. `RequiresEncryption` is rejected by `write_memory` (the plaintext path) with `WriteFailureKind::EncryptionRequired`. It is the only classification accepted by `write_encrypted`.
4. `Trusted` is rejected if the frontmatter `sensitivity` is `confidential` or `personal` — this catches Stream D bugs where a sensitive memory is marked trusted by mistake. Returns `WriteFailureKind::ClassificationSensitivityMismatch`.
5. The classification never appears in persisted frontmatter. It is a transport-only contract between Stream D and Stream A.
6. Stream A logs the classification and the decision in the `WriteCommitted`/`WriteRefused` event payload so audit can confirm Stream D made a positive call on every write.

This makes `WriteFailureKind::SecretRefused` a real, testable Stream A code path even before Stream D exists: tests pass `classification = Secret` directly and assert the refusal contract.

**One shared pipeline for every create/supersede write entrypoint** — `write`, `write-note`, supersede, import execute, `review approve`, `quarantine resolve --edited`, dream fragment→memory, backfill, merge staging (W3) — in this fixed order: **normalize/cap-validate → classify → drop/refuse/encrypt decision → validate the final payload → persist/index/enqueue.** No canonical file write, no index row, no aux row, and no embedding job may be created before the pipeline completes. The combined payload = title + summary + body + `abstraction` + every `cues` entry; strictest outcome controls the whole write. `secret` anywhere (including in a cue) refuses before any disk effect (invariant #1). The in-place `metadata_amend` operation in the 2026-07-15 amendment is not a create/supersede write: it refuses a tier increase and never invokes this generation-context drop/rebind path.

**Generation-context drop semantics (dual classification + outcome rebind):** for create/supersede generation writes only, in dream fragment→memory and W3 merge staging (which additionally floors the result at the strictest source classification, applied *after* the rebind), the classify step runs **twice** — once over the combined payload, once over the body-only payload (title + summary + body). If the combined outcome is stricter than the body-only outcome (the strictness caused *by* the generated fields, now well-defined), the pipeline drops `abstraction`/`cues` and **rebinds the write to the body-only `ClassificationOutcome`** (and its sensitivity) before persisting — without the rebind, the write would still carry `RequiresEncryption` and the plaintext path would refuse, losing the body. `secret` in either classification still refuses the whole write. Acceptance: a sensitive-cue-on-public-body fragment→memory or W3 staging write commits the body with `abstraction: null`/`cues: []`, creates no aux rows or jobs, and the write's persisted outcome is the body-only one. This does not apply to in-place `metadata_amend`, which refuses a tier increase. Interactive writes do not drop — they refuse/error per the existing contract.

Entrypoint enumeration and the hardcoded-`Trusted` audit are W2 implementation deliverables (plan task 3); the contract here is the pipeline they must all route through — including the shipped substrate path (`api/write.rs`), which today classifies before frontmatter validation and must be reconciled to this order.

---

## 9. Validator

### 9.1 Passes

1. YAML parse and type pass.
2. Required-key and enum pass.
3. Per-file cross-field pass.
4. Canonical serialization pass.
5. Cross-file validation pass when validating a tree.

Type/required failures short-circuit for a file. Cross-field pass collects all applicable errors.
**Validation (§9 additions):**

- `abstraction`: ≤ 8 words (whitespace-split), ≤ 120 chars, single line, no control chars, NFC-normalized, trimmed, internal whitespace collapsed. Violation = hard error on write, `ValidationWarning` + field dropped to `null` on read of a hand-edited file (permissive read, canonical rewrite repairs).
- `cues`: 0–3 entries after normalization; each ≤ 6 words, ≤ 64 chars, single line, no control chars, NFC, trimmed, whitespace-collapsed; duplicates under case-fold are removed (first in side-independent total order kept). Same hard-on-write / repair-on-read posture.

### 9.2 Error structure

The validator distinguishes **errors** (the file is wrong and the author must fix it) from **warnings** (the file is mechanically repairable; the parser/canonicalizer fixed it on read).

```rust
enum ValidationError {
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    MissingRequiredScalar { field: FieldPath },
    WrongType { field: FieldPath, expected: String, found: String },
    EnumOutOfDomain { field: FieldPath, value: String, allowed: Vec<String> },
    RegexMismatch { field: FieldPath, value: String, pattern: String },
    CrossFieldViolation { rule: String, fields: Vec<FieldPath> },
    SecretRefusedBeforeWrite,
    InvalidId { value: String },
    InvalidSlug { value: String },
    DatetimeFormat { field: FieldPath, value: String },
    DurationFormat { field: FieldPath, value: String },
    SupersessionCycle { ids: Vec<MemoryId> },
    DuplicateId { id: MemoryId, paths: Vec<PathBuf> },
    UnknownFieldUnderHigherSchema { field: FieldPath, schema_version: u32 },
}

enum ValidationWarning {
    AutoPopulatedNullableField { field: FieldPath },     // §6.2
    UnknownField { field: FieldPath },
    MissingReferencePartialSync { id: MemoryId, referenced_by: MemoryId },
    NonCanonicalYaml { path: PathBuf },
    MergeDiagnosticsPresent { id: MemoryId },
}
```

Mechanically repairable conditions (missing known nullable keys, non-canonical YAML, sort order) emit warnings only; the canonical serializer guarantees the on-disk form converges to canonical on the next write through Stream A. Semantic errors (bad enum, regex mismatch, cycle, duplicate ID, missing required scalar) hard-fail.

### 9.3 Schema evolution

Every memory has `schema_version`. v0.x evolution rules:

- Adding optional nullable fields is allowed in a minor revision if old daemons preserve unknown fields.
- Adding required fields requires a new `schema_version` and a migration that backfills all files.
- Old daemons encountering higher `schema_version` must refuse mutation but may read metadata in read-only mode if parsing succeeds.
- Migrations are idempotent and logged.

### 9.4 Acceptance signals

- Every required field, enum, regex, and cross-field rule has positive and negative tests.
- A memory missing a known nullable/collection key parses, emits `AutoPopulatedNullableField`, validates if the materialized default satisfies cross-field rules, and canonical serialization re-emits the key.
- A higher `schema_version` memory is read-only, not mutated.
- Cross-file validation catches cycles, duplicate IDs, and inverse supersession mismatches.

---

## 10. SQLite indexer

### 10.1 Schema overview

SQLite is a derived projection with chunk-level search. The exact sqlite-vec DDL is adapter-owned; Stream A's stable schema contract is below.

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE memories (
    id                    TEXT PRIMARY KEY,
    path                  TEXT NOT NULL UNIQUE,
    schema_version         INTEGER NOT NULL,
    type                  TEXT NOT NULL,
    scope                 TEXT NOT NULL,
    namespace             TEXT,
    canonical_namespace_id TEXT,
    summary               TEXT NOT NULL,
    confidence            REAL NOT NULL,
    trust_level           TEXT NOT NULL,
    sensitivity           TEXT NOT NULL,
    status                TEXT NOT NULL,
    review_state          TEXT,
    requires_user_confirmation INTEGER NOT NULL,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    observed_at           TEXT,
    valid_from            TEXT,
    valid_until           TEXT,
    ttl                   TEXT,
    author                TEXT NOT NULL,
    source_kind           TEXT NOT NULL,
    source_harness        TEXT,
    source_device         TEXT,
    body_hash             TEXT NOT NULL,
    frontmatter_json      TEXT NOT NULL CHECK (json_valid(frontmatter_json)),
    file_hash             TEXT NOT NULL,
    file_mtime_ns         INTEGER NOT NULL,
    indexed_at            TEXT NOT NULL
);

CREATE INDEX idx_memories_scope_canon_status_sens_updated
    ON memories(scope, canonical_namespace_id, status, sensitivity, updated_at DESC);
CREATE INDEX idx_memories_type_status_updated
    ON memories(type, status, updated_at DESC);
CREATE INDEX idx_memories_source_updated
    ON memories(source_kind, updated_at DESC);
CREATE INDEX idx_memories_review
    ON memories(review_state, requires_user_confirmation);
CREATE INDEX idx_memories_path_nocase
    ON memories(path COLLATE NOCASE);

-- chunk_rowid is an explicit INTEGER PRIMARY KEY AUTOINCREMENT so VACUUM
-- cannot permute it. SQLite VACUUM permutes rowids of tables that lack an
-- explicit INTEGER PRIMARY KEY; the FTS5 external-content table stores
-- content rowids internally, so a VACUUM-induced rowid permutation would
-- silently break every chunk-search join. AUTOINCREMENT additionally
-- prevents rowid reuse after delete, which would corrupt the FTS link.
CREATE TABLE memory_chunks (
    chunk_rowid    INTEGER PRIMARY KEY AUTOINCREMENT,
    chunk_id       TEXT NOT NULL UNIQUE,
    memory_id      TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    chunk_text     TEXT NOT NULL,
    token_start    INTEGER NOT NULL,
    token_end      INTEGER NOT NULL,
    byte_start     INTEGER NOT NULL,
    byte_end       INTEGER NOT NULL,
    chunk_hash     TEXT NOT NULL,
    summary        TEXT NOT NULL,
    indexable      INTEGER NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE(memory_id, ordinal)
);
CREATE INDEX idx_chunks_memory ON memory_chunks(memory_id, ordinal);
CREATE INDEX idx_chunks_indexable ON memory_chunks(indexable);
CREATE INDEX idx_chunks_chunk_id ON memory_chunks(chunk_id);

CREATE VIRTUAL TABLE memory_chunks_fts USING fts5(
    chunk_text,
    summary,
    content='memory_chunks',
    content_rowid='chunk_rowid',
    tokenize='porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER memory_chunks_ai AFTER INSERT ON memory_chunks WHEN new.indexable = 1 BEGIN
    INSERT INTO memory_chunks_fts(rowid, chunk_text, summary)
    VALUES (new.chunk_rowid, new.chunk_text, new.summary);
END;
CREATE TRIGGER memory_chunks_ad AFTER DELETE ON memory_chunks WHEN old.indexable = 1 BEGIN
    INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, chunk_text, summary)
    VALUES('delete', old.chunk_rowid, old.chunk_text, old.summary);
END;
CREATE TRIGGER memory_chunks_au AFTER UPDATE ON memory_chunks BEGIN
    INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, chunk_text, summary)
    SELECT 'delete', old.chunk_rowid, old.chunk_text, old.summary WHERE old.indexable = 1;
    INSERT INTO memory_chunks_fts(rowid, chunk_text, summary)
    SELECT new.chunk_rowid, new.chunk_text, new.summary WHERE new.indexable = 1;
END;

CREATE TABLE memory_tags (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    tag       TEXT NOT NULL,
    PRIMARY KEY(memory_id, tag)
);
CREATE INDEX idx_tags_tag_memory ON memory_tags(tag, memory_id);

CREATE TABLE memory_aliases (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    alias_norm TEXT NOT NULL,
    alias_raw TEXT NOT NULL,
    PRIMARY KEY(memory_id, alias_norm)
);
CREATE INDEX idx_aliases_norm ON memory_aliases(alias_norm, memory_id);

CREATE TABLE memory_entities (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    label     TEXT NOT NULL,
    PRIMARY KEY(memory_id, entity_id)
);
CREATE INDEX idx_entities_entity_memory ON memory_entities(entity_id, memory_id);

CREATE TABLE memory_entity_aliases (
    memory_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    alias_norm TEXT NOT NULL,
    alias_raw TEXT NOT NULL,
    PRIMARY KEY(memory_id, entity_id, alias_norm),
    FOREIGN KEY(memory_id, entity_id)
        REFERENCES memory_entities(memory_id, entity_id)
        ON DELETE CASCADE
);
CREATE INDEX idx_entity_aliases_norm ON memory_entity_aliases(alias_norm, memory_id, entity_id);

CREATE TABLE memory_supersession (
    source_memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    earlier_id       TEXT NOT NULL,
    later_id         TEXT NOT NULL,
    PRIMARY KEY(source_memory_id, earlier_id, later_id),
    CHECK (earlier_id <> later_id)
);
CREATE INDEX idx_supersession_later ON memory_supersession(later_id);
CREATE INDEX idx_supersession_earlier ON memory_supersession(earlier_id);

CREATE TABLE memory_related (
    source_memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    a_id TEXT NOT NULL,
    b_id TEXT NOT NULL,
    PRIMARY KEY(source_memory_id, a_id, b_id),
    CHECK(a_id < b_id)
);
CREATE INDEX idx_related_b ON memory_related(b_id, a_id);
CREATE INDEX idx_related_a ON memory_related(a_id, b_id);

CREATE TABLE memory_evidence (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    evidence_id TEXT NOT NULL,
    quote TEXT NOT NULL,
    quote_norm_hash TEXT NOT NULL,
    ref TEXT NOT NULL,
    weight REAL NOT NULL,
    observed_at TEXT,
    source TEXT,
    PRIMARY KEY(memory_id, evidence_id)
);
CREATE INDEX idx_evidence_ref ON memory_evidence(ref, memory_id);
CREATE INDEX idx_evidence_hash_ref ON memory_evidence(quote_norm_hash, ref);

CREATE TABLE memory_regressions (
    memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    error_string_regex TEXT,
    stack_fingerprint TEXT,
    tool_output_hash TEXT,
    behavioral_marker TEXT,
    fire_on_attempt INTEGER NOT NULL,
    first_observed TEXT NOT NULL,
    last_observed TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL
);
CREATE INDEX idx_regressions_stack_fp ON memory_regressions(stack_fingerprint) WHERE stack_fingerprint IS NOT NULL;
CREATE INDEX idx_regressions_tool_hash ON memory_regressions(tool_output_hash) WHERE tool_output_hash IS NOT NULL;
CREATE INDEX idx_regressions_fire ON memory_regressions(fire_on_attempt, last_observed DESC);

CREATE TABLE memory_regression_occurrences (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    occurrence_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    source_ref TEXT,
    PRIMARY KEY(memory_id, occurrence_id)
);

CREATE TABLE chunk_embedding_meta (
    chunk_id TEXT PRIMARY KEY REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    model_ref TEXT NOT NULL,
    dimension INTEGER NOT NULL,
    vector_table TEXT NOT NULL,
    embedded_at TEXT NOT NULL,
    content_hash TEXT NOT NULL
);

CREATE TABLE pending_embedding_jobs (
    chunk_id TEXT PRIMARY KEY REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    model_ref TEXT NOT NULL,
    dimension INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    enqueued_at TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX idx_pending_embedding_jobs_enqueued ON pending_embedding_jobs(enqueued_at);

CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

#### Schema migration 6

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

Vector storage is behind an adapter:

```rust
trait VectorStore {
    fn create_or_open(model: EmbeddingModelRef, dimension: u32) -> Result<Self, VectorError>;
    fn upsert_chunk(&self, chunk_id: &ChunkId, vector: &[f32], meta: EmbeddingMeta) -> Result<(), VectorError>;
    fn delete_chunk(&self, chunk_id: &ChunkId) -> Result<(), VectorError>;
    fn list_chunk_ids(&self) -> Result<Vec<ChunkId>, VectorError>;
    fn contains_chunk(&self, chunk_id: &ChunkId) -> Result<bool, VectorError>;
    fn search(&self, query: &[f32], filter: VectorFilter, limit: usize) -> Result<Vec<VectorHit>, VectorError>;
}
```

The v1.0 default adapter is sqlite-vec. Because vector virtual-table DDL is extension-specific, the implementation must pin the tested sqlite-vec version in `Cargo.lock`/build metadata and generate adapter DDL from code, not hand-maintain ambiguous SQL in this spec.

### 10.2 Indexer behavior

Indexer operations are explicit, transaction-wrapped reconciliations:

- `Created`/`Modified`: read, validate, compute chunks, replace memory row and all derived rows; for each indexable chunk with `retrieval_policy.index_embeddings == true`, enqueue or refresh a `pending_embedding_jobs` row keyed by `chunk_id` and `content_hash`.
- `Deleted`: delete memory row by path after resolving current ID; cascades remove chunks, FTS rows, tags, aliases, entities, evidence, regressions, embedding metadata, and pending embedding jobs. Vector adapter deletion is best-effort inline and guaranteed by startup reconciliation (§10.2.1).
- `Renamed`: read destination file. If frontmatter ID unchanged, update path and reconcile derived rows. If frontmatter ID changed, delete old ID and upsert new ID. Never path-update blindly.
- `Tombstoned` or sensitivity changes: purge non-indexable chunks/vectors immediately unless a safe masked projection is provided.

Index transactions never hold a SQLite connection across async await points. Blocking SQLite work is done on a dedicated blocking executor or single index thread owned by Stream B.

### 10.2.1 Vector store consistency

The vector store is an external adapter (§10.1) and may live in a sqlite-vec virtual table, a sidecar SQLite database, or a process-external store. **None of these are guaranteed to honor an enclosing SQLite transaction's rollback.** Stream A therefore models embeddings as durable jobs plus stale-hash-checked updates, not as an atomic extension of the file/index transaction.

Contract:

1. **Chunk indexing.** The indexer writes `memory_chunks` and `pending_embedding_jobs` in the same SQLite transaction. No `chunk_embedding_meta` row exists until a vector has actually been written.
2. **Embedding worker.** Stream B drains `pending_embedding_jobs`, computes vectors, and calls `Substrate::update_embedding` with `{ chunk_id, expected_content_hash, provider, model_ref, dimension, vector }`.
3. **Stale protection.** `update_embedding` validates that the chunk exists, is indexable, and `memory_chunks.chunk_hash == expected_content_hash`. Mismatch returns `VectorError::StaleChunk` and does not touch the vector store.
4. **Update order.** `update_embedding` upserts the vector first, then in one SQLite transaction upserts `chunk_embedding_meta` and deletes the matching `pending_embedding_jobs` row. If SQLite commit fails after vector upsert, startup reconciliation sees an orphan vector and deletes it; the pending job remains or is re-enqueued.
5. **Delete/tombstone/sensitivity downgrade.** SQLite metadata changes delete chunks/jobs/meta first. Vector deletes are attempted inline; failures are corrected by startup reconciliation.
6. **Startup pass.** `Substrate::open` runs vector reconciliation before accepting writes:
   - `VectorStore::list_chunk_ids()` minus `chunk_embedding_meta` → delete orphan vectors.
   - `chunk_embedding_meta` minus `VectorStore::contains_chunk()` → delete stale meta and enqueue a fresh `pending_embedding_jobs` row.
   - `pending_embedding_jobs` rows whose chunks no longer exist or whose `content_hash` no longer matches are dropped with `VectorReconciled`.
   - The pass emits `VectorReconciliationReport { orphan_vectors_deleted, missing_vectors_requeued, stale_jobs_dropped }`.

The §10.5 invariants are stated in terms of the **post-reconciliation** state. Stream B must not treat newly-acknowledged writes as having vector coverage until the corresponding `pending_embedding_jobs` row has been drained and `chunk_embedding_meta` exists with the current `content_hash`.

#### Auxiliary abstraction and cue rows

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

### 10.2.2 Embedding model and dimension migration

`chunk_embedding_meta` and `pending_embedding_jobs` already key vectors by `(provider, model_ref, dimension)`. sqlite-vec virtual tables are dimension-fixed, so a real model swap means a new vector table, not an `ALTER`. Stream A's contract:

1. **Vector-table identity.** Each `(provider, model_ref, dimension)` triple maps to its own `VectorStore` instance, named deterministically by the adapter (e.g. `vec_chunks__local_gemma__embeddinggemma_300m_qat_q8_0__768`). The adapter creates the table on first use; it never re-uses a table across triples.
2. **Active triple.** `config.yaml` `embeddings.default_provider`, `default_model_ref`, and `default_dimension` define the **active** triple. The active triple is what `pending_embedding_jobs` are enqueued against and what hybrid query helpers consult by default.
3. **Switching the active triple.** Changing any element of the active triple in `config.yaml` is a configuration migration, not a schema migration. On `Substrate::open`:
   - Stream A enqueues a `pending_embedding_jobs` row for every indexable chunk that lacks `chunk_embedding_meta` for the new active triple.
   - Old vector tables for prior triples are **not** deleted automatically; they remain queryable until an explicit `Substrate::drop_embedding_model { provider, model_ref, dimension }` call removes the table and its `chunk_embedding_meta` rows.
   - The transition emits `EmbeddingModelChanged { from: Option<EmbeddingTriple>, to: EmbeddingTriple, chunks_requeued }`.
4. **Dimension mismatch on update.** `update_embedding` validates that the supplied `dimension` matches the active triple's dimension when the caller does not specify a triple, or that it matches the explicitly supplied triple. Mismatch returns `VectorError::DimensionMismatch { expected, found }` and never writes to any vector table.
5. **No silent fallback.** Stream A never embeds against a different model than the caller asked for. If `update_embedding` arrives for a triple that has been dropped, it returns `VectorError::UnknownEmbeddingTriple { provider, model_ref, dimension }`.
6. **Query routing.** Vector queries default to the active triple. Callers may supply an explicit triple to query historical vectors; results from different triples are never silently mixed in a single ranked list.

The default active triple in v1.0/v1.2 is locked by §20: `local-gemma / embeddinggemma-300m-qat-Q8_0 / 768`.

### 10.3 Chunking contract

Default chunking:

- Target ~400 tokens, 80-token overlap.
- Chunk boundaries prefer Markdown headings, paragraphs, then sentence boundaries.
- Chunks include byte offsets into normalized LF body.
- `chunk_id = chk_<sha256(memory_id || chunker_version || ordinal || chunk_hash)>`. Edits that change chunk text create new chunk IDs; stale embedding updates must fail by content hash.
- Any body above 1 MiB is artifacted or chunked streaming; it must not be copied into a single SQLite `body` column.

### 10.4 Query contract

Stream A exposes typed read-only query helpers for the MCP shapes Stream B/E need:

- by ID/path;
- by tag/entity/alias;
- by namespace/scope/status/type/sensitivity/time;
- FTS chunk search with snippets;
- vector chunk search through adapter;
- hybrid result assembly with per-hit `score_breakdown` inputs, not final policy ranking.

**Metadata-only memories.** A confidential or personal memory without a Stream D safe projection has zero rows in `memory_chunks` and no vectors. Chunk-level FTS and vector queries must not return it. Metadata queries (`query_memory`) **do** return it with `MemoryHit.body_indexability == MetadataOnly` and `content_state == EncryptedMetadataOnly`. Stream E may surface summary/metadata and may request an authorized Stream D projection/decrypt flow; Stream A's direct read returns ciphertext/envelope metadata, not plaintext. Hybrid result assembly skips metadata-only memories unless the caller sets `MemoryQuery.include_metadata_only = true`.

Raw mutable SQLite access is not exported. A test-only read-only SQL API may exist behind `cfg(test)` or an explicit admin feature.

### 10.5 Integrity invariants

- `memories.id == frontmatter_json.id`.
- Every indexed path exists at transaction time unless processing a delete.
- Every chunk belongs to one memory.
- `memory_chunks.chunk_rowid` is stable across `VACUUM` (it is `INTEGER PRIMARY KEY AUTOINCREMENT`).
- FTS rows are updated with the SQLite FTS5 external-content delete+insert trigger pattern keyed on `chunk_rowid`.
- Old unique terms disappear after update/delete.
- No vector exists for a missing, tombstoned, or non-indexable chunk **after the next reconciliation pass completes** (§10.2.1). Newly indexed chunks without vectors are represented by `pending_embedding_jobs`, not by false `chunk_embedding_meta` coverage.
- Reindex from files produces the same query-visible state as watcher-driven incremental indexing.

### 10.6 Acceptance signals

- 10K-memory load test includes long bodies, large bodies, aliases, entity aliases, regressions, prospective memories, tombstones, encrypted metadata, and supersession chains.
- FTS mutation test proves old terms vanish after update/delete.
- **VACUUM regression test:** load 1K chunks, run `VACUUM`, run a chunk FTS query that previously matched, verify the same `chunk_id`s come back. Regression-protects the §10.1 fix.
- Vector lifecycle test proves delete/tombstone/sensitivity changes purge vectors after reconciliation.
- **Embedding stale update test:** compute vector for chunk hash A, modify the file to chunk hash B, then call `update_embedding` with expected hash A and verify `VectorError::StaleChunk`.
- **Vector orphan/missing reconciliation test:** seed an orphan vector and a missing vector before `Substrate::open`, verify orphan deletion and missing-vector job requeue on startup.
- Query p95 targets are measured for real Stream E shapes: namespace + status + sensitivity cap + entity/alias + updated_at sort.
- **Secret-in-cue refusal (v1.2):** a `secret` classification triggered by any cue refuses the whole write before any disk effect.
- **Sensitive-generation drop (v1.2):** sensitive-abstraction-on-public-body in a fragment→memory or W3 staging write drops the generated fields, keeps the body, creates no aux rows/jobs, and persists the body-only outcome (dual-classification rebind).
- **B3 amendment tier refusal (v1.2):** sensitive generated fields on `metadata_amend` return `MetadataAmendmentTierIncreaseRefused` and leave the body untouched.
- **Upgrade revocation (v1.2):** sensitivity upgrade deletes API-lane vectors across all row kinds and, under default policy, local ones too — both the default-delete and override-re-enqueue paths tested.
- **Two-clone cue-merge convergence (v1.2):** opposite merge directions with overflowing unions and casing-only duplicates produce identical results.
- **Abstraction merge tie-break (v1.2):** conflict resolves by `updated_at` with the sha256 tie-break; loser preserved in `_merge_diagnostics`.
- **Migration 6 (v1.2):** up-migration + rollback-by-restore on a copied live DB.
- **Reindex rebuild (v1.2):** reindex-from-files fully rebuilds all v1.2 derived tables.
- **Aux stale-write fence (v1.2):** hash-change invalidation guarantees no stale-hash aux vector is ever servable.
- **Status-transition matrix (v1.2):** supersede/tombstone/quarantine clear aux state; enter-servable re-materializes rows + jobs and the memory becomes aux-retrievable again.
- **Triple-switch aux re-enqueue (v1.2):** re-enqueue counts include the aux row kinds.
- **Doctor per-kind counts (v1.2):** doctor/status report per-row-kind indexed/pending/held-local counts.
- Rename tests cover path-only rename and rename plus ID change.
- Metadata-only memory test: confidential memory with no Stream D projection appears in metadata query results, never in chunk FTS or vector search results, and `MemoryHit.body_indexability == MetadataOnly`.
- **Dimension-mismatch test:** `update_embedding` with a vector of dimension 1024 against an active triple of dimension 768 returns `VectorError::DimensionMismatch` and writes nothing.
- **Active-triple switch test:** load 1K indexable chunks under triple A (768-dim), change `config.yaml` to triple B (1024-dim), reopen Substrate, verify (a) every chunk has a `pending_embedding_jobs` row for triple B, (b) triple A's vector table is still queryable when explicitly addressed, (c) an `EmbeddingModelChanged` event was emitted with the correct `chunks_requeued` count.
- **Drop-triple test:** after `Substrate::drop_embedding_model` removes triple A, vector queries against triple A return `VectorError::UnknownEmbeddingTriple`, and triple B queries are unaffected.

---

## 11. File watcher

### 11.1 Event model

`Watcher` emits:

```rust
enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    RescanRequired { reason: WatcherOverflowReason },
}
```

The watcher may coalesce or overproduce events. Exact one-event-per-operation is not guaranteed by FSEvents/inotify and is not required for correctness.

### 11.2 Watch root and filters

Watch the repo root recursively, excluding `.git/`, editor backups, `.DS_Store`, and temp files matching Stream A's same-directory temp pattern. Do not watch `~/.memoryd/`.

### 11.3 Duplicate self-event suppression

Substrate writes update SQLite directly before returning. The watcher suppression ledger only prevents duplicate work from the filesystem notification that follows that write.

Suppression key:

```rust
struct SuppressionEntry {
    op_id: OperationId,
    path: PathBuf,
    state: InFlight | Committed,
    expected_final_hash: Sha256,
    final_file_hash: Option<Sha256>,
    committed_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
}
```

A watcher event is suppressed only if the current file hash equals `expected_final_hash`/`final_file_hash`. If an external editor modifies the same path within the suppression window, the hash differs and the event is processed. An event arriving between rename and committed-ledger promotion still matches the in-flight entry and is suppressed.

Default expiry is 60 seconds, but correctness does not depend on expiry; hash mismatch wins.

### 11.4 Acceptance signals

- Substrate write updates the index even when the watcher notification is suppressed.
- External edit to the same path within the suppression window is indexed.
- Watcher overflow emits `RescanRequired` and a reindex converges.
- Mass changes converge to fresh-reindex state; tests do not assert impossible exact OS event counts.
- **WatchSubscription outlives Substrate.** A test obtains a `WatchSubscription`, drops the owning `Substrate`, performs filesystem changes inside the watched root, and verifies the subscription continues to deliver `FileEvent`s until either `WatchSubscription::unsubscribe()` is called or the handle is dropped. After unsubscribe/drop, OS-level watcher resources (file descriptors on Linux, FSEventStreams on macOS) are released synchronously, verified by counting the relevant resource before and after.

---

## 12. Event log

### 12.1 Format

Per-device JSONL at `events/<device-id>.jsonl`. Device IDs are local and unique per adopted clone.

Each line is one framed event:

```json
{"schema":1,"id":"evt_01HX...","ts":"2026-04-24T13:14:15.123Z","device":"dev_a1b2...","seq":42,"kind":"WriteCommitted","data":{},"crc32c":"..."}
```

`seq` is per-device monotonic and persisted in `~/.memoryd/event-seq.json` under exclusive lock. Event append increments the sequence only after the event buffer and sequence file are both fsynced; on recovery, Stream A sets `event-seq.next = max(existing seq for device) + 1`. ULID timestamp order is useful for display but not treated as causal truth.

### 12.2 Event kinds

- `WriteStarted`
- `WriteCommitted`
- `WriteIndexed`
- `WriteEventAppendFailed`
- `WriteRefused`
- `Deleted`
- `Tombstoned`
- `Superseded`
- `IndexUpdated`
- `IndexFailed`
- `VectorReconciled`
- `EmbeddingJobEnqueued`
- `EventLogRecovered`
- `MergeQuarantined`
- `DuplicateIdRepaired`
- `PendingIndexReplayed`
- `PendingEventReplayed`
- `GitCommitted`
- `GitFetched`
- `GitPushFailed`
- `WatcherSuppressed`
- `OperatorRepairRequired`
- `ReconciliationRepaired`
- `StartupReconciliationCompleted`
- `MetadataAmended`

Every kind has a typed data schema in code and fixtures. Free-form `data` is not permitted in implementation even if rendered schematically in docs.

### 12.3 Append semantics

1. Encode event as one bounded UTF-8 buffer. Max line length: 64 KiB; larger payloads must be artifacted and referenced.
2. Append with file opened `O_APPEND`.
3. Retry short writes until full buffer is written or an error occurs.
4. `fsync(log_fd)` after each event for v1.0.
5. On startup, read line by line, validate JSON and checksum. If the final line is incomplete or checksum-invalid, truncate exactly that trailing line and emit `EventLogRecovered`. Non-final malformed lines quarantine the log and require admin repair.

`O_APPEND` guarantees atomic offset selection, not arbitrary-record crash atomicity. Recovery is part of the contract.

### 12.4 Multi-device union

Readers union all `events/*.jsonl` by `(ts, device, seq, id)` for display. Replay/dedup identity is event `id`; duplicate IDs with different checksums are errors. Duplicate identical events are ignored idempotently and logged as warnings.

### 12.5 Acceptance signals

- Crash during append/fsync leaves either a complete final event or one truncated trailing line that recovery truncates.
- Repeated union merges do not double-apply duplicate event IDs.
- Same-device-id duplicate logs from a bad clone are detected and refused until adoption repair.
- Event append failure after file/index commit is reconciled on startup.

---

## 13. Git operations

### 13.1 Init

`git::init(root, local_device)`:

1. Create repo and working-tree directories.
2. Write `.gitattributes`:
   ```gitattributes
   * text eol=lf
   *.md merge=memory-frontmatter-merge
   events/*.jsonl merge=union
   substrate/**/*.jsonl merge=union
   tombstones/*.jsonl merge=union
   ```
3. Write `.gitignore` for `.DS_Store`, editor backups, Stream A temp files, and local crash markers.
4. Write synced `config.yaml` without device ID.
5. Create local `~/.memoryd/local-device.yaml` if missing.
6. Configure local git:
   - `merge.memory-frontmatter-merge.driver = <absolute-path-to-memory-merge-driver> --base %O --ours %A --theirs %B --path %P`
   - `merge.memory-frontmatter-merge.name = Semantic frontmatter merge for memory files`
   - `core.autocrlf = false`
   - `pull.rebase = false`
7. Create `events/<device-id>.jsonl` and commit tracked bootstrap files.

The driver command uses an absolute path or a stable shim path managed by installation. Ambient `PATH` is not sufficient for unattended merges.

### 13.2 Clone adoption

`git::adopt_clone(root)` is required after cloning the memory repo on a new device:

1. Generate or load local device ID from `~/.memoryd/local-device.yaml`.
2. If the local device ID matches an existing different machine's event log and no explicit migration flag is set, generate a new device ID.
3. Create `events/<new-device-id>.jsonl`.
4. Configure local merge driver and git settings.
5. Recreate untracked directories.
6. Run `git::preflight` and `tree::validate(StartupPreflight)`.
7. Commit the new empty event log if the repo policy tracks per-device event logs immediately.

### 13.3 Preflight before merge

Before any fetch+merge, Stream A checks:

- merge driver config exists;
- binary/shim exists and is executable;
- `.gitattributes` has expected rules;
- local device ID exists and has a corresponding event log;
- working tree has no unresolved conflict markers or invalid quarantine files.

Failure returns `GitError::PreflightFailed`; Stream B must surface a repair command and must not run a text merge.

### 13.3.1 Inspect-only fetch (chicken-and-egg escape)

If preflight fails because `.gitattributes` content or merge-driver config is stale and the user believes the remote ref carries a fix, Stream A exposes `git::fetch_inspect(opts) -> InspectReport`:

1. Run a subset of preflight that only checks for filesystem corruption (unresolved conflict markers, invalid quarantine files). Repository-config issues are **skipped**.
2. `git fetch origin` into the local refs without merging.
3. Diff `origin/main`'s `.gitattributes` and any `policies/` files against the working tree. Local merge-driver config is not versioned, so it is checked against the currently installed driver's expected absolute/shim path instead of against a remote file.
4. Return `InspectReport { remote_fixes: Vec<RepoConfigDelta>, still_required: Vec<PreflightFailure> }`.

Stream B uses the report to drive a guided repair: show the remote-side tracked-file fix, let the user accept it, apply via `git checkout origin/main -- .gitattributes` (or equivalent), run `git::configure_merge_driver` for local config drift, then re-run full preflight. `fetch_inspect` never merges and never modifies the working tree itself.

### 13.4 Auto-commit

Triggered by durable events, not by raw watcher events. Debounced default 30 seconds.

Commit steps:

1. `git status --porcelain=v1 -z`.
2. Group changed paths by namespace; parsing nested `projects/<a>/<b>/` must use path-to-memory metadata, not naive first two segments.
3. `git add -A` only inside repo root.
4. `git commit` with deterministic auto message and trailers: `memoryd-version`, `device`, `schema-version`.
5. Append `GitCommitted`.

### 13.5 Fetch + merge

1. Preflight.
2. `git fetch origin`.
3. Compute ahead/behind/diverged exactly.
4. If only ahead: no merge.
5. If behind or diverged: `git merge --no-ff origin/main`.
6. If git exits conflict due to true textual/unparseable conflicts, stop and surface.
7. Scan for valid `status: quarantined` memories and append `MergeQuarantined` events.
8. Run repo-level reconciliation: duplicate IDs, missing event log adoption, reference rewrites, cross-file validation.
9. Reindex changed/repaired paths.
10. Auto-commit any reconciliation changes (via §13.4 path; uses a distinct commit message prefix `reconciliation:` so the audit trail separates merge from repair).
11. Append `GitFetched`.

### 13.5.1 Startup reconciliation

`Substrate::open` runs reconciliation before accepting writes. The contract:

1. **Crash-recovery scan.** Look for the on-disk markers `~/.memoryd/startup-reconcile.required` (set by §8.3 when index/event commit fails after a durable file write) and `<memory-root>/.git/MERGE_HEAD` (incomplete merge). Either marker forces full reconciliation.
2. **Working-tree audit.** `git status --porcelain=v1 -z` is classified path by path:
   - files whose hashes match `pending_reconciliation_files` are resumed as prior repair work;
   - valid or mechanically repairable memory Markdown edits made while the daemon was down are ingested as human edits, reindexed, and scheduled for auto-commit;
   - invalid memory files, files with conflict markers, dirty `.gitattributes`, dirty policies, and unknown non-memory paths are copied to `~/.memoryd/quarantine/<startup-ts>/` and produce `OperatorRepairRequired`; substrate refuses writes only while such operator-required items remain.
3. **Vector reconciliation.** Run §10.2.1 startup pass.
4. **Event log recovery.** Truncate any trailing partial line per §12.3 and emit `EventLogRecovered` if needed.
5. **Pending index-after-commit reconciliation.** Replay `~/.memoryd/pending/index-ops.jsonl` idempotently before accepting writes; emit `PendingIndexReplayed` for each completed op.
6. **Pending event-after-commit reconciliation.** Walk `~/.memoryd/pending/events.jsonl` (events whose append failed after their write committed) and re-append idempotently; emit `PendingEventReplayed`.
7. **Index/file consistency check.** Compare all `memories.file_hash` values against current file hashes; mismatches enqueue a reindex. Sampling is allowed only for periodic health checks after startup, never for startup reconciliation.
8. **Auto-commit any post-merge reconciliation work that was never committed.** This is the path that fires when the daemon crashed in the §13.5 step-10 debounce window.
9. Emit `StartupReconciliationCompleted { phases_run, reindexed, vector_repairs, event_repairs, pending_index_replays, operator_action_required: bool }`.

Substrate must not return from `open` until startup reconciliation completes or returns an explicit operator-required error. There is no path where Stream B begins serving writes against an unreconciled substrate.

### 13.6 JSONL union merge rules

`merge=union` is allowed only because readers validate, dedupe by event ID, and quarantine malformed non-final lines. JSONL order in git is not semantically meaningful.

### 13.6.1 Two-clone convergence definition

Multi-device convergence claims (release-gate criterion §17.7.4 and the two-clone test) are only meaningful with a precise byte-level definition. A pair of clones `A` and `B` is **convergent** at git ref `R` iff:

1. `A` and `B` are both at commit `R` on the tracked sync branch (default `main`).
2. The working tree under each clone, when filtered to **canonical content**, is byte-identical between `A` and `B`.

Canonical content is the working tree minus:

- `.git/` (per-clone storage),
- `~/.memoryd/` and any other untracked runtime state (per-device),
- per-device event log files `events/<device-id>.jsonl` for **other** devices' IDs (each device only owns its own log file; the union of all such files across devices is the audit stream).

Equality is defined as:

- exact byte equality for tracked Markdown memory files (canonical YAML serializer guarantees byte stability per §6.12);
- exact byte equality for `.gitattributes`, `.gitignore`, `config.yaml`, and `policies/**`;
- **set equality** by event `id` for each `events/<device-id>.jsonl` file that both clones do contain (e.g. each clone has its own log plus copies of every other device's log via git sync); JSONL line order is not part of canonical equality;
- exact byte equality for `tombstones/**` and `substrate/**/*.jsonl` after the same set-by-id normalization;
- structural equality (not byte equality) for `_merge_diagnostics.add_add_alternates[]` arrays — both clones must contain the same alternates by stable ID.

The release-gate two-clone harness (`scripts/two-clone-convergence.sh`) runs:

1. Init two empty clones with distinct device IDs against a shared bare remote.
2. On each clone: write a deterministic fixture series, including writes that conflict at file-level and trigger semantic merge.
3. Push, fetch, merge from each direction.
4. Run the canonical-content equality check above and exit non-zero on any difference.
5. Re-run the canonical comparison after a second round of fetch/merge with no new writes; the result must be a fixed point (idempotence).

### 13.7 Acceptance signals

- Fresh clone without adoption fails preflight with a specific repair instruction.
- Fresh clone with adoption can perform a semantic same-file merge.
- Diverged local/remote branches merge; ahead-only branches do not incorrectly skip future behind state.
- Missing merge-driver binary refuses before merge.
- JSONL union duplicate lines replay idempotently.
- Two-clone convergence harness (§13.6.1) reaches a fixed point; canonical-content diff is empty after a second no-op fetch/merge round.

---

## 14. Frontmatter merge driver

### 14.1 Invocation and authority

Git invokes:

```text
memory-merge-driver --base <base> --ours <ours> --theirs <theirs> --path <pathname>
```

The merge driver is path-local. It may only write the merged content to `<ours>`. It must not create extra files, rename paths, or rewrite references in other files. Repo-level repairs happen after git merge.

### 14.2 Core algorithm

1. Parse base/ours/theirs. If base is absent for add/add, represent base as `None`.
2. **Schema-version gate.** If any side's `schema_version` exceeds the driver's supported version, exit `1` with stderr `merge-driver: schema_version=<n> exceeds supported=<m>; upgrade required`. Git surfaces the conflict; the user upgrades the driver before retrying. The driver never silently falls back to "unknown _extras" handling for a higher schema. The supported version is exposed as a single named constant `memory_substrate::merge::MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` (currently `1`); the merge-driver binary, the Rust merge library, and any test harness all read from this constant — there is no other source of truth.
3. If a side cannot parse YAML but frontmatter delimiters are identifiable, produce a valid quarantined file with the unparsable side preserved in `_merge_diagnostics.unparsed_sides[]` as raw base64 plus the parse error. If frontmatter delimiters are not identifiable, leave Git conflict markers and exit `1`.
4. Merge frontmatter using true 3-way field rules (§14.3): compare `base→ours` and `base→theirs`, not just `updated_at`.
5. Merge body with diff3 semantics.
6. If body conflicts, keep conflict markers in body, set `status: quarantined`, `trust_level: quarantined`, `review_state: pending`, and populate `_merge_diagnostics`.
7. Revalidate. If validation fails, try status-aware normalization where specified. If still invalid, quarantine with diagnostics.
8. Write canonical file to `<ours>`.
9. Exit `0` for clean merges and semantic quarantines represented as valid files. Exit `1` only when Git still needs to treat the path as unmerged because no valid file could be written, or for the schema-version gate above.

### 14.3 Generic 3-way rule

For each field, first classify changes:

| Case | Result |
| --- | --- |
| ours == theirs | that value |
| base == ours, base != theirs | theirs |
| base == theirs, base != ours | ours |
| base, ours, theirs all differ | field-specific conflict rule |
| base missing add/add | field-specific add/add rule |

`updated_at` is never used to discard an independent change on another field. It is only a tie-breaker inside a same-field conflict rule when the field's semantics allow newer-wins.

### 14.4 Field rules

| Field | Rule |
| --- | --- |
| `schema_version` | all sides must be supported; higher supported version wins only through migration, not merge |
| `id` | same-file updates must match; add/add with same ID quarantines path and lets repo-level duplicate repair preserve both files |
| `type`, `scope`, `canonical_namespace_id` | immutable; same-field conflict quarantines |
| `namespace` | immutable for project/org unless `.memory-project.yaml` migration context is present; conflict quarantines |
| `summary` | true 3-way; same-field conflict selects the side with later `updated_at` and preserves loser in diagnostics |
| `confidence` | true 3-way; same-field conflict selects the side with later `updated_at` unless values differ by >0.25, then quarantine |
| `trust_level` | lifecycle matrix aware; conflicts involving `quarantined` quarantine unless both sides agree |
| `sensitivity` | true 3-way (so a legitimate downgrade by one side survives if the other is unchanged from base); same-field 3-way conflict selects the maximum by order `personal > confidential > internal > public` and records the loser in `_merge_diagnostics`; `secret` is not a persisted value, so any side with `sensitivity: secret` causes the driver to exit `1` without writing a merged file |
| `status` | lifecycle merge table (§14.5) |
| `review_state`, `requires_user_confirmation` | true 3-way; same-field `approved` vs `rejected` quarantines; otherwise stricter state wins: pending > rejected > approved > null for review, true > false for confirmation |
| `tags`, `aliases` | normalized set union with deterministic sort |
| `entities` | union by `id`; label same-field conflict preserves newer label and loser in diagnostics |
| `evidence` | union by evidence `id`; fallback to `(quote_norm_hash, ref)`; near-duplicates preserved in diagnostics |
| `supersedes`, `superseded_by`, `related` | set union, then status-aware normalization and cross-field validation |
| `tombstone_events` | union by event `id`; if no IDs, by `(applied_at, actor, reason_hash)` |
| `created_at` | min |
| `updated_at` | max of merged changes; if merge itself changes diagnostics/quarantine, set to merge time and preserve previous max in diagnostics |
| `observed_at` | true 3-way; same-field conflict max if both are observations of same claim, otherwise diagnostics |
| `valid_from`, `valid_until`, `ttl` | true 3-way; same-field conflict quarantines unless one side only narrows validity window safely |
| `author` | true 3-way; conflict preserves primary by newer update and loser in diagnostics |
| `source` | true 3-way; conflict preserves both via diagnostics/evidence provenance |
| `retrieval_policy`, `write_policy` | recursive per-key true 3-way; stricter value wins for safety keys |
| `regression` | detection signature true 3-way; occurrences union by ID or per-device G-counter max; never raw-count sum |
| `prospective` | recursive true 3-way; terminal states ordered `cancelled/completed/expired` require diagnostics on conflict |
| `privacy_scan` | newest `ran_at` primary; differing models preserved in diagnostics |
| `_merge_diagnostics` | union diagnostics by ID/content hash |
| `abstraction` | true 3-way; same-field conflict selects the side with later `updated_at`; equal timestamps → **any non-null beats null; null vs null is a no-op; both non-null → lexicographically greater `sha256(NFC(value))` wins** (null/empty-after-trim are equivalent and normalize to null before comparison); loser preserved in `_merge_diagnostics`. (Ratified 2026-07-10 deviation from the originating plan's ours-wins rule: ours-wins is Git-side-dependent and violates two-clone convergence, §13.6.1) |
| `cues` | set union of both sides → NFC canonicalize → sort by the strict total order **`(case_fold(NFC(value)), NFC(value) bytes)`** where `case_fold` is **Unicode default (full) case folding — never locale-aware lowercasing** (`I`/`İ`, `ß` hazards) → dedup under case-fold equality keeping the first entry in that order (canonical casing = the byte-lexicographically smaller spelling; never insertion- or side-order) → keep first 3. Worked example: union `{OAuth, oauth}` → both fold to `oauth`, `O` < `o` byte-wise → keep `OAuth`; identical in both merge directions. Two-clone convergence fixtures required for opposite merge directions with overflowing unions **and with casing-only duplicates** |
| unknown `_extras` | per-key true 3-way; both non-null/add-add same-key conflict → lexicographically greater `sha256(canonical value)` wins (canonical value recursively sorts object keys and NFC-normalizes string values); equal values are a no-op; loser preserved in `_merge_diagnostics`. (Ratified 2026-07-18, Trey; supersedes the prior quarantine-as-written rule which was Git-side-dependent for add/add and violated two-clone convergence §13.6.1) |

### 14.5 Lifecycle/status merge table

Status is not a naive total order.

1. If either side is `tombstoned`, result is `tombstoned`; union tombstone events; clear `superseded_by`; preserve any displaced supersession info in diagnostics.
2. If either side is `quarantined`, result is `quarantined` unless the other side is `tombstoned`, which remains tombstoned but preserves quarantine diagnostics.
3. `pinned` beats `active`/`candidate` but not `tombstoned`.
4. `superseded` beats `active`/`candidate` only if `superseded_by` survives validation.
5. `archived` beats `active`/`candidate`, but `archived` vs `superseded` quarantines unless both sides include compatible lifecycle diagnostics explaining the transition.
6. `active` beats `candidate`.
7. Same-status conflicts merge dependent fields normally.

The table must have fixtures for every pair.

### 14.6 Add/add same path

If both sides created a different file at the same path and base is absent:

- If frontmatter IDs differ: produce a valid quarantined combined file at the contested path, with the primary side in normal frontmatter/body and every losing logical memory stored in structured `_merge_diagnostics.add_add_alternates[]` (§6.10). Exit `0` only if every original frontmatter and body is mechanically recoverable from the quarantined file or referenced artifact.
- If frontmatter IDs match: produce a valid quarantined file and emit diagnostics that duplicate-ID repair is required; do not invent invalid suffix IDs.

### 14.7 Quarantine marker

A semantic quarantine file has:

- `status: quarantined`;
- `trust_level: quarantined`;
- `review_state: pending`;
- `_merge_diagnostics.status: quarantined`;
- structured `_merge_diagnostics.add_add_alternates[]` when quarantine came from add/add same-path conflict;
- body conflict markers only if the body was the conflict;
- a short HTML comment at the end of body with human-readable reason.

It must validate. It must be preserved by future merges until resolved by admin command.

### 14.8 Acceptance signals

- Fixture matrix covers every lifecycle pair.
- Scalar independent edits both survive.
- Same-field scalar conflict follows rule or quarantines.
- Evidence whitespace near-duplicates are not duplicated silently or dropped.
- Regression counts merge via occurrence IDs/G-counter.
- Unknown fields use true 3-way per key and do not parent-`updated_at` stomp independent edits.
- Add/add collisions preserve both logical memories or quarantine validly.
- Add/add same-path quarantine fixture proves both original frontmatters and bodies can be recovered byte-for-byte from `_merge_diagnostics.add_add_alternates[]` or referenced artifacts.
- Quarantine output validates and exits `0` for semantic quarantine.
- **Sensitivity downgrade fixture.** base=`confidential`, ours=`internal` (intentional downgrade), theirs=`confidential` (no change) → result is `internal`. Inverse case must also pass. Same-field 3-way conflict (e.g. base=`internal`, ours=`personal`, theirs=`confidential`) resolves to `personal` with diagnostics.
- **Schema-version gate fixture.** A file with `schema_version: 2` against a v1 driver causes exit `1` with a `schema_version exceeds supported` stderr line; no merged file is written.
- Fuzzing never panics and never emits invalid YAML.

---

## 15. Configuration

### 15.1 Synced `config.yaml`

```yaml
schema_version: 1

paths:
  memory_root: ~/.memory
  runtime_root: ~/.memoryd

sync:
  remote: git@github.com:treygoff/memory.git
  enabled: true
  fetch_interval: 120
  push_interval: 300
  auto_push: true
  auto_commit_debounce: 30

embeddings:
  default_provider: local-gemma
  default_dimension: 768
  default_model_ref: embeddinggemma-300m-qat-Q8_0
  allow_multiple_models: true

privacy:
  filter_enabled: false
  encrypted_index_default: metadata_only

logging:
  level: info
```

No device ID lives here.

### 15.2 Local `~/.memoryd/local-device.yaml`

```yaml
schema_version: 1
device:
  id: dev_a1b2c3d4e5f60718
  name: trey-mbp-2025
  shard: a1b2c3d4e5f60718
paths:
  memory_root: ~/.memory
  runtime_root: ~/.memoryd
```

### 15.3 Environment overrides

Configuration precedence is:

1. explicit API/CLI roots passed in `Roots`;
2. environment overrides;
3. local `~/.memoryd/local-device.yaml`;
4. synced `config.yaml` defaults.

Synced `config.yaml` may contain default `paths`, but local roots always win on adopted devices. Env overrides are for tests/CI and local repair. They must not mutate synced config unless explicitly saved.

### 15.4 Acceptance signals

- Fresh clone has synced config but no local device config until adoption.
- Loading config never copies another machine's device ID from repo state.
- Env overrides are visible in loaded config but not serialized into `config.yaml` unless asked.
- A fixture where synced and local roots differ loads local roots and does not mutate synced config.

---

## 16. Public Rust API surface

### 16.1 Context object

All mutating APIs take explicit context; no hidden global `~/.memoryd` assumptions.

```rust
pub struct Substrate {
    roots: Roots,
    device: LocalDevice,
    index: Index,
    events: EventLog,
    git: Repo,
}

impl Substrate {
    pub async fn open(roots: Roots) -> Result<Self, OpenError>;
    pub async fn init(roots: Roots, opts: InitOptions) -> Result<Self, InitError>;
    pub async fn adopt_clone(roots: Roots, opts: AdoptOptions) -> Result<Self, AdoptError>;
    pub fn doctor(&self) -> DoctorReport;
}
```

### 16.2 Frontmatter/markdown

```rust
pub fn parse_frontmatter(yaml: &str) -> Result<FrontmatterRaw, ParseError>;
pub fn validate_frontmatter(raw: FrontmatterRaw) -> Result<Frontmatter, ValidationReport>;
pub fn serialize_frontmatter(fm: &Frontmatter) -> Result<String, SerializeError>;

impl Substrate {
    pub async fn read_memory(&self, id: &MemoryId) -> Result<MemoryEnvelope, ReadError>;
    pub async fn read_path(&self, path: &RepoPath) -> Result<MemoryEnvelope, ReadError>;
    pub async fn write_memory(&self, req: WriteRequest) -> Result<WriteOutcome, WriteFailure>;
    pub async fn write_encrypted(&self, req: EncryptedWriteRequest) -> Result<WriteOutcome, WriteFailure>;
    pub async fn update_encrypted_memory_metadata(&self, id: &MemoryId, actor: Option<&str>, mutate: impl FnOnce(&mut Memory)) -> Result<(), WriteFailure>;
    pub async fn tombstone_memory(&self, req: TombstoneRequest) -> Result<WriteOutcome, WriteFailure>;
}
```

`MemoryEnvelope` distinguishes content availability:

```rust
enum MemoryContent {
    Plaintext(String),
    Ciphertext { bytes: Vec<u8>, encryption: EncryptionEnvelope },
    MetadataOnly,
}
```

Stream A does not decrypt confidential/personal ciphertext. Stream D-mediated APIs may convert `Ciphertext` to a masked or plaintext projection outside this Stream A contract.

### 16.3 IDs

```rust
impl Substrate {
    pub async fn next_memory_id(&self) -> Result<MemoryId, IdError>;
}
pub fn parse_memory_id(s: &str) -> Result<MemoryId, IdError>;
```

### 16.4 Index

```rust
impl Substrate {
    pub async fn reindex(&self, opts: ReindexOptions) -> Result<ReindexReport, IndexError>;
    pub async fn query_memory(&self, query: MemoryQuery) -> Result<Vec<MemoryHit>, QueryError>;
    pub async fn query_recall_index(&self, query: RecallIndexQuery) -> Result<Vec<RecallIndexRow>, QueryError>;
    pub async fn query_recall_index_including_metadata_only(&self, query: RecallIndexQuery) -> Result<Vec<RecallIndexRow>, QueryError>;
    pub async fn query_chunks(&self, query: ChunkQuery) -> Result<Vec<ChunkHit>, QueryError>;
    pub async fn update_embedding(&self, req: EmbeddingUpdate) -> Result<(), VectorError>;
    pub async fn drop_embedding_model(&self, triple: EmbeddingTriple) -> Result<DropTripleReport, VectorError>;
}

pub struct EmbeddingTriple {
    pub provider: String,
    pub model_ref: String,
    pub dimension: u32,
}
```

`EmbeddingUpdate` includes `chunk_id`, `expected_content_hash`, `triple: EmbeddingTriple`, and `vector`. `update_embedding` returns `VectorError::StaleChunk` if the current chunk hash differs from `expected_content_hash`, `VectorError::DimensionMismatch` if `triple.dimension` disagrees with `vector.len()`, and `VectorError::UnknownEmbeddingTriple` if the triple has been dropped.

No raw mutable SQLite connection is public. A read-only debug query may be feature-gated:

```rust
#[cfg(feature = "admin-sql")]
pub fn query_readonly_unchecked<R>(&self, f: impl FnOnce(&ReadOnlyConnection) -> Result<R, QueryError>) -> Result<R, QueryError>;
```

### 16.5 Watcher/events/git

Async boundaries are explicit. Stream A itself may be synchronous internally, but Stream B must know what blocks.

```rust
impl Substrate {
    pub fn watch(&self) -> Result<WatchSubscription, WatchError>;
    pub async fn append_event(&self, event: Event) -> Result<EventAppendOutcome, EventError>;
    pub async fn read_events(&self, query: EventQuery) -> Result<impl Stream<Item = Result<Event, EventReadError>>, EventError>;
    pub async fn git_preflight(&self) -> Result<(), GitError>;
    pub async fn fetch_inspect(&self, opts: InspectOptions) -> Result<InspectReport, GitError>;
    pub async fn auto_commit(&self, opts: CommitOptions) -> Result<Option<CommitSha>, GitError>;
    pub async fn fetch_and_merge(&self, opts: FetchOptions) -> Result<FetchOutcome, GitError>;
    pub async fn push(&self, opts: PushOptions) -> Result<PushOutcome, GitError>;
    pub fn durability_tier(&self) -> DurabilityTier;
}

/// Owned subscription handle. `WatchSubscription` borrows nothing from
/// `Substrate`; `Substrate` may be dropped while the subscription is alive,
/// in which case the underlying watcher continues until the subscription
/// itself is dropped or `unsubscribe()` is called. Cancellation is explicit;
/// dropping the handle releases OS watcher resources synchronously.
pub struct WatchSubscription { /* opaque */ }

impl WatchSubscription {
    pub fn events(&mut self) -> impl Stream<Item = Result<FileEvent, WatchError>> + '_;
    pub fn unsubscribe(self);                            // explicit close
    pub fn rescan_now(&self) -> Result<(), WatchError>;  // forces a RescanRequired emission
}
```

All public `async` methods that perform filesystem, SQLite, git, vector, or network work must run blocking sections on Stream A's configured blocking executor or single index thread. The public API must not hide blocking/network behavior behind cheap-looking sync calls. Test-only synchronous helpers, if any, use a `_blocking` suffix.

### 16.6 Error taxonomy

Errors distinguish:

- validation/schema problems;
- stale write/concurrency problems;
- durability/fsync problems (including `DurabilityUnsupported` and `DurabilityUnavailable` per §3.1);
- index-after-commit failures (with durable pending index repair queue receipt);
- event-after-commit failures (with durable pending event repair queue receipt);
- repair-queue/repair-marker failures after durable commit (`RepairQueueFailed`, `RepairStateNotDurable`);
- git preflight/config failures (`PreflightFailed` distinct from `ConfigDeltaSinceLastInspect`);
- merge semantic quarantines;
- merge schema-version refusals (`SchemaVersionUnsupportedByMergeDriver`);
- vector adapter/model/dimension/stale-chunk failures including `VectorError::DimensionMismatch` and `VectorError::UnknownEmbeddingTriple` (with explicit pending embedding job receipt when applicable);
- classification-routing failures: `WriteFailureKind::SecretRefused`, `WriteFailureKind::EncryptionRequired`, `WriteFailureKind::ClassificationSensitivityMismatch` (§8.7);
- operator-required repair states (`OperatorRepairRequired` from §13.5.1 step 2);
- partial-sync cross-reference warnings.

No single `IoError` bucket may hide whether a write was committed before failure. `WriteOutcome` always reports `{ committed, indexed, event_recorded, durability, repair_required }`. `WriteFailure` always carries `outcome: WriteOutcome` so callers can distinguish recoverable committed states from non-committed failures.

### 16.7 Acceptance signals

- Public API has no extra exported mutable internals.
- Every error variant has at least one test.
- Async write/index/git/watch APIs can be cancelled without corrupting repo/index/event state; cancellation after a durable commit returns or records a committed outcome.
- Write outcomes clearly distinguish `not_committed`, `committed_not_indexed`, `committed_indexed_event_failed`, and `fully_committed`.

---

## 17. Test plan

### 17.1 Unit tests

- Frontmatter schema and canonical serialization.
- ID allocation boundaries and device mismatch.
- Path validation and case-folding.
- Markdown write CAS and failure classifications.
- Event log framing/checksum/recovery.
- Merge field rules.
- Query builders and index projections.

### 17.2 Property tests

- Frontmatter serialize/parse round-trip.
- Merge idempotence: merge(A,A,A) == A.
- Merge commutativity for symmetric fields.
- Merge preserves independent edits.
- Set serialization deterministic.
- Event replay idempotence under duplicate union lines.

### 17.3 Integration tests

- Init and clone adoption.
- Write/read/index/event/commit happy path.
- Human editor change via watcher.
- Stale-base programmatic write conflict.
- Multi-device divergent merge with semantic merge driver.
- Duplicate device ID repair.
- Sensitive encrypted write with no plaintext leakage.
- Reindex equivalence to incremental index.

### 17.4 Merge fixtures

At least 60 fixtures:

- independent scalar edits;
- same-field conflicts;
- all lifecycle pairs;
- evidence duplicate/near-duplicate;
- regression occurrence merge;
- supersession DAG cycle, missing-endpoint, and inverse-mismatch fixtures;
- tombstone vs supersede/archive;
- unknown field add/add conflict;
- privacy scan model mismatch;
- add/add same path;
- valid quarantine output.

### 17.5 Crash/durability tests

Use fault injection, not only kill timing:

- temp write short write;
- fsync temp failure;
- rename failure;
- parent fsync failure;
- index transaction failure after durable rename;
- event append failure after index;
- pending repair queue append failure after durable commit;
- crash during event append;
- startup reconciliation after each committed-but-incomplete state.

### 17.6 Performance tests

Benchmarks must record hardware, OS, filesystem, SQLite pragmas, and memory count. v1.1 treats performance as a **release gate**, because Stream E's recall-block budget assumes these p95s; missing them ships a Stream A that downstream streams cannot use.

Reference hardware for the gate: Apple Silicon laptop (M-series, 16 GB+) **and** Linux x86_64 runner (≥4 vCPU, NVMe-class storage). Both must produce a results JSON; see §17.7 for which is CI-enforced and which is manual evidence.

Gate targets (per profile):

- 10K-memory cold reindex p95 ≤ 60s.
- Query by ID p95 ≤ 10ms.
- Filtered metadata query p95 ≤ 50ms.
- FTS chunk query p95 ≤ 75ms.
- Vector chunk query p95 ≤ 100ms (sqlite-vec adapter, 768-dim vectors, 10K corpus).
- Tree validator p95 ≤ 500ms on 10K memory files.

#### Corpus and vector provenance

Stream A does not run an embedding model — Stream B owns inference. The 10K-corpus vectors used for the perf gate are therefore **deterministic synthetic vectors** generated by `memory-test-support::perf::synthetic_vectors(seed, dimension, n)`:

- Each vector is sampled from a fixed RNG seeded by `(seed, chunk_id, dimension)` so the corpus is reproducible across machines.
- Vectors are L2-normalized so sqlite-vec's distance distribution is realistic for ANN-indexed cosine search.
- The seed and the SHA256 of the materialized corpus are recorded in `bench/results.json` so a perf regression can be confirmed against an identical corpus.
- Synthetic vectors are sanctioned only for Stream A perf gates. Streams B/E that exercise real recall must use real embeddings.

#### Baseline and regression detection

The baseline lives at `bench/baseline.json`, checked into the repo. Regression detection:

- Each metric records `{ p50_ms, p95_ms, p99_ms, runs }` over `runs >= 9` repetitions.
- A run is **regressing** on a metric iff `current.p95_ms > 1.10 * baseline.p95_ms` AND the difference exceeds the baseline's per-metric `noise_floor_ms` (also stored in baseline.json, default 2 ms).
- Baseline is updated only by an explicit human-authored commit that touches `bench/baseline.json`; the bench harness never overwrites baseline automatically. A baseline update must include the prior baseline JSON in the commit message for audit.
- Per-profile baselines are stored as `bench/baseline.<profile>.json` where `<profile>` is one of `darwin-arm64` or `linux-x86_64`; cross-profile regression comparison is forbidden.
- A regression on any target on any profile blocks merge.

### 17.7 Overall acceptance

Stream A is done when every criterion below passes. Each criterion is tagged `[CI]` (must be enforced by an automated pipeline) or `[MANUAL]` (currently requires a human to run and attach JSON evidence to the release-review doc; may move to `[CI]` once the relevant runner is provisioned).

1. `[CI]` All section acceptance signals pass per the §17.8 coverage manifest.
2. `[CI]` `cargo test --workspace --release` passes on Linux x86_64. `[MANUAL]` Same command on macOS arm64; output captured in the release-review doc until a macOS arm64 CI runner exists.
3. `[CI]` Merge-driver fuzzing for 10 minutes produces no panics and no invalid output.
4. `[CI]` Two-clone multi-device scripted test (per §13.6.1) converges to a fixed point under canonical-content equality after semantic merges and repo-level reconciliation.
5. `[CI]` Public API docs (`cargo doc`) build without warnings and explain blocking/async behavior and every error outcome.
6. `[CI]` Performance gates from §17.6 pass on the Linux x86_64 profile, with `bench/results.linux-x86_64.json` written and compared against `bench/baseline.linux-x86_64.json`. `[MANUAL]` Same on macOS arm64, with `bench/results.darwin-arm64.json` attached to the release-review doc and compared against `bench/baseline.darwin-arm64.json`. Both profiles must be present and within the per-metric regression threshold before release.
7. `[CI]` Durability probe behaves per §3.1: `Full` succeeds on a tmpfs/ext4/apfs fixture; `Refused` on a fixture that monkey-patches parent-dir fsync to `EINVAL`; `BestEffort` on a fixture returning a documented non-fatal error.
8. `[CI]` Crash-injection matrix from §17.5 passes with startup reconciliation cleanly converging to a consistent state.
9. `[CI]` Spec acceptance coverage manifest (§17.8) is green: every "Acceptance signals" bullet in this spec maps to a named test in the codebase.
10. `[MANUAL]` Independent review has no blocking findings; the review is not itself a test, but a release gate.

A `[MANUAL]` criterion does not lower the bar — it just acknowledges that the verifying mechanism is human-attached evidence rather than a CI job. Each `[MANUAL]` item must be re-evaluated for `[CI]` promotion every release; the release-review doc records what's blocking promotion.

### 17.8 Spec acceptance coverage manifest

Spec acceptance signals must be mechanically traceable, not merely aspirational. Stream A ships a build-time test `spec_acceptance_signals_have_named_tests` that:

1. Parses every `### Acceptance signals` and `### N.M Acceptance signals` subsection of this spec from `docs/specs/stream-a-core-substrate-v1.2.md`.
2. For each bullet, requires a corresponding entry in `crates/memory-substrate/tests/spec_coverage_manifest.rs`'s `SPEC_COVERAGE: &[(&str, &str)]` table mapping `(spec_section, test_path::test_name)`.
3. Fails the build if any spec bullet has no manifest entry, or any manifest entry references a test that does not exist.
4. Composes with the analogous `every_spec_event_kind_has_typed_payload_fixture` (event kinds, §12.2) and `every_public_error_variant_has_behavioral_coverage` (error enums, §16.6) tests so that drift between spec and tests is detected at build time, not at review time.

Acceptance:

- Adding a new acceptance signal bullet to this spec without a corresponding manifest entry must fail CI.
- Removing a manifest entry without removing the spec bullet must fail CI.
- Renaming a test referenced in the manifest without updating the manifest must fail CI.

---

## 18. Risks and mitigations

### 18.1 Merge complexity

Risk: field rules compose badly and silently drop data.

Mitigation: true 3-way generic rule, status table fixtures, field-specific fixtures, property tests proving independent edits survive, and semantic quarantine that validates.

### 18.2 Human/editor races

Risk: daemon write overwrites a human edit or watcher suppression hides an external change.

Mitigation: CAS `expected_base_hash`, hash-based suppression, direct post-write indexing, watcher rescan on overflow.

### 18.3 Event/file/index non-atomicity

Risk: file durable but index/event fails.

Mitigation: explicit write outcomes, repair markers (`startup-reconcile.required`, `pending/index-ops.jsonl`, `pending/events.jsonl`), startup reconciliation (§13.5.1), and tests for every injected failure point in §17.5.

### 18.3a Vector store ↔ metadata drift

Risk: SQLite metadata commit succeeds, vector adapter call fails, leaving FTS-but-no-vector or vice-versa drift.

Mitigation: §10.2.1 contract — durable `pending_embedding_jobs`, stale-hash-checked `update_embedding`, startup reconciliation walks both directions, and dedicated event kinds (`EmbeddingJobEnqueued`, `VectorReconciled`) so the audit trail makes drift visible.

### 18.3b Durability degradation

Risk: silently acknowledging writes on a filesystem where parent-directory fsync is best-effort, then losing them on power loss.

Mitigation: §3.1 tiered probe at startup, `Refused` tier blocks `Substrate::open` by default, `BestEffort` tier requires per-write opt-in, every `WriteOutcome` carries the active tier so Stream B can refuse if policy demands `Full`.

### 18.4 Clone misconfiguration

Risk: missing merge driver or shared device ID causes text conflicts or corrupt per-device logs.

Mitigation: `adopt_clone`, preflight before merge, local device config outside repo, absolute driver path.

### 18.5 Vector model drift

Risk: one fixed vector dimension blocks model changes; or worse, a dimension change silently fails or corrupts an existing vector table.

Mitigation: vector adapter keyed by `(provider, model_ref, dimension)` and chunk content hash; per-triple vector tables; explicit migration semantics in §10.2.2; `VectorError::DimensionMismatch` and `VectorError::UnknownEmbeddingTriple` make every drift case a typed refusal rather than a silent fall-back.

### 18.6 Forward compatibility

Risk: v1.x required fields break v1.0 daemons.

Mitigation: `schema_version`, additive-only nullable changes without migration, read-only behavior for unsupported versions.

### 18.7 Classification escape

Risk: Stream A has no way to refuse a `secret` write because it cannot classify content itself; a buggy or absent Stream D would let secret material reach disk.

Mitigation: §8.7 makes `ClassificationOutcome` a required field on every write request, so the absence of Stream D is a typed compile/runtime error rather than a silent default to "trusted." Stream A enforces routing for every value (`Trusted`, `RequiresEncryption`, `Secret`) with three distinct typed `WriteFailureKind` variants, all of which have direct test coverage independent of Stream D being implemented.

### 18.8 Spec drift

Risk: spec acceptance signals drift away from the implementation as both evolve, and reviewers don't notice until release.

Mitigation: §17.8 build-time coverage manifest fails CI when any acceptance bullet has no test or any manifest test goes missing, alongside the existing event-kind and error-variant manifests.

### 18.9 Performance regression baseline gaming

Risk: a developer with commit access can mask a regression by quietly bumping the baseline.

Mitigation: §17.6 requires baseline updates to be authored by an explicit human-touched commit to `bench/baseline.<profile>.json` that includes the prior baseline JSON in the commit message; the bench harness never overwrites baselines automatically. Reviewers can see baseline movement in git history.

---

## 19. Implementation phasing

1. **Schema/tree/config/IDs.** Frontmatter, local-vs-synced config, device-sharded IDs, validators.
2. **Durable Markdown/event transaction.** Atomic writes, CAS, event framing, startup reconciliation.
3. **SQLite index.** Metadata tables, chunks, FTS, vector adapter contract, query helpers.
4. **Watcher integration.** Hash-based suppression and reindex convergence.
5. **Git init/adopt/preflight/commit/fetch.** Merge-driver config and clone workflows.
6. **Merge driver.** True 3-way rules and fixtures.
7. **End-to-end fault and multi-device tests.**

Phases 3 and 6 can parallelize after Phase 1 if the frontmatter structs are stable. Git fetch/merge should not ship before clone adoption and merge-driver preflight are done.

---

## 20. Locked implementation decisions

The following decisions are locked for v1.2. Later streams may add adapters or policy layers without changing the Stream A substrate contract.

1. **Vector adapter:** sqlite-vec is the default v1.x adapter. DDL is adapter-generated and version-pinned. Additional adapters may ship later behind the same `VectorStore` trait.
2. **Embedding model default:** local 768-dim `embeddinggemma-300m-qat-Q8_0` is the default model. The schema supports multiple provider/model/dimension pairs; switching the active triple is a configuration migration governed by §10.2.2, not a schema break.
3. **Encrypted index projection default:** confidential/personal writes default to `metadata_only`. Stream D may supply `masked_summary_and_entities` or richer safe projections per write, but Stream A never invents them.
4. **Tombstone retention:** normal forget uses frontmatter tombstones and tombstone events; git history retention is expected in v1.x. True privacy erasure is an explicit admin/history-rewrite runbook outside normal substrate writes.
5. **Semantic quarantine git exit:** valid semantic quarantine exits `0`; unrepresentable conflicts and unsupported `schema_version` exit `1`.
6. **Performance reference hardware:** both reference profiles must produce a results JSON: Apple Silicon laptop (M-series, 16 GB+) and Linux x86_64 runner (>=4 vCPU, NVMe-class storage). Linux is `[CI]`; macOS arm64 is `[MANUAL]` until a CI runner exists (§17.7).
7. **Durability tier default:** `Full` is required by default. `BestEffort` requires per-write opt-in. `Refused` blocks `Substrate::open` except for explicit test/CI force-open.
8. **Classification contract:** every `WriteRequest`/`EncryptedWriteRequest` carries a typed `ClassificationOutcome`; Stream A enforces routing/refusal from it (§8.7). Classification is never inferred and never persisted to frontmatter.
9. **Embedding triple identity:** `(provider, model_ref, dimension)` is the unit of vector-table identity. Mismatch never silently downgrades; old triples remain queryable until explicitly dropped (§10.2.2).
10. **Merge-driver supported version:** the merge driver and library both read `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` (currently `1`) as the single source of truth for the schema gate (§14.2).
11. **Convergence definition:** two-clone convergence is canonical-content equality per §13.6.1, not raw working-tree diff.
12. **Performance baseline:** `bench/baseline.<profile>.json` is checked in and updated only by explicit human-authored commits; the bench harness never overwrites baselines automatically (§17.6).
13. **Synthetic perf vectors:** `memory-test-support::perf::synthetic_vectors` is the sanctioned source of vector content for Stream A perf gates; real embedding-model integration is Stream B's responsibility (§17.6).
14. **Spec coverage enforcement:** the `spec_acceptance_signals_have_named_tests` build-time test is part of the release gate; spec drift fails CI (§17.8).

---

## 21. What this spec deliberately does not decide

- Stream C promotion policy thresholds.
- Stream D Privacy Filter model details and encryption key management. Stream A's contract with Stream D is the `ClassificationOutcome` enum (§8.7); the model that produces it is Stream D's concern.
- Stream E final ranking formula and context-budget policy.
- Stream F dreaming logic.
- UI/CLI workflows for human conflict resolution.
- Multi-user ACLs beyond single-user future-proofing fields.

The spec also deliberately does **not** legislate the following implementation-process concerns. They belong in the implementation plan (`docs/plans/<date>-stream-a-core-substrate-implementation-plan.md`) rather than the substrate contract:

- Branch / worktree strategy for parallel agent execution (single working tree vs. branch-per-task vs. worktree-per-agent).
- `Cargo.lock` ownership across parallel tasks that add dependencies.
- Definitions of script flags such as `scripts/two-clone-convergence.sh --smoke|--full` and `scripts/bench-gate.sh --tier smoke|release` (the spec defines convergence and the perf gate; the script's flag surface is the plan's job).
- The contents and invariants of `scripts/rust-boundary-check.sh` standing in for Specgate's missing Rust support.
- Pinning of the agentlinters checkout SHA used at scaffold time.
- The `cargo fuzz` toolchain pin (must be compatible with the dylint nightly pin chosen in the plan).
- Ownership of the `memory-test-support` crate after scaffold.
- macOS arm64 CI runner provisioning (the spec acknowledges its absence in §17.7; provisioning is operational).

---

## Amendments

Dated additive clarifications that do not change a required behavior carry no version bump (per the repo's spec/plan conventions). A behavior change requires explicit Trey authorization. Each amendment records the date, the sections it touches, and the rationale.

### 2026-07-18 — `_extras` deterministic value-hash merge tie-break

**Trey-authorized behavior-change amendment.** Authorized 2026-07-18, in-version per explicit authorization. **Touches:** §14.4 unknown `_extras` field rule. **Rationale:** replace the prior quarantine-as-written add/add rule with the same side-independent value-hash resolution pattern used for `abstraction` and `summary`; the prior rule was Git-side-dependent in practice and violated two-clone convergence (§13.6.1).

### 2026-07-15 — B3 abstraction/cue metadata amendment

**Trey-authorized behavior-change amendment.** Authorized 2026-07-15, in-version per explicit authorization; it replaces the shipped `abstraction_compile` apply behavior for this bounded path. **Touches:** §8.7 (generation-context carve-out), §12 (`MetadataAmended` event), `docs/specs/stream-f-dreaming-v0.3.md` (compile apply path), and `docs/api/memoryd-cli-contract-v1.md` §8 (report contract). **Rationale:** imported memories need retrieval metadata without a body/evidence rewrite or the supersede grounding gate.

`metadata_amend` is an internal, validator-gated operation with fixed request shape `{ id, expected_base_hash, abstraction, cues }`. It is not a general metadata patch API. At the compile-to-handler boundary, the actor is hardcoded to `memoryd-abstraction-compile`; MCP callers, `write`, `supersede`, and a free-form operator patch command cannot reach it. Only `abstraction` and `cues` may change. The fresh canonical read supplies namespace, storage form, and immutable state. The body, id, lifecycle/status, namespace and canonical binding, sensitivity, evidence/provenance, encryption envelope, and path remain unchanged; `created_at` remains unchanged and `updated_at` advances on a changed amendment.

The operation normalizes and validates the proposed fields with `memory_substrate::frontmatter::{normalize_abstraction_cues, validate_frontmatter}` and the §9.1 caps. It uses `handlers/governance/privacy.rs::classify_plaintext_memory`, extended for this operation to scan proposed abstraction/cues always and stored plaintext body, summary, and tags only when those plaintext fields are available. It derives `PrivacyNamespace` from the stored scope, including `User` to `Me`. It never decrypts solely to scan: encrypted body content is outside this scan by construction. `Secret` refuses before disk effects and corresponds to `WriteFailureKind::SecretRefused`. A classification that needs a tier higher than the stored memory's sensitivity/storage posture returns `MetadataAmendmentTierIncreaseRefused`; the operation does not re-tier, encrypt, drop generated fields, or use §8.7's drop/rebind behavior.

The handler compares `expected_base_hash` before its idempotent short-circuit. A mismatch is `MetadataAmendmentStaleBase`, even when the requested canonical fields already match. A matching unchanged amendment returns `changed: false` with no event, index work, or git work. A changed amendment uses a dedicated thin amend write: CAS-write the canonical file, update the index and §10.2.1 auxiliary fence, and append exactly one `MetadataAmended { id, path, actor, changed_fields }` event, where `changed_fields` is the exact nonempty subset of `{abstraction, cues}`. The F1 commit worker handles it as a durable canonical write.

Plaintext and encrypted rows use that same amend contract. `update_encrypted_memory_metadata` retains its `(id, actor, mutate)` signature; the worker-hash comparison occurs inside `mutate`, so its fresh-read CAS protects the check while ciphertext and its envelope remain intact. The handler appends `MetadataAmended` only after the mutation succeeds.

The handler refuses unless the stored status is `active` or `pinned`; otherwise it returns `MetadataAmendmentLifecycleNotAmendable`. The closed refusal set is `MetadataAmendmentStaleBase`, `MetadataAmendmentTierIncreaseRefused`, `MetadataAmendmentValidationFailed`, `MetadataAmendmentMissingId`, `MetadataAmendmentActorMismatch`, `SecretRefused`, and `MetadataAmendmentLifecycleNotAmendable`. The arm does not re-run grounding because it cannot change body or evidence, and it does not create a superseding version. The canonical diff, event, index hashes, and F1 git history are its audit trail.

### 2026-06-09 — Default active-embedding triple updated to the shipped production model

**Touches:** §10.2.2 #2 (active triple), §10.2.2 final paragraph ("default active triple ... locked by §20"), §20 #2 ("embedding model default"), §20 #13 (synthetic perf vectors). **Approved by Trey 2026-06-09** ("adapt ur spec and plan accordingly"); see `docs/plans/2026-06-09-dynamics-eval-hardening.md` Task 3.0 and `docs/reference/2026-06-09-embedding-model-research.md`.

When this spec was written, Stream A could not run a real embedding model — inference was Stream B's responsibility — so the substrate shipped a *synthetic* default triple (`synthetic / stream-a-test / 32`) in `bootstrap_repo_tree`, and §20 named `local-gemma / embeddinggemma-300m-qat-Q8_0 / 768` as the intended production default. Stream B has now shipped production embedding inference (Task 3.0). The bootstrapped default active triple is therefore:

```
provider:  fastembed-candle
model_ref: Qwen/Qwen3-Embedding-0.6B
dimension: 1024
```

Qwen3-Embedding-0.6B (Apache 2.0, ungated, 1024-dim, 32K context) was selected over EmbeddingGemma-300m and other candidates by a golden-corpus bench on Memorum's real failure modes (trap-rate and abstention calibration). The model is served locally via the fastembed candle backend (`Qwen3TextEmbedding`) with Metal GPU offload and a CPU fallback; weights are downloaded on first use into `<runtime_root>/models` and are never bundled.

This is contract-touching under invariant 3 (§10.2.2 #9): provider, model_ref, **and** dimension all change versus the prior literal default (768 → 1024). It does **not** change any required *behavior* of the substrate — the triple remains opaque to Stream A, vector-table identity and the no-silent-fallback rule are unchanged, and any clone carrying an older triple in its synced `config.yaml` keeps that triple (the new default is written only when `config.yaml` is absent at bootstrap). Hence a dated amendment rather than a version bump.

The synthetic triple and `memory-test-support::perf::synthetic_vectors` (§20 #13) remain the sanctioned source of vector content for Stream A perf gates and tests; only the *bootstrapped production default* changed.

**Spec-honesty note (§10.1 "hybrid keyword + vector"):** until Task 3.0, no production consumer wrote vectors, so production retrieval was FTS-only bm25 in practice despite the hybrid description. With the embedding worker shipped, the hybrid description now reflects production reality once the active-triple vector table is populated.

**Correction 2026-06-10 (hybrid recall production status):** the final sentence above overclaimed. Production writes now populate active-triple vectors, and governance contradiction detection consumes those vectors through KNN, but no production recall handler embeds recall queries or passes `vector` into `ChunkQuery`. Production retrieval remains FTS-only bm25 today; hybrid keyword+vector recall is still future work.

### 2026-06-10 — `Substrate::query_hybrid_chunks` recall-membership hybrid query surface

**Touches:** §10.4 (query contract — adds the named hybrid-assembly helper the section already promises). **Approved as part of** `docs/plans/2026-06-10-vector-recall-fusion.md` (Wave 0 / S2; Stream E v0.6 fusion arc). This is a **dated additive amendment, not a version bump:** it adds one new public read-only query method that assembles per-hit `score_breakdown` inputs and performs no final policy ranking — exactly the role §10.4 reserves for Stream A. It adds no new required behavior to existing surfaces, removes/renames nothing, and the new surface is allowed in-version per the repo's spec/plan conventions and §10.4.

**Why it exists.** §10.4 already lists "hybrid result assembly with per-hit `score_breakdown` inputs, not final policy ranking" as a Stream A responsibility, but no concrete method exposed it. Stream E v0.6 delta recall and `memory_search` need a recall-membership-respecting hybrid query that runs the bm25 FTS lane and the sqlite-vec KNN lane over the *same* recall-filtered candidate set and hands both per-lane rank inputs back to the caller, who applies the fusion policy (RRF) — Stream A assembles, Stream E ranks.

**New surface:**

```rust
pub async fn query_hybrid_chunks(
    &self,
    text: &str,
    triple: &EmbeddingTriple,
    vector: &[f32],
    limit: usize,
) -> Result<Vec<HybridCandidate>, VectorError>;
```

returning **per-MEMORY** candidates (one row per memory, never per chunk), each carrying:

```rust
pub struct HybridCandidate {
    pub memory_id: MemoryId,
    pub score_breakdown: HybridScoreBreakdown,
}

pub struct HybridScoreBreakdown {
    /// 0-based rank of this memory in the bm25 FTS lane; `None` when the memory
    /// had no FTS hit. Lower rank = better.
    pub bm25_rank: Option<usize>,
    /// Cosine similarity (from the stored L2 distance under the unit-vector
    /// assumption, `cosine_from_l2_distance`) for the memory's nearest chunk;
    /// `None` when the memory had no embedded chunk in the vector lane.
    /// Explanation/trust-artifact display only — the caller's RRF fusion ranks
    /// by ordinal rank, not by this value.
    pub cosine_similarity: Option<f32>,
}
```

The exact struct/field names are the implementer's to finalize; the contract is the shape — one row per memory, carrying a bm25-lane rank input and a cosine-similarity explanation value, each `Option` so a memory may appear in one lane, the other, or both. (This is distinct from the existing per-chunk `ScoreBreakdown { fts, vector, distance }` on `ChunkHit`/`MemoryHit`; the hybrid surface returns the per-memory rank-input shape above.)

**Contract:**

- **Both-or-neither, never the silent FTS fallthrough.** The surface takes the FTS `text` lane **and** the `(triple, vector)` vector lane together — both present, or (degenerately) neither. It must **never** silently run vector-without-triple as an FTS-only query the way `query_chunks` does today. Triple is identity (§10.2.2 #6/#9); a triple mismatch is a typed error, never a silent fallback.
- **Recall membership filter, in both lanes.** Both lanes apply the recall exclusion contract: `metadata_only = 0 AND passive_recall = 1`, **and** exclude superseded and tombstoned memories. This is **explicitly distinct from `knn_active_memories`**, which deliberately **omits** the `passive_recall = 1` filter for write-governance semantics — so `knn_active_memories` is **not** reusable for recall; `query_hybrid_chunks` carries its own passive-recall-respecting query.
- **Chunk→memory collapse.** Both lanes over-fetch at the chunk level (the same `CHUNK_FANOUT` collapse `knn_active_memories` performs) and collapse to one row per memory: **best bm25** for the FTS lane, **minimum L2 distance** (nearest chunk) for the vector lane. No duplicate memory ids in the output.
- **Partial-vector-coverage tolerance.** Pending embedding jobs mean some chunks lack vectors (§10.5). A memory with a bm25 hit but no embedded chunk appears with `cosine_similarity: None` / `bm25_rank: Some(_)`; a vector-only hit appears with `bm25_rank: None` / `cosine_similarity: Some(_)`. Unembedded chunks simply contribute no vector rank; this is normal, not an error.
- **`UnknownEmbeddingTriple` contract — vec-table-absent is an error, never a silent empty.** When the active triple's vector table is absent or dropped, the surface returns `Err(VectorError::UnknownEmbeddingTriple(..))` — mirroring `query_vector_chunks` at `index/query.rs:398-400` — **never** a silently empty result vector. The recall caller (Stream E) catches this `Err` and maps it to its `no_vector_table` degrade marker (FTS-only). A *present* table with some chunks unembedded is the partial-coverage case above (tolerated), not this error.

**Scope of Stream A's role:** `query_hybrid_chunks` is **hybrid result assembly with per-hit `score_breakdown` inputs**, NOT final policy ranking. The Reciprocal Rank Fusion (RRF) policy, the seven-rung degradation ladder, and the query embedding (`embed_query`) all live in Stream E / memoryd — see Stream E v0.6 §16. Stream A runs the two lanes and returns the rank inputs; the caller fuses.

### 2026-07-09 — `gemini-api` embedding provider + API-lane privacy fence

**Touches:** §10.2.2 (registered provider strings), §20 #2 (embedding default unchanged — this adds an opt-in alternative), config.yaml key registry. **Approved by Trey 2026-07-09** ("spec amendment approved"); staged text in `docs/plans/2026-07-09-t33-spec-amendment-draft.md`, implementation plan `docs/plans/2026-07-09-api-embedding-lane.md`. No version bump: the change is additive surface (a second registered provider string, two additive config keys, an additive eligibility parameter); triple identity, no-silent-fallback, and all §10.2.2 behavior are unchanged.

1. **New provider string.** `gemini-api` joins `fastembed-candle` as a registered `provider` value in the embedding triple. Default API triple: `("gemini-api", "gemini-embedding-2", 768)` (dimension subject to the T4.1 bake-off; the triple literal, not this amendment, changes if it moves). Triple identity and typed-mismatch rules (§10.2.2 #6/#9) apply unchanged.
2. **Plaintext-eligibility fence (Stream A surface).** Embedding job fetch/count/reconcile surfaces take an `EmbeddingLaneEligibility` parameter: `AllTiers` (local providers) or `PlaintextOnly` (API providers). `PlaintextOnly` restricts to persisted sensitivity `public`/`internal` — `confidential`/`personal` rows (including masked `safe_body` projections, which keep their source tier) are never fetched for embedding and are reported separately as held-local jobs. Fail-closed: unknown tiers are held.
3. **Consent key.** Synced `config.yaml` gains optional `api_embedding_consent: bool` (absent = false). The daemon MUST NOT start an API embedding provider unless it is `true`; the CLI consent ceremony is the only writer. Unknown-key tolerance in `SyncedConfig` loading is load-bearing and now contractual.
4. **Credentials.** API keys live in device-local runtime state (env `MEMORUM_GEMINI_API_KEY` or 0600 key file), never in synced config — same rationale as device IDs (invariant 4).
5. **Non-goals.** No hot lane-swap (restart required); no cross-triple vector reuse; old triple tables remain until explicitly dropped (§10.2.2 unchanged).

---

*End of Stream A — Core Substrate Spec v1.2.*
