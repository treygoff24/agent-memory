# Stream A — Core Substrate Spec (v0.2)

**Status:** implementation-locked spec, 2026-04-24. Supersedes `stream-a-core-substrate-v0.1.md` after a second adversarial pass by Claude focused on latent-correctness and contract-ambiguity gaps the v0.1 revision still carried.

**Parent:** `docs/specs/system-v0.1.md`. Stream A implements the canonical storage/index/event/git substrate that every later stream depends on.

**Revision goal (v0.1 → v0.2):** close the remaining bugs and underspecified contracts in v0.1.

1. SQLite chunk index must survive `VACUUM` without silently breaking FTS lookups.
2. `sensitivity` merges must use true 3-way semantics so legitimate downgrades are not silently reverted.
3. The validator must accept human-edited files that omit known nullable keys (auto-populated, warned), while still rejecting wrong types, bad enums, and missing required scalars.
4. Durability degradation (no parent-directory fsync) must have a defined operational behavior, not just a `DoctorReport` line item.
5. Performance targets must gate release, since Stream E's recall-block budget depends on them.
6. Preflight must not block fetching a fix to its own preconditions; an inspect-only fetch path is required.
7. Vector store ↔ metadata consistency must have a startup reconciliation contract, not just a "logical operation" claim.
8. `author` must be structured, like `source`.
9. Startup reconciliation behavior with uncommitted post-merge repair changes must be explicit.
10. The merge driver must refuse files with unsupported `schema_version`, exit `1`, and let git surface the conflict.
11. The watcher subscription must have an explicit handle/lifetime contract.
12. Stream E must have a defined fallback when a confidential memory is metadata-only indexed.

---

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

- macOS 14+ and Linux kernel 5.10+. Windows is explicitly out of scope for v0.1; atomic-rename and fsync semantics on NTFS require a separate write strategy.
- POSIX `rename(2)` atomicity only **within the same filesystem**.
- Git 2.40+.
- SQLite 3.45+ with JSON1 and FTS5.
- Filesystem case-sensitivity is not assumed. All path uniqueness checks compare both exact bytes and case-folded relative paths.

### 3.1 Durability tiers

Parent-directory fsync support is the gating signal for full write durability. Stream A probes for it at startup and pins a tier; behavior depends on the tier:

| Tier | Probe result | Behavior |
| --- | --- | --- |
| `Full` | `fsync(parent_dir_fd)` succeeds on the memory root and on `events/` | Default. Writes acknowledged only after the §8.3 sequence completes. |
| `BestEffort` | parent-dir fsync returns a documented non-fatal error (e.g. older glibc on certain remote filesystems) | Writes acknowledged with `WriteOutcome.durability = BestEffort`. Callers must explicitly opt in via `WriteRequest.allow_best_effort_durability = true`; otherwise `write_memory` returns `WriteError::DurabilityUnavailable`. |
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
├── index.sqlite
├── index.sqlite-wal
├── index.sqlite-shm
├── socket                               # Stream B
├── pid                                  # Stream B
├── logs/memoryd.log
└── tmp/                                 # non-canonical scratch only; not used for atomic final renames
```

`config.yaml` in the repo contains portable configuration. `local-device.yaml` contains device identity and any local-only overrides. A fresh clone must never inherit the previous machine's device identity.

### 5.3 Path constraints

- `<memory-id>` matches `^mem_\d{8}_[0-9a-f]{4}_\d{6}$`.
- The four-hex shard is derived from the local `device_id` and is stable for that device.
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
| `id` | string | `^mem_\d{8}_[0-9a-f]{4}_\d{6}$` |
| `type` | enum | `project`, `person`, `procedure`, `episode`, `claim`, `artifact`, `prospective`, `pattern`, `playbook`, `postmortem`, `anti-pattern`, `heuristic`, `regression`, `correction`, `invariant`, `decision`, `open-question` |
| `scope` | enum | `user`, `project`, `org`, `agent`, `subagent` |
| `summary` | string | 1-280 chars |
| `confidence` | float | 0.0 <= x <= 1.0; no implicit default |
| `trust_level` | enum | `trusted`, `untrusted`, `candidate`, `quarantined`, `pinned` |
| `sensitivity` | enum | `public`, `internal`, `confidential`, `secret`, `personal` |
| `status` | enum | `candidate`, `active`, `pinned`, `superseded`, `archived`, `tombstoned`, `quarantined` |
| `created_at` | datetime | RFC3339 UTC `Z` |
| `updated_at` | datetime | RFC3339 UTC `Z`, >= `created_at` |
| `author` | object | structured principal, see §6.4 |

### 6.2 Known nullable/collection fields

Every memory's canonical serialization contains every key in this table. The serializer always emits them. The **parser** is permissive: when a known nullable/collection key is absent on read, the parser materializes the typed default (`null` for nullable scalars, `[]` for arrays, an object with all-null leaves for nested objects) and emits `ValidationWarning::AutoPopulatedNullableField { field }`. The validator does **not** fail on missing nullable keys; this preserves human-edit affordance while keeping round-trip output canonical.

Wrong types, bad enums, missing required *scalar* fields (§6.1), and unknown fields under a higher-than-supported `schema_version` remain hard errors.

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

Unknown future fields are preserved in `_extras` by the parser and re-emitted after known fields. Unknown fields produce warnings, not errors, unless `schema_version` is higher than the implementation supports and the field is declared required by that schema.

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
  harness: claude-code | codex | cursor | cli | null
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

```yaml
source:
  kind: user | agent-primary | agent-subagent | tool | web | email | file | synthesis | import | system
  ref: string | null
  harness: claude-code | codex | cursor | cli | null
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

