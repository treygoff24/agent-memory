# Stream A — Core Substrate Spec (v0.1)

**Status:** draft for review (Trey + Codex). Targets implementation by Codex, code review by Claude.
**Parent:** `shared-memory-layer-spec-v0.1.md` (this spec is the implementation-depth expansion of Streams A/sub-items 1–4 from §19, plus the contract surface every other Stream depends on).
**Date:** 2026-04-24.

---

## 1. Purpose

Stream A is the substrate every other stream sits on. It owns the on-disk format, the indexer, the event log, and the git merge driver. Get this right and the rest of the system is a set of well-scoped consumers; get this wrong and every other stream inherits the rot.

Concretely, Stream A produces:

1. A library crate (`memory-substrate`) that other streams link as a dependency. This crate is the only code in the system that touches the filesystem, the SQLite index, the event log JSONL files, or the git working tree.
2. A standalone binary (`memory-merge-driver`) that git invokes during merges to handle frontmatter semantically.
3. A reference test suite that exercises the crate and binary end to end, including realistic multi-device merge scenarios.

Stream A is **not a process**. It does not run. It is a library that other streams (notably Stream B, the daemon) instantiate and drive. The merge driver binary runs only when git invokes it.

The reason this matters: by drawing the substrate as a library rather than a service, Stream B owns process lifecycle, Stream C owns governance gates, Stream D owns privacy gates, etc. The substrate stays single-purpose: format, durability, queryability, mergeability.

---

## 2. Scope

### 2.1 In scope

- Memory tree directory layout under `~/.memory/` (canonical paths, naming conventions, sidecar file placement).
- Frontmatter schema: complete field definitions, types, required-vs-optional discipline, constraints.
- Frontmatter validator: parses, type-checks, constraint-checks, returns structured errors.
- Markdown file I/O: read/write Markdown files with frontmatter, atomic write semantics, line-ending normalization.
- ID generation: `mem_YYYYMMDD_<seq>` with collision handling.
- SQLite schema and indexer: tables, indexes, FTS5 keyword search, vector column for embeddings, derived-from-files invariant.
- File watcher: detects changes to the memory tree, emits typed events.
- Event log: per-device append-only JSONL, schema, append semantics, replay-ability.
- Git operations: init, auto-commit with debounce, commit message format, gitattributes config.
- Frontmatter merge driver: gets invoked by git, parses frontmatter from base/ours/theirs, applies field-level merge rules, emits resolved file or sets quarantine state.
- Configuration loading: `~/.memory/config.yaml` plus environment overrides.
- Public Rust API of `memory-substrate` (the contract Stream B depends on).

### 2.2 Out of scope (other streams)

- Daemon process management, signal handling, `launchd`/`systemd` integration (Stream B).
- Unix socket server, MCP protocol, agent-facing tools (Stream B).
- Embedding worker — Stream A owns where embeddings live in SQLite, but the embedder lives in Stream B.
- Governance machinery: contradiction detection, supersession chains, tombstone matching, grounding verification (Stream C). Stream A enforces *schema*, not *policy*.
- Privacy Filter integration, age-encrypted tier, secret detection regex (Stream D). Stream A enforces *file routing by `sensitivity` field*, but does not classify content.
- Passive recall, base block assembly, hooks (Stream E).
- Dreaming pipeline (Stream F).
- CLI admin commands beyond the subset Stream A needs for testing (Stream G).
- Eval harness (Stream H).
- Cross-session event subscription (Stream I) — Stream A produces the event log; Stream I builds the subscription model on top.

### 2.3 Boundary clarifications (where the line is fuzzy)

- **Sensitivity field routing.** Stream A reads the `sensitivity` field from frontmatter and chooses which subtree the file lives in (`encrypted/` vs. plain). Stream A does *not* run the Privacy Filter, does *not* classify content, does *not* perform encryption itself; it consumes a classification produced by Stream D and routes accordingly.
- **Policy validation.** Stream A's validator enforces *schema* (field types, required fields, enum values). Stream C's policies enforce *gates* (does this write meet `me-strict`?). Stream A returns a typed error if schema is wrong; Stream C consumes valid memories and applies governance.
- **Index queries.** Stream A exposes raw SQL access through the public API (parameterized queries, prepared statements). Stream B layers MCP-tool query shapes on top. Stream A does not own the agent-facing query surface.

---

## 3. Dependencies and assumptions

### 3.1 Inherited from v0.1 system spec (locked, do not re-litigate)

All 14 locked decisions from `handoff.md`. The ones Stream A directly inherits:

- Markdown + YAML frontmatter is canonical. SQLite is derived. JSONL event log is durable audit. (Decision 3.)
- Single daemon owns all writes through Stream A's library. Files are not written by any other code path. (Decision 4.)
- Identity is git-remote SHA256 with `.memory-project.yaml` override. (Decision 5.)
- Sync is git-backed with semantic frontmatter merge driver. (Decision 6.)
- Frontmatter schema as defined in v0.1 §7 with full A′ discipline. (Decision 8.)
- Sensitivity routing: `secret` refused, `confidential`/`personal` → encrypted tier, `public`/`internal` → plain git-synced. (Decision 10, partial — Stream A enforces routing, not classification.)

### 3.2 Assumptions about other streams

Stream A is implemented as if the following will be true:

- **Stream B will be the only caller** of `memory-substrate`'s mutating APIs. The library is not designed to be safe under concurrent writes from multiple processes; it assumes one daemon owns the tree.
- **Stream C will validate writes** before they reach the substrate. The substrate's validator is a *backstop*, not the primary gate. Schema errors at this layer indicate a bug upstream.
- **Stream D will set the `sensitivity` field and `privacy_scan` block** before calling the substrate's write API. The substrate routes on these fields without re-running detection.
- **Stream B owns the embedder.** When Stream B has computed an embedding, it calls the substrate's `update_embedding(memory_id, vector)` API. The substrate stores the vector; it does not generate one.

### 3.3 Platform assumptions

- macOS 14+ and Linux (kernel 5.10+). Windows is a future concern (NSSM mention in v0.1 §5); Stream A targets macOS and Linux first.
- POSIX filesystem with atomic rename semantics (rename(2) within the same filesystem).
- Git 2.40+ (for merge driver protocol and reliable `gitattributes` matching).
- SQLite 3.45+ (FTS5 features and JSON1 support).
- Filesystem case-sensitivity *not* assumed: paths must be unique under case-insensitive comparison to support default macOS APFS configurations and case-sensitive Linux deployments.

---

## 4. Component map

```
memory-substrate (library crate)
├── tree::layout           — canonical paths, naming, validation of tree shape
├── frontmatter::schema    — field definitions, enum domains, constraints
├── frontmatter::parser    — YAML frontmatter ←→ typed struct
├── frontmatter::validator — typed struct → ValidationResult
├── markdown::file         — read/write Markdown+frontmatter atomically
├── ids                    — generate, parse, validate memory IDs
├── index::schema          — SQLite DDL, migrations
├── index::writer          — apply file changes to index
├── index::query           — SELECT helpers (raw SQL access)
├── watcher                — FS event source
├── events::log            — append, read, replay JSONL event logs
├── git::repo              — open, init, commit, fetch operations
├── git::auto_commit       — debounce + message generation
└── config                 — load ~/.memory/config.yaml + env overrides

memory-merge-driver (binary)
└── single-purpose: invoked by git; parses frontmatter from base/ours/theirs
    paths; applies field-level merge; writes resolved file; exits 0 (success)
    or 1 (quarantine).
```

The split between library and merge-driver binary is deliberate. Git's merge driver protocol expects an executable on PATH, so the merge logic ships as its own binary. The binary depends on the same `memory-substrate` crate so the merge rules and the validator share code.

---

## 5. Memory tree layout

The canonical tree is rooted at `~/.memory/` (configurable via `MEMORY_ROOT` env or `--root` CLI flag). The tree is a git repo. SQLite index and runtime files live outside the repo at `~/.memoryd/` (also configurable).

### 5.1 Repo paths (synced, in git)