Stream A does not schedule prospective memories, but it must be able to store and validate them without forcing a v0.2 schema break.

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
  lifecycle_notes: array
  human_reason: string
```

It is preserved through future merges by unioning array fields by stable IDs or normalized content hashes. It may be cleared only by a human/admin resolution command that emits an event.

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
10. `sensitivity == secret` is refused by write APIs; existing secret-tier files found during manual edits are validation errors.
11. `type == regression` requires `regression` and at least one detection signature.
12. `type == prospective` requires `prospective`.
13. `privacy_scan.labels` must include `private_credential` if `sensitivity == secret`; Stream D owns classification, but Stream A validates consistency when scan data exists.

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
- Unknown v0.2 fields parse, warn, preserve, and reserialize.
- Supersession cycle fixtures fail cross-file validation.

---

## 7. ID generation

### 7.1 Format

`mem_YYYYMMDD_<device-shard>_<seq>`.

Example: `mem_20260424_a1b2_000087`.

- `YYYYMMDD` is UTC date at mint time.
- `device-shard` is the first four lowercase hex chars of SHA256(local `device_id`).
- `seq` is a six-digit per-device daily sequence from `000001` to `999999`.

This preserves human sortability while removing the broken random-offset collision strategy.

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
3. If date changed, set `date=today_utc`, `next=1`.
4. If `next > 999999`, return `IdError::SequenceExhausted { date }`.
5. Return ID with current seq, increment `next`, fsync file, fsync parent directory, release lock.

### 7.3 Duplicate-ID recovery

Duplicate IDs should happen only if a repo was cloned/copied without adoption or if a bug reused a device ID. The path-local merge driver does **not** repair duplicate IDs.

Repo-level reconciliation (`git::repair_duplicate_ids`) runs after fetch/merge and during startup validation:

1. Detect duplicate frontmatter IDs across paths.
2. Select canonical survivor by earliest `(created_at, git commit timestamp, device_id, path)`.
3. For each non-survivor, mint a new valid ID using the current local device shard and next sequence.
4. Rename ID-based files to the new ID path; slug-based files keep path unless path collision exists.
5. Rewrite references in files changed in the same reconciliation transaction: `supersedes`, `superseded_by`, `related`, evidence refs that explicitly point to memory IDs, and known sidecars.
6. Emit `DuplicateIdRepaired` events with old/new IDs and affected paths.
7. Reindex affected files.

If references cannot be rewritten safely, quarantine the affected files with valid `status: quarantined` and do not silently drop either memory.

### 7.4 Acceptance signals

- 10,000 sequential IDs on one device are unique and monotonic by sequence.
- Two devices with different device IDs mint 50,000 IDs each for the same UTC day with zero collisions.
- Sequence `999999` succeeds; `1000000` returns `SequenceExhausted`.
- A fixture with duplicate IDs from a copied device is repaired into valid IDs matching the regex, with references rewritten or quarantined.

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
    memory: Memory,
    expected_base_hash: Option<Sha256>,
    write_mode: WriteMode,          // CreateNew | ReplaceExisting | AdminRepair
    index_projection: Option<IndexProjection>,
    event_context: EventContext,
}
```

- `CreateNew` fails if final path already exists.
- `ReplaceExisting` fails with `WriteError::StaleBase` if `expected_base_hash` does not match current file hash.
- `AdminRepair` bypasses CAS only for explicit repair operations and emits an admin repair event.