```
~/.memory/
├── .git/
├── .gitattributes                        # configures merge driver
├── .memory-project.yaml                  # optional; project identity override
├── config.yaml                           # daemon config; user-edited
├── me/
│   ├── identity/
│   │   ├── role.md
│   │   └── principles.md
│   ├── relationship/
│   │   ├── facts/<entity-slug>.md
│   │   ├── preferences/<topic-slug>.md
│   │   ├── corrections/<id>.md
│   │   └── patterns/<id>.md
│   ├── knowledge/<topic-slug>.md
│   ├── episodic/<YYYY-MM-DD>.md
│   └── prospective/<id>.md
├── projects/
│   └── <namespace-segment>/<sub>/        # nested allowed; e.g. prospera/atlasos/
│       ├── state.md
│       ├── decisions/<YYYY-MM-DD>-<slug>.md
│       ├── open-questions/<id>.md
│       ├── playbooks/<slug>.md
│       ├── entities/<entity-slug>.md
│       ├── episodic/<YYYY-MM-DD>.md
│       ├── invariants.md
│       └── regressions/<id>.md
├── agent/
│   ├── patterns/<id>.md
│   ├── playbooks/<slug>.md
│   ├── postmortems/<id>.md
│   ├── anti-patterns/<id>.md
│   ├── heuristics/<id>.md
│   ├── regressions/<id>.md
│   └── episodic/<YYYY-MM-DD>.md
├── dreams/
│   ├── journal/<YYYY-MM-DD>.md
│   ├── questions/<YYYY-MM-DD>.md
│   └── reports/<phase>/<YYYY-MM-DD>.md
├── substrate/
│   └── <device-id>/<YYYY-MM-DD>.jsonl
├── encrypted/                            # mirrors namespace structure under age
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
├── leases/
│   └── journal.lease
└── privacy-scans/
    └── <memory-id>.json                  # sidecars referenced by frontmatter
```

### 5.2 Non-repo paths (per-device, not synced)

```
~/.memoryd/                                # not in git, per-device runtime state
├── index.sqlite                           # primary derived index
├── index.sqlite-wal                       # SQLite WAL
├── index.sqlite-shm                       # SQLite shared memory
├── socket                                 # daemon Unix socket (Stream B)
├── pid                                    # daemon pid file (Stream B)
├── logs/
│   └── memoryd.log
└── tmp/                                   # atomic write staging
```

### 5.3 Path constraints

- `<id>` matches `^mem_\d{8}_\d{3,6}$` (see §7).
- `<slug>` is `[a-z0-9][a-z0-9-]{0,62}` (kebab-case, 1–63 chars, no leading hyphen).
- `<entity-slug>` follows the same slug rules; the canonical entity ID is in frontmatter, the filename is a human-readable slug.
- `<YYYY-MM-DD>` is ISO 8601 calendar date (UTC).
- `<namespace-segment>` is a slug; `projects/<a>/<b>/` represents a nested namespace where `<a>/<b>` is the human-readable namespace alias (canonical id is in frontmatter and `.memory-project.yaml`).
- Paths must be unique under case-insensitive comparison. `me/knowledge/Foo.md` and `me/knowledge/foo.md` are the same path.

### 5.4 Tree validator

The library exposes `tree::validate(root: &Path) -> ValidationReport`. It walks the tree and reports:

- Files outside the canonical path patterns (e.g., a `.md` in `me/` root rather than under a recognized subdirectory).
- Filename mismatches (frontmatter `id` does not match `<id>.md` filename for paths that use ID-based naming).
- Slug violations.
- Case-insensitive collisions.

Tree validator is read-only and cheap; it runs at daemon startup and on demand via CLI.

### 5.5 Acceptance signals

- A freshly-initialized tree (`memory init`) produces every directory in §5.1 (empty), `.gitattributes` containing the merge driver line, an initial commit with the empty tree.
- `tree::validate(root)` on the result returns `ValidationReport { errors: 0, warnings: 0 }`.
- A test fixture with a known invalid file (e.g., `me/knowledge/Foo Bar.md`) returns a `ValidationReport` containing exactly one error of kind `SlugViolation { path: "me/knowledge/Foo Bar.md", reason: "uppercase" }`.

---

## 6. Frontmatter schema

This section defines the typed schema. The on-disk YAML is the source of truth; this struct is the in-memory representation.

### 6.1 Required fields (every memory)

| Field | Type | Constraint |
|-------|------|------------|
| `id` | string | matches `^mem_\d{8}_\d{3,6}$` (see §7) |
| `type` | enum | one of: `project`, `person`, `procedure`, `episode`, `claim`, `artifact`, `prospective`, `pattern`, `playbook`, `postmortem`, `anti-pattern`, `heuristic`, `regression`, `correction`, `invariant`, `decision`, `open-question` |
| `scope` | enum | one of: `user`, `project`, `org`, `agent`, `subagent` |
| `summary` | string | 1–280 chars; first sentence is the operational headline |
| `confidence` | float | 0.0 ≤ x ≤ 1.0; default forbidden (must be set explicitly) |
| `trust_level` | enum | one of: `trusted`, `untrusted`, `candidate`, `quarantined`, `pinned` |
| `sensitivity` | enum | one of: `public`, `internal`, `confidential`, `secret`, `personal` |
| `status` | enum | one of: `candidate`, `active`, `pinned`, `superseded`, `archived`, `tombstoned` |
| `created_at` | datetime | RFC 3339 with timezone, UTC preferred |
| `updated_at` | datetime | RFC 3339 with timezone; ≥ `created_at` |
| `author` | string | matches `^(user|agent|dreaming):[a-z0-9-]+(:[A-Za-z0-9_-]+)*$` |

### 6.2 Conditionally required fields

| Field | Required when | Type |
|-------|---------------|------|
| `namespace` | `scope ∈ {project, org}` | string (human-readable alias) |
| `canonical_namespace_id` | `scope ∈ {project, org}` | string matching `^proj_[0-9a-f]{16}$` |
| `regression` | `type == regression` | object (see §6.6) |
| `privacy_scan` | Stream D Privacy Filter ran on this memory | object (see §6.7) |
| `review_state` | `requires_user_confirmation == true` | enum: `pending`, `approved`, `rejected`, or `null` |

### 6.3 Optional fields with explicit nullability

Every optional field below is either present with a value or present as `null`. Missing fields are a schema error. The reason: this discipline keeps the field set stable across reads and forces writers to think about each axis.

| Field | Type | Default |
|-------|------|---------|
| `tags` | array of slugs | `[]` |
| `entities` | array of `{id: string, label: string}` | `[]` |
| `aliases` | array of strings | `[]` |
| `source` | object (see §6.4) | required object with `kind` set; other inner fields nullable |
| `evidence` | array of `{quote, ref, weight, observed_at}` | `[]` |
| `requires_user_confirmation` | bool | `false` |
| `observed_at` | datetime or null | `null` |
| `valid_from` | datetime or null | `null` |
| `valid_until` | datetime or null | `null` |
| `ttl` | ISO 8601 duration or null | `null` |
| `supersedes` | array of memory IDs | `[]` |
| `superseded_by` | array of memory IDs | `[]` |
| `related` | array of memory IDs | `[]` |
| `retrieval_policy` | object (see §6.5) | required object; inner defaults below |
| `write_policy` | object (see §6.5) | required object; inner defaults below |

### 6.4 `source` object

```yaml
source:
  kind: user | agent-primary | agent-subagent | tool | web | email | file | synthesis  # required
  ref: string | null             # session id, file path, URL handle, artifact id
  harness: claude-code | codex | cursor | cli | null
  session_id: string | null
  subagent_id: string | null
  device: string | null          # matches ^dev_[0-9a-f]{16}$
```

### 6.5 Policy objects

```yaml
retrieval_policy:
  passive_recall: bool                          # default: true
  max_scope: user | project | org | agent       # default: project
  mask_personal_for_synthesis: bool             # default: true

write_policy:
  human_review_required: bool                   # default: false
  policy_applied: string                        # e.g., me-strict@v3; required
```

### 6.6 `regression` object (only when `type == regression`)