### 8.3 Atomic write sequence

For plaintext indexable writes:

1. Validate frontmatter and path.
2. Serialize canonical YAML + body.
3. Compute final path and ensure target parent exists.
4. Create temp file in the **same directory as the final file**: `<parent>/.<basename>.<op_id>.tmp` with `O_CREAT|O_EXCL`.
5. Write full buffer with short-write retry loop.
6. `fsync(temp_fd)`.
7. `rename(temp, final)` within the same directory.
8. `fsync(parent_dir_fd)` if `DurabilityTier == Full`; skip with explicit best-effort flag if `BestEffort` (see §3.1).
9. Apply SQLite index transaction directly for this operation.
10. Append and fsync event log entry.
11. Record operation ID in watcher suppression ledger.
12. Return success with `WriteOutcome.durability` set to the active tier.

If steps 4-8 fail, remove temp file if present and return error. If step 9 fails after the file is durable, write a `~/.memoryd/startup-reconcile.required` marker (referenced by §13.5.1), enqueue the missing index entry into `pending_index_ops`, and return `WriteError::IndexAfterCommitFailed`. If step 10 fails after file/index durability, append the missing event to `pending_events`, return `WriteOutcome { committed: true, event_recorded: false, durability }` plus `WriteErrorKind::EventAfterCommitFailed` in the outcome; callers must not blindly retry the file write.

### 8.4 Sensitive/encrypted writes

Stream A never writes plaintext sensitive content to repo paths.

For `sensitivity in {confidential, personal}`:

1. Stream D classifies and encrypts content or produces an approved masked projection.
2. Stream D calls Stream A with `EncryptedWriteRequest { metadata_frontmatter, ciphertext, safe_index_projection }`.
3. Stream A validates metadata, writes ciphertext atomically under `encrypted/<original-relative-path>`, indexes only the safe projection, and emits events.
4. If no safe projection exists, the memory remains retrievable by ID/path metadata only; body FTS and embeddings are disabled.

`secret` is refused and must not touch disk.

### 8.5 Delete/tombstone

Hard delete is admin-only. Normal forget operations write a tombstone event and update frontmatter to `status: tombstoned` with `tombstone_events[]`; they do not erase git history. Privacy leak runbooks live in Stream D/G.

### 8.6 Acceptance signals

- Atomic write tests stage in target parent and prove `EXDEV` cannot occur.
- Crash tests cover before write, during write, after temp fsync, after rename before parent fsync, after parent fsync before index, after index before event, and after event.
- Stale-base write returns `WriteError::StaleBase` and leaves the file unchanged.
- Confidential write never writes plaintext bytes to repo path or SQLite FTS/vector tables.
- Event-after-commit failure produces a committed outcome plus startup reconciliation marker.

---

## 9. Validator

### 9.1 Passes

1. YAML parse and type pass.
2. Required-key and enum pass.
3. Per-file cross-field pass.
4. Canonical serialization pass.
5. Cross-file validation pass when validating a tree.

Type/required failures short-circuit for a file. Cross-field pass collects all applicable errors.

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
    SecretSensitivity,
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
- A memory missing a known nullable key fails validation.
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
    entity_id TEXT NOT NULL,
    alias_norm TEXT NOT NULL,
    alias_raw TEXT NOT NULL,
    PRIMARY KEY(entity_id, alias_norm)
);
CREATE INDEX idx_entity_aliases_norm ON memory_entity_aliases(alias_norm, entity_id);

CREATE TABLE memory_supersession (
    earlier_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    later_id   TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    PRIMARY KEY(earlier_id, later_id),
    CHECK (earlier_id <> later_id)
);
CREATE INDEX idx_supersession_later ON memory_supersession(later_id);

CREATE TABLE memory_related (
    a_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    b_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    PRIMARY KEY(a_id, b_id),
    CHECK(a_id < b_id)
);
CREATE INDEX idx_related_b ON memory_related(b_id, a_id);

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

CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

Vector storage is behind an adapter:

```rust
trait VectorStore {
    fn create_or_open(model: EmbeddingModelRef, dimension: u32) -> Result<Self, VectorError>;
    fn upsert_chunk(&self, chunk_id: &ChunkId, vector: &[f32], meta: EmbeddingMeta) -> Result<(), VectorError>;
    fn delete_chunk(&self, chunk_id: &ChunkId) -> Result<(), VectorError>;
    fn search(&self, query: &[f32], filter: VectorFilter, limit: usize) -> Result<Vec<VectorHit>, VectorError>;
}
```

The v0.1 default adapter is sqlite-vec if available. Because vector virtual-table DDL is extension-specific, the implementation must pin the tested sqlite-vec version in `Cargo.lock`/build metadata and generate adapter DDL from code, not hand-maintain ambiguous SQL in this spec.

### 10.2 Indexer behavior

Indexer operations are explicit, transaction-wrapped reconciliations:

- `Created`/`Modified`: read, validate, compute chunks, replace memory row and all derived rows.
- `Deleted`: delete memory row by path after resolving current ID; cascades remove chunks, FTS rows, tags, aliases, entities, evidence, regressions, and embedding metadata; vector adapter is invoked to delete chunk vectors as part of the same indexer operation, but **not** transactionally with the SQLite metadata change (see §10.2.1).
- `Renamed`: read destination file. If frontmatter ID unchanged, update path and reconcile derived rows. If frontmatter ID changed, delete old ID and upsert new ID. Never path-update blindly.
- `Tombstoned` or sensitivity changes: purge non-indexable chunks/vectors immediately unless a safe masked projection is provided.

Index transactions never hold a SQLite connection across async await points. Blocking SQLite work is done on a dedicated blocking executor or single index thread owned by Stream B.

### 10.2.1 Vector store consistency

The vector store is an external adapter (§10.1) and may live in a sqlite-vec virtual table, a sidecar SQLite database, or a process-external store. **None of these are guaranteed to honor an enclosing SQLite transaction's rollback.** The indexer therefore cannot rely on `BEGIN ... COMMIT` to keep `chunk_embedding_meta` and the vector store in lockstep.

Stream A's actual contract is *eventually-consistent within a bounded window*, with a startup reconciliation pass closing every drift:

1. **Write path.** Indexer commits the SQLite metadata transaction first (memory row + chunks + `chunk_embedding_meta`). It then calls `VectorStore::upsert_chunk` / `delete_chunk` for each affected chunk. If the vector call fails after the metadata commit, indexer emits `IndexFailed { stage: VectorAdapter, chunk_ids }` and records the divergence in a per-chunk `pending_vector_op` table:

   ```sql
   CREATE TABLE pending_vector_ops (
       chunk_id   TEXT PRIMARY KEY,
       op         TEXT NOT NULL CHECK (op IN ('upsert','delete')),
       enqueued_at TEXT NOT NULL,
       attempts   INTEGER NOT NULL DEFAULT 0,
       last_error TEXT
   );
   ```

2. **Background drain.** A bounded retry worker drains `pending_vector_ops` with exponential backoff. Successful drains delete the row and emit `VectorReconciled`.
3. **Startup pass.** `Substrate::open` runs a reconciliation that:
   - replays `pending_vector_ops` once before accepting new writes;
   - walks `chunk_embedding_meta LEFT JOIN VectorStore::list_chunks()` and detects orphan vectors (vector exists, no metadata) → delete; and missing vectors (metadata exists, no vector) → enqueue an upsert.
   - emits `VectorReconciliationReport` summarizing both directions.

The §10.5 invariants are stated in terms of the **post-reconciliation** state. Stream B must not treat newly-acknowledged writes as having vector coverage until either the inline `upsert_chunk` succeeds or the next reconciliation completes.

### 10.3 Chunking contract

Default chunking:

- Target ~400 tokens, 80-token overlap.
- Chunk boundaries prefer Markdown headings, paragraphs, then sentence boundaries.
- Chunks include byte offsets into normalized LF body.
- Any body above 1 MiB is artifacted or chunked streaming; it must not be copied into a single SQLite `body` column.

### 10.4 Query contract

Stream A exposes typed read-only query helpers for the MCP shapes Stream B/E need:

- by ID/path;
- by tag/entity/alias;
- by namespace/scope/status/type/sensitivity/time;
- FTS chunk search with snippets;
- vector chunk search through adapter;
- hybrid result assembly with per-hit `score_breakdown` inputs, not final policy ranking.

**Metadata-only memories.** A confidential or personal memory without a Stream D safe projection has zero rows in `memory_chunks` and no vectors. Chunk-level FTS and vector queries must not return it. Metadata queries (`query_memory`) **do** return it with a `body_indexability: MetadataOnly` field on `MemoryHit`, so Stream E can fall back to summary-level recall and `memory_get(id)` (which reads the canonical file directly through Stream A) without leaking body bytes through SQLite. Hybrid result assembly skips metadata-only memories unless the caller sets `MemoryQuery.include_metadata_only = true`.