```yaml
regression:
  detection_signature:
    error_string_regex: string | null
    stack_fingerprint: string | null            # opaque hash from Stream C tooling
    tool_output_hash: string | null             # SHA256 of normalized tool output
    behavioral_marker: string | null            # human-readable trigger description
  fire_on_attempt: bool                         # default: true
  first_observed: datetime
  last_observed: datetime
  occurrence_count: integer                     # ≥ 1
```

At least one of `error_string_regex`, `stack_fingerprint`, `tool_output_hash`, `behavioral_marker` must be non-null. A regression with no detection signature is a schema error.

### 6.7 `privacy_scan` object

```yaml
privacy_scan:
  model: string                                  # e.g., openai/privacy-filter@v1.0
  ran_at: datetime
  spans_detected: integer                        # ≥ 0
  labels: array of enum
                                                  # one of: private_person, private_email, private_phone,
                                                  # private_address, private_credential, private_health,
                                                  # private_financial, private_other
  span_details_ref: string | null               # sidecar://privacy-scans/<memory-id>.json
```

### 6.8 Cross-field constraints

These are validated after type-checking individual fields:

1. `updated_at >= created_at`.
2. If `valid_from` and `valid_until` are both set, `valid_until > valid_from`.
3. If `status == superseded`, `superseded_by` must be non-empty.
4. If `status == tombstoned`, `superseded_by` must be empty (tombstones are terminal).
5. `id` must not appear in `supersedes`, `superseded_by`, or `related` (no self-reference).
6. `supersedes` and `superseded_by` must not overlap.
7. If `scope ∈ {project, org}`, `namespace` and `canonical_namespace_id` must be set.
8. If `type == regression`, the `regression` object must be present and have a non-null detection signature.
9. If `sensitivity == secret`, write is refused (the substrate returns `WriteRefused::SecretSensitivity`; Stream B/D are responsible for handling secrets before they reach the substrate).
10. `confidence` must be present even when `null` is not allowed; the default-forbidden rule for `confidence` means the writer must consciously assign a value.

### 6.9 YAML serialization rules

The validator is strict on YAML form to keep diffs clean and merge-friendly:

- Block style only. No flow style (no `[1, 2, 3]`, no `{key: value}` inline).
- Strings always single-quoted unless the value is a number, bool, null, datetime, or contains special characters requiring double quotes.
- Datetimes formatted as RFC 3339 with explicit `Z` for UTC; no implicit local time.
- Empty arrays serialized as `[]` (compact form is the one exception to "no flow style").
- `null` is the literal `null`, not `~` or empty.
- Maps in canonical key order: identity → content metadata → provenance → governance → privacy_scan → temporal → supersession → policies → type-specific.
- Top of file is `---`, end of frontmatter is `---`, body follows after a single blank line.
- Line endings: LF. Files written by the substrate normalize to LF on save; `.gitattributes` enforces `text eol=lf`.

The reason for this rigor: merge conflicts in YAML are a special hell when two writers happen to use different quoting styles. Canonicalizing the on-disk form means semantic merges aren't fighting cosmetic noise.

### 6.10 Acceptance signals

- Round-trip: take any valid memory, parse, re-serialize, byte-compare to the canonical form. Output must be byte-identical (modulo the original being canonical).
- Property test: generate random valid memories, serialize, parse, assert struct equality.
- Negative tests: 50+ malformed YAML samples, each with a documented expected error; validator must produce a `ValidationError` whose `kind` matches the expected enum variant.

---

## 7. ID generation

### 7.1 Format

`mem_YYYYMMDD_<seq>` where:

- `YYYYMMDD` is the UTC date the ID is minted.
- `<seq>` is a 3-to-6 digit zero-padded sequence number, monotonically increasing within the day on the minting device.

Examples: `mem_20260424_001`, `mem_20260424_087`, `mem_20260424_999999`.

### 7.2 Sequence allocation

The substrate reads `~/.memoryd/seq.json`:

```json
{
  "date": "2026-04-24",
  "next": 87
}
```

On `ids::next() -> MemoryId`:

1. Read the file under an exclusive flock.
2. If `date` matches today's UTC date, use `next`, increment, write back.
3. If `date` is older, reset to today with `next: 1`.
4. Release flock.
5. Return `mem_YYYYMMDD_<seq>` zero-padded to 3 digits if seq < 1000, else minimum width to fit.

### 7.3 Cross-device collision handling

Two devices can mint the same ID on the same day if seq counters collide. The strategy is **probabilistic avoidance + deterministic resolution**:

- **Avoidance:** the substrate seeds its sequence counter at startup with a per-device random offset in `[0, 100)`. With two devices and ~50 writes/device/day the collision probability is small but non-zero.
- **Resolution at merge time:** if two devices both mint `mem_20260424_087`, the merge driver detects the collision (same path, both sides have a memory with the same id, neither is an update of the other — this is a *parallel create*). The driver renames the later-committed one to `mem_20260424_087a`, updates its frontmatter `id`, rewrites any internal references in same-commit files, and emits a `CollisionRenamed` event. The earlier-committed memory keeps its ID.

"Earlier-committed" means by git committer timestamp. Tie-break by device-id lexicographic order.

### 7.4 Acceptance signals

- 10,000 sequential `ids::next()` calls on a single device produce 10,000 unique IDs in monotonic order.
- A simulated collision test (two trees both containing `mem_20260424_087`, merged via the driver) produces a tree with `mem_20260424_087` and `mem_20260424_087a`, both valid, both indexed.

---

## 8. Markdown file I/O

### 8.1 Read

`markdown::read(path) -> Result<Memory, ReadError>`:

1. Open file, read contents.
2. Find frontmatter delimiters: file must start with `---\n`, frontmatter ends at next `---\n`.
3. Parse YAML into typed `FrontmatterRaw` (no validation yet).
4. Body is everything after the second `---\n` (strip leading single newline if present).
5. Return `Memory { frontmatter: FrontmatterRaw, body: String, path: PathBuf }`.

Errors: `MissingDelimiter`, `MalformedYaml(line, message)`, `IoError`.

### 8.2 Write (atomic)

`markdown::write(memory: &Memory) -> Result<(), WriteError>`:

1. Validate (§9). On error, return `WriteError::ValidationFailed(report)`.
2. Determine path:
   - If `memory.path` is set, use it.
   - Else compute from `frontmatter` (e.g., `me/episodic/2026-04-24.md` for an episodic memory with today's date and scope `user`).
3. Serialize: render frontmatter (canonical YAML per §6.9), then `---\n\n`, then body.
4. Determine sensitivity routing:
   - `public` or `internal` → write to plain path under `~/.memory/`.
   - `confidential` or `personal` → write to `~/.memory/encrypted/<original-relative-path>`. Stream A writes the *plaintext* there; Stream D's commit hook encrypts before commit. (Open decision §15.4.)
   - `secret` → return `WriteError::SecretSensitivity` immediately, do not touch disk.
5. Write to staging path: `~/.memoryd/tmp/<uuid>.md`.
6. `fsync(staging_fd)`.
7. `rename(staging, final)`.
8. `fsync(parent_dir_fd)` to persist the rename.
9. Append a `WriteCommitted` event to the device's event log (§11).
10. Return success.

If any step 5–8 fails, attempt to delete the staging file and return the error. The final file is never partially written.

### 8.3 Concurrency

The substrate is single-writer. The library does not internally lock; it assumes the caller (Stream B's daemon) serializes writes. If a second writer attempts to write the same path concurrently, the rename will succeed for one and the other will see no error but its content is lost. This is acceptable because Stream B is the only writer.

### 8.4 Line ending normalization

On read, CRLF and CR are converted to LF in-memory. On write, only LF is emitted. The `.gitattributes` line `* text eol=lf` reinforces this in the repo.

### 8.5 Acceptance signals

- Round-trip: write 1,000 randomly-generated valid memories, read each back, assert struct equality.
- Crash safety: simulate a kill-9 between fsync and rename in 100 trials; final tree must always contain either the old version of every file or the new version, never partial.
- Routing test: a memory with `sensitivity: confidential` writes to `encrypted/<path>`, never to the plain path.
- Refusal test: `sensitivity: secret` returns `WriteError::SecretSensitivity` and disk is not touched (verify by file-mtime check before/after).

---

## 9. Validator

`frontmatter::validate(raw: FrontmatterRaw) -> ValidationResult` returns either a typed `Frontmatter` struct or a `ValidationReport` containing one or more `ValidationError`s.

### 9.1 Validation passes

Run in order, short-circuiting on first failure of pass 1, then collecting all failures in passes 2 and 3:

1. **Type pass.** Every required field present with correct primitive type. Enum values in domain. Datetime parses. Regex-constrained strings match.
2. **Cross-field pass.** All §6.8 cross-field constraints.
3. **Structural pass.** Sub-objects (`source`, `regression`, `privacy_scan`) recursively validated.

### 9.2 Error structure

```rust
struct ValidationReport {
    errors: Vec<ValidationError>,
    warnings: Vec<ValidationWarning>,
}

enum ValidationError {
    MissingRequired { field: String },
    WrongType { field: String, expected: String, found: String },
    EnumOutOfDomain { field: String, value: String, allowed: Vec<String> },
    RegexMismatch { field: String, value: String, pattern: String },
    CrossFieldViolation { rule: String, fields: Vec<String> },
    SecretSensitivity,                             // §6.8 rule 9
    InvalidId { value: String },
    InvalidSlug { value: String },
    DatetimeFormat { field: String, value: String },
    DurationFormat { field: String, value: String },
}

enum ValidationWarning {
    NullableNotExplicit { field: String },          // optional field missing entirely; should be `null`
    UnknownField { field: String },                 // forward-compat: don't error, but flag
    AuthorFormatLax { value: String },              // matches but is unusually structured
}
```

### 9.3 Forward compatibility

`UnknownField` is a *warning*, not an error. This is critical: a v0.2 daemon will write fields a v0.1 daemon doesn't know about. The v0.1 daemon must still be able to parse, validate, and re-serialize those memories without losing the unknown fields. The parser preserves unknown fields in a `_extras: BTreeMap<String, serde_yaml::Value>` and the serializer emits them in the canonical position immediately before type-specific fields.

This means the on-disk form might have field ordering differences across daemon versions. The merge driver tolerates this; canonical ordering is enforced only on writes from the current daemon.

### 9.4 Acceptance signals

- Every required field, every enum, every cross-field rule has at least one positive test (passes) and one negative test (fails with the specific expected error).
- 100 randomly-mutated valid memories (one field corrupted per mutation) produce exactly the expected error for that mutation.
- A v0.2-style memory with three unknown fields parses, validates with three `UnknownField` warnings, re-serializes with the unknown fields preserved.

---

## 10. SQLite indexer

### 10.1 Schema

```sql
-- memories: one row per .md file in the tree
CREATE TABLE memories (
    id              TEXT PRIMARY KEY,                -- frontmatter.id
    path            TEXT NOT NULL UNIQUE,             -- repo-relative path
    type            TEXT NOT NULL,
    scope           TEXT NOT NULL,
    namespace       TEXT,                              -- nullable for me/, agent/
    canonical_namespace_id TEXT,
    summary         TEXT NOT NULL,
    confidence      REAL NOT NULL,
    trust_level     TEXT NOT NULL,
    sensitivity     TEXT NOT NULL,
    status          TEXT NOT NULL,
    review_state    TEXT,
    requires_user_confirmation INTEGER NOT NULL,       -- 0 or 1
    created_at      TEXT NOT NULL,                     -- RFC 3339 string; collation BINARY for sort
    updated_at      TEXT NOT NULL,
    observed_at     TEXT,
    valid_from      TEXT,
    valid_until     TEXT,
    ttl             TEXT,                              -- ISO 8601 duration string
    author          TEXT NOT NULL,
    source_kind     TEXT NOT NULL,
    source_harness  TEXT,
    source_device   TEXT,
    body            TEXT NOT NULL,                     -- denormalized for FTS
    frontmatter_json TEXT NOT NULL,                    -- full frontmatter as JSON for round-trip
    file_mtime_ns   INTEGER NOT NULL,                  -- for change detection
    indexed_at      TEXT NOT NULL
);

CREATE INDEX idx_memories_type ON memories(type);
CREATE INDEX idx_memories_scope_namespace ON memories(scope, namespace);
CREATE INDEX idx_memories_status ON memories(status);
CREATE INDEX idx_memories_updated_at ON memories(updated_at);
CREATE INDEX idx_memories_sensitivity ON memories(sensitivity);

-- tags: many-to-many
CREATE TABLE memory_tags (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    tag       TEXT NOT NULL,
    PRIMARY KEY (memory_id, tag)
);
CREATE INDEX idx_memory_tags_tag ON memory_tags(tag);

-- entity links: many-to-many with label
CREATE TABLE memory_entities (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    label     TEXT NOT NULL,
    PRIMARY KEY (memory_id, entity_id)
);
CREATE INDEX idx_memory_entities_entity ON memory_entities(entity_id);

-- supersession edges: directed
CREATE TABLE memory_supersession (
    earlier_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    later_id   TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    PRIMARY KEY (earlier_id, later_id)
);
CREATE INDEX idx_supersession_later ON memory_supersession(later_id);

-- related edges: undirected, stored as canonical-ordered pair
CREATE TABLE memory_related (
    a_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    b_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    PRIMARY KEY (a_id, b_id),
    CHECK (a_id < b_id)
);

-- evidence rows
CREATE TABLE memory_evidence (
    memory_id    TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    quote        TEXT NOT NULL,
    ref          TEXT NOT NULL,
    weight       REAL NOT NULL,
    observed_at  TEXT,
    PRIMARY KEY (memory_id, seq)
);

-- regressions: type-specific row alongside memories
CREATE TABLE memory_regressions (
    memory_id            TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    error_string_regex   TEXT,
    stack_fingerprint    TEXT,
    tool_output_hash     TEXT,
    behavioral_marker    TEXT,
    fire_on_attempt      INTEGER NOT NULL,
    first_observed       TEXT NOT NULL,
    last_observed        TEXT NOT NULL,
    occurrence_count     INTEGER NOT NULL
);
CREATE INDEX idx_regressions_error_regex ON memory_regressions(error_string_regex) WHERE error_string_regex IS NOT NULL;
CREATE INDEX idx_regressions_stack_fp    ON memory_regressions(stack_fingerprint)  WHERE stack_fingerprint  IS NOT NULL;
CREATE INDEX idx_regressions_tool_hash   ON memory_regressions(tool_output_hash)   WHERE tool_output_hash   IS NOT NULL;

-- FTS5: full-text over summary + body
CREATE VIRTUAL TABLE memories_fts USING fts5(
    id UNINDEXED,
    summary,
    body,
    content='memories',
    content_rowid='rowid',
    tokenize = 'porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER memories_fts_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, id, summary, body) VALUES (new.rowid, new.id, new.summary, new.body);
END;
CREATE TRIGGER memories_fts_update AFTER UPDATE ON memories BEGIN
    UPDATE memories_fts SET summary = new.summary, body = new.body WHERE rowid = new.rowid;
END;
CREATE TRIGGER memories_fts_delete AFTER DELETE ON memories BEGIN
    DELETE FROM memories_fts WHERE rowid = old.rowid;
END;

-- vector embeddings (sqlite-vec extension)
-- See §15.1 for vector store decision; this DDL assumes sqlite-vec
CREATE VIRTUAL TABLE memory_embeddings USING vec0(
    id TEXT PRIMARY KEY,
    embedding FLOAT[768]            -- adjust to chosen model dimension
);

-- migrations metadata
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

INSERT INTO schema_migrations (version, applied_at) VALUES (1, datetime('now'));
```

### 10.2 Indexer behavior

The indexer is invoked by the file watcher (§11) with typed events. For each event:

- **Created** or **Modified**: read the file, validate (warnings allowed, errors abort the indexing of *that* file with a logged error and event), upsert into `memories`, replace rows in tag/entity/supersession/related/evidence/regression tables.
- **Deleted**: `DELETE FROM memories WHERE path = ?`. Cascading deletes handle the join tables.
- **Renamed**: update `path` on the existing row; do not touch other fields.

The indexer never holds the SQLite connection across `await` boundaries (Rust async). Index writes are wrapped in transactions per file event.

### 10.3 Reindex

`memory reindex` (CLI) walks the entire tree, computes a manifest of (path, mtime, hash), diffs against the index, applies the same per-file logic to converge. Always idempotent. Safe to run repeatedly.

Reindex also runs at daemon startup if `index.sqlite` is missing or `schema_migrations.version` is older than the daemon expects.

### 10.4 Index integrity invariants

The indexer enforces these on every write transaction (PRAGMA + checks):

- `memories.id` matches `frontmatter_json.id` parsed back.
- `memories.path` corresponds to a file that exists on disk at index time.
- For every row in `memory_supersession`, both `earlier_id` and `later_id` exist in `memories` (FK constraint).
- For every row in `memory_related`, the pair is canonically ordered.

Violations indicate indexer bugs, not data bugs; they raise loud errors to logs.

### 10.5 Acceptance signals

- 1,000-file fixture: index from empty, query each file by id, query by tag, query by entity, FTS query for known body strings; all return expected results.
- Mutation test: modify 100 random files (rewrite, delete, rename); index converges via watcher events and final state matches a fresh reindex.
- Schema migration test: open an index produced by an earlier migration version; daemon detects, reindexes, schema_migrations advances.

---

## 11. File watcher

### 11.1 Library

Use a cross-platform watcher with FSEvents (macOS) and inotify (Linux) backends. Reference choice: `notify` crate (Rust) with the `recommended_watcher` policy. Decision pinned in §15.

### 11.2 Watch root

Watch `~/.memory/` recursively, filtering out:

- `.git/` (anything inside).
- `~/.memoryd/` is *not* under `~/.memory/`, so it's naturally excluded.
- `tmp/` directories.
- Files with names matching `\.swp$`, `\.tmp$`, `~$`, `\.DS_Store$`.

### 11.3 Event coalescing

Raw FS events are noisy: an editor save can produce rename + create + modify in milliseconds. The watcher debounces per-path with a 100ms quiet period: it emits a single `FileChanged(path)` event after 100ms of no further events for that path. Deletions, renames, and creates each get a dedicated event after the same debounce.

### 11.4 Event types emitted

```rust
enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}
```

### 11.5 Self-write suppression

When the substrate's own `markdown::write` produces a file change, the watcher will see it. To avoid double-processing:

- The substrate maintains a short-lived (5-second) ring buffer of paths it just wrote.
- Watcher events whose path is in the ring buffer are dropped, with a counter incremented for observability.

### 11.6 Acceptance signals

- Touch a file via `echo > foo.md`; watcher emits exactly one `Modified` event within 200ms.
- `mv a.md b.md`; watcher emits exactly one `Renamed` event.
- `rm a.md`; watcher emits exactly one `Deleted` event.
- Substrate writes a file; watcher's self-write ring suppresses the event (test assertion: indexer is not re-invoked).
- 1000-file mass change (`touch *.md` in a directory) produces exactly 1000 events, each within the debounce window after the corresponding touch.

---

## 12. Event log

### 12.1 Format

Per-device append-only JSONL at `~/.memory/events/<device-id>.jsonl`. One JSON object per line, no trailing whitespace, newline-terminated.

### 12.2 Event schema

Every event carries:

```json
{
  "schema": 1,
  "id": "evt_01HX2A3B4C5D6E7F8G9HJK",
  "ts": "2026-04-24T13:14:15.123Z",
  "device": "dev_a1b2c3d4e5f60718",
  "session": "sess_claude_code_2026-04-24_001",
  "kind": "WriteCommitted",
  "data": { /* kind-specific */ }
}
```

Event ID is ULID (lexicographically sortable, time-prefixed). The substrate provides `events::next_id() -> EventId`.

### 12.3 Event kinds

| Kind | When | `data` shape |
|------|------|--------------|
| `WriteCommitted` | After successful `markdown::write` | `{memory_id, path, frontmatter_summary: {type, scope, namespace}}` |
| `WriteRefused` | Validator rejected a write attempt | `{path?, reason: ValidationError}` |
| `Deleted` | After successful delete | `{memory_id, path}` |
| `Renamed` | After successful rename | `{memory_id, from, to}` |
| `Tombstoned` | After tombstone applied | `{memory_id, tombstone_id, reason}` |
| `Superseded` | When a memory becomes superseded | `{earlier_id, later_id}` |
| `IndexUpdated` | Indexer applied a change | `{path, action: created|modified|deleted}` |
| `IndexFailed` | Indexer encountered an error | `{path, error}` |
| `MergeQuarantined` | Merge driver quarantined a file | `{path, reason}` |
| `CollisionRenamed` | ID collision resolved at merge | `{original_id, renamed_to, path}` |
| `GitCommitted` | Auto-commit succeeded | `{sha, summary}` |
| `GitFetched` | Fetch + merge cycle | `{remote, ahead, behind, conflicts}` |
| `WatcherSuppressed` | Self-write event suppressed | `{path, count_total}` |

### 12.4 Append semantics

Append is atomic per-line by virtue of POSIX write-append semantics for files opened with `O_APPEND`. The library opens the device's event log file with `O_APPEND` once at startup and reuses the descriptor.

`fsync` policy: every event is `fsync`d. The cost is real (millisecond-order on consumer SSDs) but acceptable for the write rates expected (maximum ~10 events/sec sustained, bursts up to ~100). If profiling shows this is a bottleneck in practice, batch-fsync at 50ms intervals is a future optimization.

### 12.5 Read

`events::read_log(device: DeviceId, from: Option<EventId>) -> impl Iterator<Item = Event>` streams events from a starting point. Other streams (notably Stream I) build subscriptions on top.

### 12.6 Multi-device union

When git fetch brings in another device's event log, the substrate union-iterates by ULID across all `events/*.jsonl` files. Sort order is global by ULID timestamp prefix.

### 12.7 Acceptance signals

- Append 10,000 events; read back; assert order matches append order, all events parse, no truncated lines.
- Crash test: kill -9 the writer between event N and event N+1; reopen log; events 1..N are intact, no half-written line.
- Multi-device test: two devices each write 100 events with overlapping timestamps; merged read iterates 200 events in correct global order.

---

## 13. Git operations

### 13.1 Repo init

`git::init(root)`:

1. `git init` at `root`.
2. Write `.gitattributes` with:
   ```
   * text eol=lf
   *.md merge=memory-frontmatter-merge
   events/*.jsonl merge=union
   substrate/**/*.jsonl merge=union
   tombstones/*.jsonl merge=union
   ```
3. Write a minimal `.gitignore` (none of `~/.memoryd/` is in the repo, but `.DS_Store` and editor backups need exclusion).
4. Set local git config:
   - `merge.memory-frontmatter-merge.driver` = `memory-merge-driver --base %O --ours %A --theirs %B --path %P`
   - `merge.memory-frontmatter-merge.name` = `Semantic frontmatter merge for memory files`
   - `core.autocrlf` = `false`
   - `pull.rebase` = `false` (we want merge commits to anchor the merge driver invocations)
5. Create `events/<device-id>.jsonl` (empty).
6. Initial commit: empty tree + dotfiles only.

### 13.2 Auto-commit

Triggered by event log appends of `WriteCommitted`, `Deleted`, `Renamed`, `Tombstoned`, `Superseded`. Coalesces with a 30-second debounce timer (configurable in `config.yaml`; default 30s).

When the timer fires:

1. Run `git status --porcelain` to enumerate changes.
2. Group by namespace (extracted from path: `me/`, `projects/<ns>/`, `agent/`, etc.).
3. Generate commit message:

   ```
   auto: <N> writes, <M> notes, <K> supersedes [<namespace>]

   - writes: mem_..., mem_..., mem_...
   - supersedes: mem_... -> mem_...
   - tombstones: mem_...
   - renames: mem_... <path-old> -> <path-new>

   memoryd-version: 0.1.0
   device: dev_a1b2c3d4
   ```

   If multiple namespaces, use `[multi]` and let the body itemize.

4. `git add -A` over the changes.
5. `git commit -m <message>` with `commit.gpgsign=false` for the auto-commit (signing is per-user choice configured separately; auto-commit must work without keychain unlock).
6. Append `GitCommitted { sha, summary }` to the event log.
7. Schedule background push if `config.yaml.sync.auto_push == true`.

### 13.3 Fetch + merge

Triggered every `config.yaml.sync.interval` (default 120s) when daemon is idle, or on demand:

1. `git fetch origin`.
2. If local is ahead: skip.
3. If remote has new commits: `git merge --no-ff origin/main`. The merge driver runs on conflicting `*.md` files automatically.
4. After merge, scan working tree for files containing the quarantine marker (set by merge driver on irreconcilable cases — see §14.5). For each, append a `MergeQuarantined` event.
5. Trigger a reindex pass over changed paths.
6. Append `GitFetched { remote, ahead, behind, conflicts }` event.

### 13.4 Push

Background, throttled. Default: push every 5 minutes if there are unpushed commits, configurable. Push failures (network, auth) are logged and retried with exponential backoff.

### 13.5 Acceptance signals

- After a sequence of writes, the auto-commit timer fires once and produces a single commit with a well-formed message containing all changes.
- Fetch + merge test: tree A and tree B both modify the same memory's `summary` field. After fetch + merge, the working tree shows the field-level merge result per the driver rules; both devices produce byte-identical merged files (run on each device, compare).
- Push retry: simulate network failure for 10s; push succeeds on next interval after recovery.

---

## 14. Frontmatter merge driver

### 14.1 Invocation

Git invokes the binary as:

```
memory-merge-driver --base <base-path> --ours <ours-path> --theirs <theirs-path> --path <pathname>
```

Where `<base>`, `<ours>`, `<theirs>` are temporary files git provides; `<pathname>` is the working-tree path being merged. Exit code 0 means resolved (driver writes the resolved content to `<ours-path>`); exit code 1 means conflict.

### 14.2 Algorithm

1. Read base, ours, theirs as Markdown+frontmatter.
2. If any side fails to parse: write a conflict-marked file to `<ours>` (using standard git conflict markers in the body), set frontmatter `status: quarantined`, exit 1.
3. **Frontmatter merge:**
   - For each field, apply the rule (§14.3).
   - Track per-field provenance for the diagnostic log.
4. **Body merge:**
   - Run textual 3-way merge (use `diff3`-style semantics; reuse a library like `imara-diff` or shell out to `git merge-file`).
   - On clean merge: take the merged body.
   - On conflict: include conflict markers in body, set frontmatter `status: quarantined`, set `review_state: pending`.
5. Re-validate the merged frontmatter. If validation fails (e.g., merged supersession produced a self-reference): set `status: quarantined`, attach a diagnostic note.
6. Write canonical YAML+body to `<ours>` (the path git uses as the resolution).
7. Exit 0 if no quarantine; exit 1 if quarantined.

### 14.3 Field-level merge rules

**Global tie-break rule:** wherever a rule below says "newer `updated_at` wins," the implicit tie-break is: if `ours.updated_at == theirs.updated_at` and the field values differ, quarantine. If the field values are equal, no conflict. `updated_at` ties are extremely rare in practice (sub-millisecond timestamps) and quarantine is the right safety valve.

| Field | Rule |
|-------|------|
| `id` | Must match across all three sides. If different: invariant violation, abort with non-zero exit and conflict markers (this should not happen and indicates a bug). |
| `type`, `scope`, `namespace`, `canonical_namespace_id` | Immutable; all three sides must agree. Disagreement → quarantine. |
| `summary`, `confidence`, `trust_level`, `sensitivity` | Newer `updated_at` wins; if tied, quarantine. |
| `status` | Poset: `tombstoned` > `archived` > `superseded` > `active` > `candidate` > `quarantined-by-merge`. The greater (further along the poset) wins. Irreconcilable transitions (both sides moved to different terminal states) → quarantine. |
| `review_state`, `requires_user_confirmation` | Newer `updated_at` wins. |
| `tags`, `aliases` | Set union. |
| `entities` | Union by `id` field; on collision, newer `updated_at` wins for the `label`. |
| `evidence` | Union by `(quote, ref)` pair. Preserve all quotes. |
| `supersedes`, `superseded_by`, `related` | Set union. |
| `created_at` | Always min (oldest). |
| `updated_at` | Always max (newest). |
| `observed_at`, `valid_from`, `valid_until` | Newer `updated_at` wins. |
| `ttl` | Newer `updated_at` wins. |
| `author` | Newer `updated_at` wins. |
| `source` | Newer `updated_at` wins (entire object replaced). |
| `retrieval_policy`, `write_policy` | Field-level newer wins per inner field. |
| `regression` | Special: `occurrence_count` is summed across base→ours and base→theirs deltas applied to base. `last_observed` is max. `first_observed` is min. Detection signature follows newer `updated_at`. |
| `privacy_scan` | Newer `ran_at` wins. If models differ, keep newer and emit a warning event. |
| Unknown fields (`_extras`) | Per-key newer-`updated_at` wins. |

### 14.4 Status poset clarification

The full poset, drawn:

```
       tombstoned
           ▲
           │
        archived
           ▲
           │
       superseded
           ▲
           │
         active
           ▲
           │
       candidate
           ▲
           │
   quarantined-by-merge
```

A move "up" the poset is monotonic: once a memory is `tombstoned`, no merge can resurrect it. If one side has `tombstoned` and the other has `active`, the result is `tombstoned`. If both sides moved to `tombstoned` with different reason metadata, the merged result is `tombstoned` with both reasons preserved in a `tombstone_reasons` array (extends schema; this is one of the open decisions in §15).

Two sides moving to non-comparable terminal states (e.g., one `tombstoned` and one `archived` on the same merge) is impossible because `tombstoned > archived` and the higher wins.

### 14.5 Quarantine marker

When the driver produces a quarantine, the resulting file has:

- Frontmatter `status: quarantined`.
- Frontmatter `review_state: pending`.
- An auto-injected `_merge_diagnostics` field listing the conflicting fields.
- Body retains git conflict markers if the body merge conflicted.
- Driver exits 1 (so the fact of quarantine is visible to the caller).

A trailing block comment in the body summarizes the quarantine reason in human language so a `git diff` review surfaces it.

### 14.6 Acceptance signals

- 30+ canonical merge scenarios as fixture pairs (base, ours, theirs, expected): scalar bump, array union, status promotion, status conflict, evidence accumulation, regression occurrence sum, supersession union, privacy scan version mismatch, etc. Driver produces the expected output byte-for-byte.
- Quarantine scenarios (5+): each produces a quarantine marker, `_merge_diagnostics` field, exit code 1.
- Fuzz: 1,000 random base/ours/theirs triples generated by mutation; driver never panics, always exits 0 or 1 with valid output.

---

## 15. Open decisions (need Trey's call before lock)

These are real decisions where I have a recommendation but want sign-off before locking.

### 15.1 Implementation language

**Recommendation: Rust.**

Reasoning: the merge driver is a small static binary that needs to be on PATH everywhere; Rust's static linking is ideal. The substrate library will be linked into the daemon (Stream B); same-language sharing avoids FFI. SQLite/yaml/markdown/git/notify ecosystems are mature in Rust. Performance matters for the indexer at scale. And Codex has demonstrated good Rust output in recent sessions.

Alternatives considered: Go (also fine; slightly weaker yaml ecosystem, simpler concurrency model), Node (lots of glue available, but the merge driver as a Node binary is heavy and slow to start). Python is a non-starter for the merge driver (interpreter startup dominates merge time at scale).

**Decision needed:** confirm Rust, or push back with a rationale for another choice.

### 15.2 Vector store

**Recommendation: sqlite-vec.**

Reasoning: keeps the index in one file, one connection, one transactional unit. Avoids the operational footprint of a second store. Good enough for the scale (~10K to 100K memories) and Stream E's recall queries. The v0.1 spec called this out as "sqlite-vec ... or pgvector-adjacent local store depending on availability" — sqlite-vec has shipped stable 0.1.x as of 2025 and is the simplest path.

Alternatives: lancedb (richer features, separate process, more ops surface), per-namespace flat files with HNSW (over-engineered for v0.1).

**Decision needed:** confirm sqlite-vec, or accept lancedb tradeoffs.

### 15.3 Embedding model and dimension

The vector column needs a fixed dimension. v0.1 spec mentions `embeddinggemma-300m-qat-Q8_0.gguf` as default; that's 768-dim.

**Recommendation:** lock at 768-dim for v0.1. Re-embedding all memories on a model swap is acceptable cost (run as a one-time migration).

**Decision needed:** confirm 768; confirm Stream B will own the actual embedder; confirm that re-embedding migrations are acceptable.

### 15.4 Encrypted-tier write order

The spec says `confidential`/`personal` route to `encrypted/` and Stream D's commit hook encrypts before commit. Question: does Stream A write *plaintext* under `encrypted/` and rely on Stream D's hook, or does Stream A defer the write until Stream D produces ciphertext?

**Recommendation: defer the write to Stream D for `confidential`/`personal`.** Stream A produces a "pending sensitive write" record on its event log; Stream D consumes it, encrypts, writes ciphertext to the canonical path. Reason: writing plaintext to disk even momentarily is a leak risk if the daemon crashes between Stream A's write and Stream D's hook.

**Decision needed:** confirm deferred-write model, or accept the plaintext-then-encrypt risk in exchange for simplicity.

### 15.5 Filename strategy for ID-based vs. slug-based memories

Some paths use `<id>.md` (e.g., `agent/patterns/<id>.md`). Others use slugs (e.g., `me/knowledge/<topic-slug>.md`). The schema's `id` is canonical regardless. But when a slug-based file has its slug changed (rename), what happens?

**Recommendation:** filename and frontmatter `id` are loosely coupled. The indexer keys on `id`, not path. Renames update the row's `path` field. Slugs are encouraged to be stable but not enforced.

For ID-based paths, the filename *must* match the frontmatter id, validated by the tree validator.

**Decision needed:** confirm the loose coupling for slug-based paths.

### 15.6 `tombstone_reasons` array

The merge driver §14.4 mentions extending the schema with a `tombstone_reasons` array to preserve both reasons when both sides tombstone with different rationales. Adding a field to v0.1 schema needs a call.

**Recommendation:** add it. It's a cheap field, makes the merge correct, costs nothing in normal operation.

**Decision needed:** approve schema addition for v0.1.

### 15.7 Sequence allocation random offset

§7.3 proposes seeding `seq` at startup with a random offset to reduce collision probability. This means seq numbers won't necessarily start at 1 each day. Mostly fine, but worth noting because it changes the human-readable feel of IDs.

**Recommendation:** seed in `[0, 100)`. Cheap, dramatically reduces collisions.

**Decision needed:** approve, or accept the higher-collision-rate alternative of starting at 1.

### 15.8 `gitattributes` for substrate JSONL

I've drafted `substrate/**/*.jsonl merge=union` in §13.1. This means substrate fragment files merge by line union across devices. That's the behavior we want (each device contributes fragments; merge is concatenation). But it means the file isn't sorted; readers must sort by ULID at read time.

**Recommendation:** keep `merge=union`. Sorting at read time is cheap.

**Decision needed:** confirm.

---

## 16. Configuration

`~/.memory/config.yaml`:

```yaml
schema_version: 1

device:
  id: dev_a1b2c3d4e5f60718         # generated at first init
  name: trey-mbp-2025               # human-readable label

paths:
  memory_root: ~/.memory
  runtime_root: ~/.memoryd

sync:
  remote: git@github.com:treygoff/memory.git
  enabled: true
  fetch_interval: 120                # seconds
  push_interval: 300                 # seconds
  auto_push: true
  auto_commit_debounce: 30           # seconds

embeddings:
  provider: local-gemma              # placeholder; Stream B owns
  dimension: 768
  model_ref: embeddinggemma-300m-qat-Q8_0

privacy:
  filter_enabled: false              # opt-in; Stream D owns
  age_recipients: []                 # for encrypted tier; Stream D owns

logging:
  level: info                        # debug | info | warn | error
  file: ~/.memoryd/logs/memoryd.log
```

The substrate exposes `config::load(path) -> Result<Config>`. Fields documented above with defaults; missing `device.id` triggers generation on first load and the file is rewritten.

Environment overrides (for tests and CI): any field can be overridden by `MEMORY_<UPPER_SNAKE>` (e.g., `MEMORY_PATHS__MEMORY_ROOT=/tmp/test-memory`).

### 16.1 Acceptance signals

- Round-trip: load config, re-serialize, byte-compare.
- Missing required fields produce a structured error naming the field.
- Env override: setting `MEMORY_SYNC__ENABLED=false` disables sync regardless of file content.

---

## 17. Public Rust API surface

This is the crate's exported surface. Stream B will be the primary consumer; other streams may also depend on the substrate.

```rust
// memory_substrate::tree
pub fn validate(root: &Path) -> ValidationReport;
pub fn init(root: &Path, device_id: DeviceId) -> Result<(), InitError>;

// memory_substrate::frontmatter
pub fn parse(yaml: &str) -> Result<FrontmatterRaw, ParseError>;
pub fn validate(raw: FrontmatterRaw) -> Result<Frontmatter, ValidationReport>;
pub fn serialize(fm: &Frontmatter) -> String;       // canonical form

// memory_substrate::markdown
pub fn read(path: &Path) -> Result<Memory, ReadError>;
pub fn write(memory: &Memory) -> Result<WriteOutcome, WriteError>;
pub fn delete(memory_id: &MemoryId) -> Result<(), DeleteError>;

// memory_substrate::ids
pub fn next() -> Result<MemoryId, IdError>;
pub fn parse(s: &str) -> Result<MemoryId, IdError>;

// memory_substrate::index
pub struct Index { /* opaque */ }
impl Index {
    pub fn open(path: &Path) -> Result<Self, IndexError>;
    pub fn upsert(&self, memory: &Memory) -> Result<(), IndexError>;
    pub fn delete(&self, id: &MemoryId) -> Result<(), IndexError>;
    pub fn rename(&self, id: &MemoryId, new_path: &Path) -> Result<(), IndexError>;
    pub fn update_embedding(&self, id: &MemoryId, vec: &[f32]) -> Result<(), IndexError>;
    pub fn query<R>(&self, f: impl FnOnce(&Connection) -> R) -> R;       // raw SQL access
    pub fn reindex(&self, root: &Path) -> Result<ReindexReport, IndexError>;
}

// memory_substrate::watcher
pub struct Watcher { /* opaque */ }
impl Watcher {
    pub fn watch(root: &Path) -> Result<Self, WatchError>;
    pub fn next_event(&mut self) -> Option<FileEvent>;
    pub fn suppress_self_write(&self, path: &Path);
}

// memory_substrate::events
pub fn append(log_path: &Path, event: Event) -> Result<(), EventError>;
pub fn read_log(log_path: &Path, from: Option<EventId>) -> impl Iterator<Item = Event>;
pub fn next_id() -> EventId;

// memory_substrate::git
pub struct Repo { /* opaque */ }
impl Repo {
    pub fn open(root: &Path) -> Result<Self, GitError>;
    pub fn init(root: &Path) -> Result<Self, GitError>;
    pub fn auto_commit(&self, events: &[Event]) -> Result<Option<CommitSha>, GitError>;
    pub fn fetch_and_merge(&self) -> Result<FetchOutcome, GitError>;
    pub fn push(&self) -> Result<PushOutcome, GitError>;
}

// memory_substrate::config
pub fn load(path: &Path) -> Result<Config, ConfigError>;
```

Supporting types are all defined and exported: `Memory`, `Frontmatter`, `FrontmatterRaw`, `MemoryId`, `DeviceId`, `EventId`, `Config`, `Event`, `FileEvent`, `ValidationReport`, `WriteOutcome`, `ReindexReport`, `FetchOutcome`, `PushOutcome`, `CommitSha`, plus the error enums (`ParseError`, `ValidationError`, `ReadError`, `WriteError`, `DeleteError`, `IdError`, `IndexError`, `WatchError`, `EventError`, `GitError`, `ConfigError`, `InitError`).

The crate uses `thiserror` for error types and `serde` for serialization. Tokio is the async runtime where async is needed (watcher, git operations).

---

## 18. Test plan

### 18.1 Unit tests (per module)

Each module ships with unit tests covering:

- All `pub` functions, happy path.
- Every typed error, with at least one test producing it.
- Boundary conditions for numeric ranges, regex patterns, enum domains.

Coverage target: 85% line coverage on the substrate crate, 90% on the merge driver.

### 18.2 Property tests

Using `proptest`:

- Frontmatter round-trip: serialize-then-parse equals original.
- Validator: any valid frontmatter validates; any frontmatter with a known violation fails with the expected error.
- ID format: any ID matching the regex parses; any non-matching string fails.
- Merge driver: idempotence (merge(A, A, A) == A), commutativity where applicable (merge(base, ours, theirs) == merge(base, theirs, ours) for symmetric operations).

### 18.3 Integration tests

- `tests/init_and_write.rs`: init a tree, write 100 memories of varied types, verify on-disk form, verify index state.
- `tests/watcher_indexer.rs`: spawn watcher + indexer; modify files; assert index converges.
- `tests/multi_device_merge.rs`: simulate two devices via two clones of one repo; both make changes; merge; assert convergence.
- `tests/crash_safety.rs`: kill writer mid-write; assert tree integrity.

### 18.4 Merge driver fixtures

`fixtures/merge/<scenario>/{base.md, ours.md, theirs.md, expected.md, expected_exit}`. At least 30 scenarios covering each rule in §14.3 plus quarantine cases. Test runner walks the fixture dir, invokes the driver, compares output.

### 18.5 Acceptance for Stream A overall

Stream A is "done" when:

1. All §5–§14 acceptance signals pass.
2. `cargo test --workspace` runs in under 60 seconds and passes.
3. The merge driver binary, installed to PATH, handles real `git merge` invocations correctly on a test repo with conflicting commits across two simulated devices.
4. A 10K-memory load test: indexer ingests 10,000 memories in under 60 seconds; queries by id, tag, entity, and FTS each return in under 50ms p95.
5. `memory init` produces a valid tree that passes `tree::validate(root)` clean.
6. Independent code review by Claude with no blocking findings (per the workflow you and I agreed: Codex implements, I review).

---

## 19. Risks

### 19.1 Merge driver complexity

The driver is the most complex piece. The poset rules, the regression occurrence-count merge, the privacy_scan model-version handling, the `_extras` forward-compat — each is small but they compose. Bugs here cause silent data loss across devices.

**Mitigation:** the fixture suite is the primary defense. Every rule needs at least three fixtures: a base case, an edge case, a "should quarantine" case. Property tests for symmetric operations. The quarantine path is a safety valve; better to over-quarantine and let humans review than to over-merge and lose data.

### 19.2 SQLite + watcher race

A naive implementation could index a half-written file if the watcher fires before the writer's `fsync`. The substrate's atomic write (tmp + rename) prevents this — readers always see whole files — but the watcher will fire on the rename, and the indexer must handle "file exists, read, parse, validate" without assuming the write is complete. With atomic rename, this is fine: the file is either old or new, never partial.

The actual race: substrate's self-write suppression (§11.5) uses a 5-second window. If the indexer is lagging more than 5 seconds, the suppression won't fire and the indexer will redo the same write. Idempotent indexing (upsert by id) makes this benign but wastes work.

**Mitigation:** widen the suppression window if lag is observed. Add a counter; if it ever fires, surface in observability.

### 19.3 Forward compatibility drift

If a v0.2 daemon adds a required field, a v0.1 daemon will refuse to read those memories — breaking the user's tree on a partial upgrade.

**Mitigation:** schema evolution in v0.x is *additive only* for required fields. New required fields can only be introduced with a `schema_version` bump and a one-time migration that fills defaults. The substrate's schema_version field (§16) gates this. v0.1 ships with `schema_version: 1`; future required fields require `schema_version: 2` with a migration.

### 19.4 Git merge driver not invoked

If a user clones the repo without running `memoryd init`, the local git config doesn't have `merge.memory-frontmatter-merge.driver` set, so git falls back to default text merge — which produces conflict markers and breaks the substrate.

**Mitigation:** the daemon detects this on startup (checks `git config --get merge.memory-frontmatter-merge.driver`) and refuses to run if the config is missing, prompting the user to run `memory link-config`. This is a Stream B concern but Stream A produces the link-config helper.

### 19.5 Vector store dimension lock-in

If we lock at 768-dim and later switch models to a 1024-dim model, the vector column needs to be re-typed. sqlite-vec doesn't support `ALTER TABLE` on virtual tables; we'd need to rebuild.

**Mitigation:** rebuild is acceptable as a one-time op. Add `memory reindex --rebuild-vectors` for this. Cost is bounded by memory count, not history.

### 19.6 Path case-insensitivity

A user who has macOS APFS (case-insensitive default) and shares a repo with someone on Linux ext4 (case-sensitive) could create files that conflict on the macOS side. The tree validator catches this on startup, but the damage is already done.

**Mitigation:** validator runs at startup *and* before every write. If a write would create a case-insensitive collision with an existing file, refuse the write with `WriteError::CaseCollision`.

---

## 20. Implementation phasing within Stream A

The four sub-items in v0.1 §19 Stream A map roughly to phases. Suggested order:

**Phase 1: Tree, schema, validator, file I/O.**
- Deliverables: `tree::layout`, `frontmatter::schema/parser/validator`, `markdown::file`, `ids`, `config`.
- Dependencies on later phases: none.
- Validation: §5.5, §6.10, §7.4, §8.5, §9.4, §16.1 acceptance signals.

**Phase 2: Index + watcher.**
- Deliverables: `index::*`, `watcher`.
- Depends on: Phase 1 for the `Memory` type.
- Validation: §10.5, §11.6 acceptance signals.

**Phase 3: Event log + git auto-commit.**
- Deliverables: `events::*`, `git::repo`, `git::auto_commit`.
- Depends on: Phase 2 for triggering auto-commit on indexer events.
- Validation: §12.7, §13.5 acceptance signals.

**Phase 4: Merge driver.**
- Deliverables: `memory-merge-driver` binary.
- Depends on: Phase 1 (validator and parser for frontmatter manipulation).
- Validation: §14.6 acceptance signals, §18.4 fixtures.

**Phase 5: Integration + load tests.**
- Deliverables: §18.3, §18.5 acceptance.
- Depends on: all prior.

Phases 2 and 4 can be parallelized after Phase 1. Phases 3 and 4 can also run in parallel. So a reasonable build order is: Phase 1 (sequential) → Phases 2/3/4 in parallel → Phase 5.

---

## 21. Acceptance criteria (Stream A overall)

Codex's job is done when all of the following hold. Claude's code review must verify each:

1. `cargo test --workspace --release` passes on macOS arm64 and Linux x86_64.
2. The merge driver binary is single-file, statically linked, < 8 MB, starts in < 50 ms.
3. The substrate library has no `unsafe` code outside of FFI to SQLite/git2.
4. All §5–§14 acceptance signals pass with documented test names traceable to spec section.
5. Fuzzing the merge driver (`cargo fuzz` for 10 minutes) produces no panics.
6. Load test: index 10,000 memories from cold; total time < 60 seconds; memory peak < 200 MB.
7. Multi-device test (two clones of a test repo, scripted concurrent edits, merge) produces byte-identical converged trees on both clones.
8. Public API surface (§17) matches what's documented; no extra exported types or functions; all docs comments populated.
9. `clippy --all-targets -D warnings` passes.
10. The tree validator runs in < 200ms on a 10K-memory tree.

When Codex believes Stream A is done, the handoff note to Claude (review) lists every acceptance signal with the file:line of the test that proves it.

---

## 22. What this spec doesn't decide

The §15 list is the explicit "needs Trey's call" surface. Everything else in this spec is locked-in for v0.1 implementation. If you find ambiguity outside §15 during review or implementation, treat it as a spec bug and escalate before guessing.

---

*End of Stream A — Core Substrate Spec v0.1.*