Raw mutable SQLite access is not exported. A test-only read-only SQL API may exist behind `cfg(test)` or an explicit admin feature.

### 10.5 Integrity invariants

- `memories.id == frontmatter_json.id`.
- Every indexed path exists at transaction time unless processing a delete.
- Every chunk belongs to one memory.
- `memory_chunks.chunk_rowid` is stable across `VACUUM` (it is `INTEGER PRIMARY KEY AUTOINCREMENT`).
- FTS rows are updated with the SQLite FTS5 external-content delete+insert trigger pattern keyed on `chunk_rowid`.
- Old unique terms disappear after update/delete.
- No vector exists for a missing, tombstoned, secret, or non-indexable chunk **after the next reconciliation pass completes** (§10.2.1). The post-write window between metadata commit and vector adapter call is bounded by `pending_vector_ops` drain, not by SQLite transactional rollback.
- Reindex from files produces the same query-visible state as watcher-driven incremental indexing.

### 10.6 Acceptance signals

- 10K-memory load test includes long bodies, large bodies, aliases, entity aliases, regressions, prospective memories, tombstones, encrypted metadata, and supersession chains.
- FTS mutation test proves old terms vanish after update/delete.
- **VACUUM regression test:** load 1K chunks, run `VACUUM`, run a chunk FTS query that previously matched, verify the same `chunk_id`s come back. Regression-protects the §10.1 fix.
- Vector lifecycle test proves delete/tombstone/sensitivity changes purge vectors after reconciliation.
- **Vector adapter failure test:** inject `VectorStore::upsert_chunk` errors, verify `pending_vector_ops` row created, restart substrate, verify reconciliation drains the row and the §10.5 invariant holds.
- **Vector orphan/missing reconciliation test:** seed an orphan vector and a missing vector before `Substrate::open`, verify both directions repair on startup.
- Query p95 targets are measured for real Stream E shapes: namespace + status + sensitivity cap + entity/alias + updated_at sort.
- Rename tests cover path-only rename and rename plus ID change.
- Metadata-only memory test: confidential memory with no Stream D projection appears in metadata query results, never in chunk FTS or vector search results, and `MemoryHit.body_indexability == MetadataOnly`.

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
    final_file_hash: Sha256,
    committed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}
```

A watcher event is suppressed only if the current file hash equals `final_file_hash`. If an external editor modifies the same path within the suppression window, the hash differs and the event is processed.

Default expiry is 60 seconds, but correctness does not depend on expiry; hash mismatch wins.

### 11.4 Acceptance signals

- Substrate write updates the index even when the watcher notification is suppressed.
- External edit to the same path within the suppression window is indexed.
- Watcher overflow emits `RescanRequired` and a reindex converges.
- Mass changes converge to fresh-reindex state; tests do not assert impossible exact OS event counts.

---

## 12. Event log

### 12.1 Format

Per-device JSONL at `events/<device-id>.jsonl`. Device IDs are local and unique per adopted clone.

Each line is one framed event:

```json
{"schema":1,"id":"evt_01HX...","ts":"2026-04-24T13:14:15.123Z","device":"dev_a1b2...","seq":42,"kind":"WriteCommitted","data":{},"crc32c":"..."}
```

`seq` is per-device monotonic and persisted locally. ULID timestamp order is useful for display but not treated as causal truth.

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
- `MergeQuarantined`
- `DuplicateIdRepaired`
- `GitCommitted`
- `GitFetched`
- `GitPushFailed`
- `WatcherSuppressed`
- `ReconciliationRepaired`
- `StartupReconciliationCompleted`

Every kind has a typed data schema in code and fixtures. Free-form `data` is not permitted in implementation even if rendered schematically in docs.

### 12.3 Append semantics

1. Encode event as one bounded UTF-8 buffer. Max line length: 64 KiB; larger payloads must be artifacted and referenced.
2. Append with file opened `O_APPEND`.
3. Retry short writes until full buffer is written or an error occurs.
4. `fsync(log_fd)` after each event for v0.1.
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
3. Diff `origin/main`'s `.gitattributes`, merge-driver config (`config.local`), and any `policies/` files against the working tree.
4. Return `InspectReport { remote_fixes: Vec<RepoConfigDelta>, still_required: Vec<PreflightFailure> }`.

Stream B uses the report to drive a guided repair: show the remote-side fix, let the user accept it, apply via `git checkout origin/main -- .gitattributes` (or equivalent), then re-run full preflight. `fetch_inspect` never merges and never modifies the working tree itself.

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
2. **Working-tree audit.** `git status --porcelain=v1 -z` must be either clean, or contain only files whose hashes match `pending_reconciliation_files` recorded by the prior process. Unexpected uncommitted changes are quarantined into `~/.memoryd/quarantine/<startup-ts>/` (copied, not moved) and an `OperatorRepairRequired` event is emitted; substrate refuses writes until the operator clears the marker.
3. **Vector reconciliation.** Run §10.2.1 startup pass.
4. **Event log recovery.** Truncate any trailing partial line per §12.3 and emit `EventLogRecovered` if needed.
5. **Pending event-after-commit reconciliation.** Walk `pending_events` (events whose append failed after their write committed) and re-append.
6. **Index/file consistency check.** Sample `memories.file_hash` against current file hashes; mismatches enqueue a reindex.
7. **Auto-commit any post-merge reconciliation work that was never committed.** This is the path that fires when the daemon crashed in the §13.5 step-10 debounce window.
8. Emit `StartupReconciliationCompleted { phases_run, reindexed, vector_repairs, event_repairs, operator_action_required: bool }`.

Substrate must not return from `open` until startup reconciliation completes or returns an explicit operator-required error. There is no path where Stream B begins serving writes against an unreconciled substrate.

### 13.6 JSONL union merge rules

`merge=union` is allowed only because readers validate, dedupe by event ID, and quarantine malformed non-final lines. JSONL order in git is not semantically meaningful.

### 13.7 Acceptance signals

- Fresh clone without adoption fails preflight with a specific repair instruction.
- Fresh clone with adoption can perform a semantic same-file merge.
- Diverged local/remote branches merge; ahead-only branches do not incorrectly skip future behind state.
- Missing merge-driver binary refuses before merge.
- JSONL union duplicate lines replay idempotently.

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
2. **Schema-version gate.** If any side's `schema_version` exceeds the driver's supported version, exit `1` with stderr `merge-driver: schema_version=<n> exceeds supported=<m>; upgrade required`. Git surfaces the conflict; the user upgrades the driver before retrying. The driver never silently falls back to "unknown _extras" handling for a higher schema.
3. If any side cannot parse frontmatter, produce a valid quarantined file if possible; otherwise leave Git conflict markers and exit `1`.
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
| `sensitivity` | true 3-way (so a legitimate downgrade by one side survives if the other is unchanged from base); same-field 3-way conflict selects the maximum by order `secret > personal > confidential > internal > public` and records the loser in `_merge_diagnostics`; if the same-field result would be `secret`, quarantine and refuse commit path |
| `status` | lifecycle merge table (§14.5) |
| `review_state`, `requires_user_confirmation` | true 3-way; same-field conflict chooses stricter state: pending > approved > null > rejected for review, true > false for confirmation |
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
| unknown `_extras` | per-key true 3-way when key exists in base; add/add same key conflict quarantines unless values equal |

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

- If frontmatter IDs differ: produce a valid quarantined combined file at the contested path with both blobs in diagnostics/body, exit `0` if valid; repo-level repair may later split manually but must not lose either body.
- If frontmatter IDs match: produce a valid quarantined file and emit diagnostics that duplicate-ID repair is required; do not invent invalid suffix IDs.

### 14.7 Quarantine marker

A semantic quarantine file has:

- `status: quarantined`;
- `trust_level: quarantined`;
- `review_state: pending`;
- `_merge_diagnostics.status: quarantined`;
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
  shard: a1b2
paths:
  memory_root: ~/.memory
  runtime_root: ~/.memoryd
```

### 15.3 Environment overrides

Env overrides are for tests/CI and local repair. They must not mutate synced config unless explicitly saved.

### 15.4 Acceptance signals

- Fresh clone has synced config but no local device config until adoption.
- Loading config never copies another machine's device ID from repo state.
- Env overrides are visible in loaded config but not serialized into `config.yaml` unless asked.

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
    pub fn open(roots: Roots) -> Result<Self, OpenError>;
    pub fn init(roots: Roots, opts: InitOptions) -> Result<Self, InitError>;
    pub fn adopt_clone(roots: Roots, opts: AdoptOptions) -> Result<Self, AdoptError>;
    pub fn doctor(&self) -> DoctorReport;
}
```

### 16.2 Frontmatter/markdown

```rust
pub fn parse_frontmatter(yaml: &str) -> Result<FrontmatterRaw, ParseError>;
pub fn validate_frontmatter(raw: FrontmatterRaw) -> Result<Frontmatter, ValidationReport>;
pub fn serialize_frontmatter(fm: &Frontmatter) -> Result<String, SerializeError>;

impl Substrate {
    pub fn read_memory(&self, id: &MemoryId) -> Result<Memory, ReadError>;
    pub fn read_path(&self, path: &RepoPath) -> Result<Memory, ReadError>;
    pub fn write_memory(&self, req: WriteRequest) -> Result<WriteOutcome, WriteError>;
    pub fn write_encrypted(&self, req: EncryptedWriteRequest) -> Result<WriteOutcome, WriteError>;
    pub fn tombstone_memory(&self, req: TombstoneRequest) -> Result<WriteOutcome, WriteError>;
}
```

### 16.3 IDs

```rust
impl Substrate {
    pub fn next_memory_id(&self) -> Result<MemoryId, IdError>;
}
pub fn parse_memory_id(s: &str) -> Result<MemoryId, IdError>;
```

### 16.4 Index

```rust
impl Substrate {
    pub fn reindex(&self, opts: ReindexOptions) -> Result<ReindexReport, IndexError>;
    pub fn query_memory(&self, query: MemoryQuery) -> Result<Vec<MemoryHit>, QueryError>;
    pub fn query_chunks(&self, query: ChunkQuery) -> Result<Vec<ChunkHit>, QueryError>;
    pub fn update_embedding(&self, req: EmbeddingUpdate) -> Result<(), VectorError>;
}
```

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
    pub fn append_event(&self, event: Event) -> Result<EventAppendOutcome, EventError>;
    pub fn read_events(&self, query: EventQuery) -> Result<impl Iterator<Item = Result<Event, EventReadError>>, EventError>;
    pub fn git_preflight(&self) -> Result<(), GitError>;
    pub fn fetch_inspect(&self, opts: InspectOptions) -> Result<InspectReport, GitError>;
    pub fn auto_commit(&self, opts: CommitOptions) -> Result<Option<CommitSha>, GitError>;
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

Blocking git operations must run on `spawn_blocking` internally or be documented as blocking with `*_blocking` names. The public API must not hide network/blocking behavior behind cheap-looking sync calls.

### 16.6 Error taxonomy

Errors distinguish:

- validation/schema problems;
- stale write/concurrency problems;
- durability/fsync problems (including `DurabilityUnsupported` and `DurabilityUnavailable` per §3.1);
- index-after-commit failures (with `pending_index_ops` enqueue receipt);
- event-after-commit failures (with `pending_events` enqueue receipt);
- git preflight/config failures (`PreflightFailed` distinct from `ConfigDeltaSinceLastInspect`);
- merge semantic quarantines;
- merge schema-version refusals (`SchemaVersionUnsupportedByMergeDriver`);
- vector adapter/model/dimension failures (with explicit `pending_vector_ops` enqueue receipt);
- operator-required repair states (`OperatorRepairRequired` from §13.5.1 step 2);
- partial-sync cross-reference warnings.

No single `IoError` bucket may hide whether a write was committed before failure. `WriteOutcome` always reports `{ committed, indexed, event_recorded, durability }` so callers can distinguish recoverable from unrecoverable states.

### 16.7 Acceptance signals

- Public API has no extra exported mutable internals.
- Every error variant has at least one test.
- Async git/watch APIs can be cancelled without corrupting repo/index/event state.
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
- crash during event append;
- startup reconciliation after each committed-but-incomplete state.

### 17.6 Performance tests

Benchmarks must record hardware, OS, filesystem, SQLite pragmas, and memory count. v0.2 promotes performance from "informational" to **release gate**, because Stream E's recall-block budget assumes these p95s; missing them ships a Stream A that downstream streams cannot use.

Reference hardware for the gate: Apple Silicon laptop (M-series, 16 GB+) **or** Linux x86_64 CI runner (≥4 vCPU, NVMe-class storage). Both must pass.

Gate targets:

- 10K-memory cold reindex p95 ≤ 60s.
- Query by ID p95 ≤ 10ms.
- Filtered metadata query p95 ≤ 50ms.
- FTS chunk query p95 ≤ 75ms.
- Vector chunk query p95 ≤ 100ms (sqlite-vec adapter, 768-dim vectors, 10K corpus).
- Tree validator p95 ≤ 500ms on 10K memory files.

Each target is measured with a deterministic fixture seed and captured in `bench/results.json` per run. A regression > 10% on any target blocks merge.

### 17.7 Overall acceptance

Stream A is done when:

1. All section acceptance signals pass.
2. `cargo test --workspace --release` passes on macOS arm64 and Linux x86_64.
3. Merge-driver fuzzing for 10 minutes produces no panics and no invalid output.
4. Two-clone multi-device scripted test converges byte-identically after semantic merges and repo-level reconciliation.
5. Public API docs explain blocking/async behavior and every error outcome.
6. **Performance gates from §17.6 pass on both reference hardware profiles.**
7. **Durability probe behaves per §3.1:** `Full` succeeds on a tmpfs/ext4/apfs fixture; `Refused` on a fixture that monkey-patches parent-dir fsync to `EINVAL`; `BestEffort` on a fixture returning a documented non-fatal error.
8. **Crash-injection matrix from §17.5 passes** with startup reconciliation cleanly converging to a consistent state.
9. Independent review has no blocking findings; the review is not itself a test, but a release gate.

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

Mitigation: explicit write outcomes, repair markers (`startup-reconcile.required`, `pending_index_ops`, `pending_events`), startup reconciliation (§13.5.1), and tests for every injected failure point in §17.5.

### 18.3a Vector store ↔ metadata drift

Risk: SQLite metadata commit succeeds, vector adapter call fails, leaving FTS-but-no-vector or vice-versa drift.

Mitigation: §10.2.1 contract — `pending_vector_ops` queue, background drain, startup reconciliation walks both directions, and a dedicated event kind (`VectorReconciled`) so the audit trail makes drift visible.

### 18.3b Durability degradation

Risk: silently acknowledging writes on a filesystem where parent-directory fsync is best-effort, then losing them on power loss.

Mitigation: §3.1 tiered probe at startup, `Refused` tier blocks `Substrate::open` by default, `BestEffort` tier requires per-write opt-in, every `WriteOutcome` carries the active tier so Stream B can refuse if policy demands `Full`.

### 18.4 Clone misconfiguration

Risk: missing merge driver or shared device ID causes text conflicts or corrupt per-device logs.

Mitigation: `adopt_clone`, preflight before merge, local device config outside repo, absolute driver path.

### 18.5 Vector model drift

Risk: one fixed vector dimension blocks model changes.

Mitigation: vector adapter keyed by provider/model/dimension and chunk content hash; rebuild per model, not schema break.

### 18.6 Forward compatibility

Risk: v0.2 required fields break v0.1 daemons.

Mitigation: `schema_version`, additive-only nullable changes without migration, read-only behavior for unsupported versions.

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

## 20. Open decisions before implementation lock

Status legend below each item. v0.2 closes a few from v0.1 and adds none new.

1. **Vector adapter:** default sqlite-vec, DDL adapter-generated and version-pinned. The §17.6 vector-query gate and §17.7 acceptance #6/#7 effectively force this choice for v0.1; alternates (e.g. an in-process HNSW store) can ship as additional adapters in v0.2+. *Status: leaning closed; final approval needed.*
2. **Embedding model:** 768-dim local Gemma remains the default; schema supports multiple model/dimension pairs. *Status: open, low-stakes.*
3. **Encrypted index projection:** choose default for confidential/personal: `metadata_only` vs. `masked_summary_and_entities`. v0.2 §10.4 fixes the metadata-only fallback contract; the choice of *default* tier remains a Stream D coordination decision. *Status: open, blocks Stream D start but not Stream A.*
4. **Tombstone retention:** frontmatter tombstone events preserve reasons, but git history still retains old content. Confirm runbook expectations for true privacy erasure. *Status: open, tracked in Stream D/G.*
5. **Semantic quarantine git exit:** this spec says valid semantic quarantine exits `0`; unrepresentable conflicts and unsupported `schema_version` exit `1`. *Status: closed in v0.2 §14.2.*
6. **Performance reference hardware:** v0.2 §17.6 pins the gate to "Apple Silicon laptop **or** Linux x86_64 CI runner ≥4 vCPU + NVMe-class storage, both must pass." *Status: closed in v0.2.*
7. **Durability tier policy default:** v0.2 §3.1 introduces `Full | BestEffort | Refused`. The default is to refuse `BestEffort` unless caller opts in. Confirm this is the right default for the daemon (Stream B may want to surface a one-time user prompt instead). *Status: new in v0.2, low-stakes.*

---

## 21. What this spec deliberately does not decide

- Stream C promotion policy thresholds.
- Stream D Privacy Filter model details and encryption key management.
- Stream E final ranking formula and context-budget policy.
- Stream F dreaming logic.
- UI/CLI workflows for human conflict resolution.
- Multi-user ACLs beyond single-user future-proofing fields.

---

*End of Stream A — Core Substrate Spec v0.2.*
