# Stream E Passive Recall Spec v0.4

**Status:** final implementation contract for Stream E passive recall. Supersedes `stream-e-passive-recall-v0.3.md`.
**Date:** 2026-04-30.
**Sources:** `docs/specs/system-v0.1.md` section 10 and the shipped Stream A-D contracts.
**Non-source:** older Stream A drafts and Stream C/D review notes are historical unless they describe a still-shipped API surface.

**Revision goal (v0.3 → v0.4):** close two correctness gaps surfaced during plan review before implementation begins.

1. **§4.2 git-remote canonicalization is now URL-form-agnostic.** SSH, HTTPS, and git-protocol clones of the same upstream must produce identical `canonical_id` so that `namespaces_in_scope` and recall blocks converge across teammates regardless of their preferred clone URL. v0.3's "trim whitespace and strip trailing `.git`" rule was insufficient and would silently bifurcate project namespaces across SSH/HTTPS clones of the same repo.
2. **§3.3 `RecallOmission` gains optional `alias` and `colliding_ids` fields, and §7 alias-collision rule emits one omission per collision.** v0.3's omission shape lost the relationship structure of an alias collision: from `{ id, section, reason: "ambiguous_alias" }` alone, no consumer could reconstruct which alias collided or which other ids were in the same collision set. v0.4 emits exactly one `RecallOmission` per collision with the alias and the colliding entity ids populated. Both new fields are `skip_serializing_if`-default so the wire shape stays JSON-additive for tolerant clients.

The version string in policy/manifest/recall-block attributes bumps to `stream-e-v0.4`.

**Revision goal (v0.2 → v0.3):** close final review gaps before implementation planning.

1. Remove stale v0.1/v0.2 labels from normative examples and deferrals.
2. Align `delta-block` acceptance with the v0.2 wrapper contract: no-match emits `<memory-delta empty="true" />`.
3. Add `omitted_count` and `omitted_truncated_count` to `RecallExplanation` DTOs so omission bounding is implementable.
4. Specify exact Stream A SQLite/index semantics for `MemoryQuery`'s new namespace and passive-recall filters.
5. Define the additive `StatusResponse.recall` counter schema.
6. Pin `safe_plaintext_fragment` decisions to existing Stream D storage-action semantics.

Stream E turns the existing daemon from a queryable memory system into a
memoryful startup system. It implements `memory_startup`, deterministic recall
block assembly, entity/alias matching, and hook/CLI output shapes while
preserving the Stream A storage contract, Stream C governance lifecycle, and
Stream D privacy boundary.

## 1. Scope and dependency boundaries

Stream E owns:

- `memory_startup` MCP and daemon protocol implementation;
- `memoryd recall startup-block` CLI output for hook-based harnesses;
- `memoryd recall delta-block` CLI output for per-turn hook-based deltas;
- session binding from caller context (`cwd`, `session_id`, `harness`);
- project binding and namespace selection for recall;
- entity and alias resolution from canonical memory frontmatter;
- deterministic ranking, budgeting, trimming, and explanation metadata;
- privacy-safe handling of encrypted and metadata-only memories;
- exclusion of quarantined, candidate, superseded, tombstoned, archived, and
  recall-disabled memories from passive recall content.

Stream E does not own:

- canonical memory mutation, index writes, event-log append, merge, or git sync
  mechanics; those remain Stream A;
- write governance, review approval/rejection, contradiction tiebreaking, or
  tombstone enforcement; those remain Stream C;
- classification, encryption, decryption, reveal authorization, masking session
  internals, or privacy key management; those remain Stream D;
- dreaming, synthesis promotion, cleanup, or uncomfortable-question generation;
  those remain Stream F;
- dashboard UI, notification routing, or eval scoring; those remain Stream G/H;
- live peer presence and claim locks beyond read-only pending-attention counts;
  those remain Stream I.

Stream E must not create a hidden second persistence layer. It may build
request-local in-memory indexes from Stream A query/read APIs, but persisted
state remains canonical Markdown/frontmatter plus Stream A's derived SQLite
index.

### 1.1 Cross-stream surface changes required by Stream E

Implementation of this spec lands two small surface additions on already-shipped
streams. They are part of the Stream E v0.3 contract; do not implement Stream E
without them.

**Stream A — `MemoryQuery` extension (spec §16.4 amendment):**

```rust
pub struct MemoryQuery {
    pub id: Option<MemoryId>,
    pub tag: Option<String>,
    pub include_metadata_only: bool,
    // New in Stream E v0.3:
    pub status: Option<MemoryStatus>,           // active, pinned, candidate, quarantined, tombstoned
    pub namespace_prefix: Option<String>,       // "me", "project:proj_<id>", "agent"
    pub passive_recall_only: bool,              // index-side filter on retrieval_policy.passive_recall
    pub updated_since: Option<DateTime<Utc>>,   // index-side recency filter
}
```

The new fields must be served from the existing SQLite index (no full-table
hydration). Defaults preserve current behavior.

Index/schema semantics:

- `status` filters the existing `memories.status` column using the serialized
  `MemoryStatus` value.
- `updated_since` filters the existing `memories.updated_at` column with
  inclusive `>=` semantics.
- `passive_recall_only` requires a new `memories.passive_recall INTEGER NOT
  NULL DEFAULT 1` column populated from
  `frontmatter.retrieval_policy.passive_recall` during every index upsert and
  full reindex. Filtering `frontmatter_json` with ad hoc JSON extraction is not
  acceptable for the Stream E perf gate.
- `namespace_prefix` is a stable synthetic filter over existing frontmatter
  fields:
  - `"me"` matches `scope = "user"`.
  - `"agent"` matches `scope = "agent"`.
  - `"project:<canonical_id>"` matches `scope = "project"` and
    `canonical_namespace_id = <canonical_id>`.
  - `"org:<canonical_id>"` matches `scope = "org"` and
    `canonical_namespace_id = <canonical_id>`.
  - any other prefix returns `invalid_query` from Stream A and maps to
    `invalid_request` at the daemon boundary.

Stream A index migration must add indexes that support the new filters without
scanning the memory table: at minimum `(status, passive_recall, updated_at)` and
`(scope, canonical_namespace_id, status, passive_recall, updated_at DESC)`.

**Stream D — `safe_plaintext_fragment` public helper:**

```rust
// crates/memory-privacy/src/lib.rs
pub fn safe_plaintext_fragment(
    classifier: &DeterministicPrivacyClassifier,
    fragment: &str,
) -> SafeFragmentDecision;

pub enum SafeFragmentDecision {
    Allow,                          // emit fragment as-is
    OmitEncryptedBodyHidden,        // hits Refuse; omit
    OmitReviewPending,              // hits EncryptAtRest; omit pending review
}
```

This is the deterministic helper Stream E calls before emitting any
recall-explanation prose, hook diagnostic message, or echoed CLI argument inside
an error. It must not allocate persistent state and must not call
`memory_reveal`. Stream D owns the implementation and tests; Stream E only
consumes.

Decision mapping is exactly tied to existing Stream D routing:

- no spans, URL-only spans, date-only spans, or final
  `PrivacyStorageAction::Plaintext` -> `Allow`;
- any `PrivacyLabel::Secret`, caller-secret tier, SSN/Luhn-valid card,
  credential/private-key/JWT/high-entropy secret, or final
  `PrivacyStorageAction::Refuse` -> `OmitEncryptedBodyHidden`;
- account number, private address, private email, private person, private
  phone, caller confidential/personal tier, or final
  `PrivacyStorageAction::EncryptAtRest` -> `OmitReviewPending`.

If multiple labels are present, the stricter result wins:
`OmitEncryptedBodyHidden > OmitReviewPending > Allow`.

Both surface additions are minimal, behavior-additive, and require updating
`docs/api/stream-a-public-api.md` and `docs/api/stream-d-privacy-api.md`
alongside Stream E's own API doc.

## 2. Safety invariants

1. **Recall is read-only.** `memory_startup` and `delta-block` must not create,
   mutate, promote, approve, reject, supersede, tombstone, decrypt, or reveal a
   memory.
2. **No encrypted plaintext in recall.** Stream E must never call
   `memory_reveal` and must never include `MemoryContent::Ciphertext` bytes or a
   decrypted body in a recall block. Encrypted memories may contribute only
   safe metadata already exposed through Stream A frontmatter and Stream D safe
   descriptors.
3. **Governance lifecycle is authoritative.** Passive recall content may include
   only memories whose status is `active` or `pinned`, whose
   `retrieval_policy.passive_recall` is true, and whose write/review policy does
   not require unresolved human review.
4. **Tombstoned and superseded records do not teach.** Tombstoned records are
   never included. Superseded records are excluded from recall content; active
   replacements may mention the supersession chain in explanation metadata.
5. **Candidates and quarantines are attention, not truth.** Candidate,
   quarantined, and pending-review memories may affect `<pending-attention>`
   counts, but their claims must not be emitted as recall facts.
6. **Token budget is deterministic.** Budgeting uses a deterministic estimator
   until a real tokenizer is explicitly configured: `estimated_tokens =
   ceil(utf8_byte_len / 4)`. Tests assert this estimator.
7. **Output is stable for cacheability.** Given the same repo state, request
   context, budget, and clock fixture, Stream E emits byte-identical recall
   blocks.
8. **Errors are typed.** Protocol errors use stable `code`, `message`, and
   `retryable`; callers must not parse free-form guidance to detect failures.

## 3. Public surfaces

### 3.1 MCP `memory_startup`

The MCP manifest must replace the current placeholder schema with this shape:

```json
{
  "cwd": "/Users/treygoff/Code/agent-memory",
  "session_id": "sess_abc123",
  "harness": "codex",
  "harness_version": "0.0.0",
  "include_recent": true,
  "since_event_id": null,
  "budget_tokens": 3600
}
```

Required fields:

- `cwd`: absolute path to the harness working directory.
- `session_id`: caller-scoped session identifier. Empty strings are invalid.
- `harness`: stable harness id, for example `codex`, `claude-code`,
  `cursor`, or `mcp-generic`.

Optional fields:

- `harness_version`: version string if known.
- `include_recent`: default `true`; if false, recent-memory and recent-decision
  sections are omitted.
- `since_event_id`: reserved for future event-based deltas. v0.3 accepts null
  or absent only; non-null values return `not_implemented` so callers do not
  assume event deltas exist.
- `budget_tokens`: inclusive range `512..=8000`; default `3600`.

The current legacy MCP request shape `{ "include_recent": true }` is not
sufficient for production recall because it lacks binding context. **The legacy
shape is removed in Stream E v0.3.** Requests missing `cwd`, `session_id`, or
`harness` return `invalid_request` with no compatibility shim. There is no
adapter-context injection path — every caller (Claude Code hook, Codex hook,
Cursor rule, generic MCP client) is responsible for supplying the three required
fields. This deliberately keeps the MCP forwarder thin and side-effect-free.

### 3.2 Daemon protocol request

Add a daemon protocol request variant:

```rust
RequestPayload::Startup {
    cwd: String,
    session_id: String,
    harness: String,
    harness_version: Option<String>,
    include_recent: bool,
    since_event_id: Option<String>,
    budget_tokens: Option<usize>,
}
```

MCP `memory_startup` must forward to this daemon request instead of returning
the current structured `not_implemented` error.

### 3.3 Response payload

Add a daemon response payload:

```rust
ResponsePayload::Startup(StartupResponse)
```

Rust DTO shape:

```rust
struct StartupResponse {
    session_binding: SessionBinding,
    recall_block: String,
    budget_used_tokens: usize,
    recall_explanation: RecallExplanation,
    guidance: String,
}

struct RecallExplanation {
    budget_tokens: usize,
    budget_used_tokens: usize,
    policy: String,
    sections: Vec<RecallSectionExplanation>,
    omitted: Vec<RecallOmission>,
    omitted_truncated_count: u32,
}

struct RecallSectionExplanation {
    name: String,
    selected_ids: Vec<String>,
    matched_entities: Vec<String>,
    budget_used_tokens: usize,
    omitted_count: u32,
}

struct RecallOmission {
    id: Option<String>,
    section: String,
    reason: OmissionReason,
    /// Populated only when `reason == AmbiguousAlias`; carries the surface
    /// form of the alias that collided. `skip_serializing_if = "Option::is_none"`
    /// so unrelated reasons stay JSON-clean for tolerant clients.
    alias: Option<String>,
    /// Populated only when `reason == AmbiguousAlias`; carries every entity id
    /// that the alias resolved to within the active namespace set. Sorted
    /// lexicographically for deterministic output. `default` + `skip_serializing_if = "Vec::is_empty"`.
    colliding_ids: Vec<String>,
}

#[serde(rename_all = "snake_case")]
enum OmissionReason {
    BudgetExhausted,
    StatusExcluded,
    PassiveRecallDisabled,
    ReviewPending,
    EncryptedBodyHidden,
    AmbiguousAlias,
    NamespaceOutOfScope,
    Superseded,
    Tombstoned,
}
```

Serialized response shape:

```json
{
  "id": "req-startup",
  "result": {
    "success": {
      "startup": {
        "session_binding": {
          "session_id": "sess_abc123",
          "harness": "codex",
          "harness_version": "0.0.0",
          "cwd": "/Users/treygoff/Code/agent-memory",
          "project": {
            "canonical_id": "proj_<sha256>",
            "alias": "agent-memory",
            "resolved_via": "git_remote"
          },
          "namespaces_in_scope": ["me", "project:proj_<sha256>", "agent"]
        },
        "recall_block": "<memory-recall>...</memory-recall>",
        "budget_used_tokens": 1420,
        "recall_explanation": {
          "budget_tokens": 3600,
          "budget_used_tokens": 1420,
          "policy": "stream-e-v0.4",
          "sections": [],
          "omitted": [],
          "omitted_truncated_count": 0
        },
        "guidance": "Use this as startup context; call memory_search for follow-up lookup."
      }
    }
  }
}
```

`recall_block` is a string because harnesses inject it as text. Structured
metadata lives in `recall_explanation` and must not contain hidden plaintext
that is absent from `recall_block`.

### 3.4 CLI surfaces

Add CLI commands under `memoryd recall`:

```bash
memoryd recall startup-block --repo . --runtime .memoryd \
  --cwd /Users/treygoff/Code/agent-memory \
  --session-id sess_abc123 \
  --harness codex \
  --harness-version 0.0.0 \
  --budget-tokens 3600

memoryd recall delta-block --repo . --runtime .memoryd \
  --cwd /Users/treygoff/Code/agent-memory \
  --session-id sess_abc123 \
  --harness claude-code \
  --message "Fix the failing OAuth callback test" \
  --budget-tokens 400
```

`startup-block` writes exactly one `<memory-recall>...</memory-recall>` block to
stdout. `delta-block` writes exactly one `<memory-delta>...</memory-delta>`
block to stdout on every successful run; on no-match it writes
`<memory-delta empty="true" />`. Downstream tooling can therefore parse stdout
unconditionally without branching on emptiness. Both commands return non-zero
for typed protocol errors and must not print debug logs to stdout. Diagnostics
go to stderr.

Exit codes:

- `0` — block (possibly the empty wrapper) printed to stdout cleanly.
- `1` — `invalid_request` (bad cwd, missing required field, malformed config).
- `2` — `substrate_error` or `recall_unavailable` (substrate temporarily
  unhealthy; retryable).
- `3` — `privacy_error` (Stream D refused output metadata).
- `4` — `not_implemented` (currently only `since_event_id` non-null).

## 4. Session and project binding

### 4.1 Session binding

`SessionBinding` fields:

```rust
struct SessionBinding {
    session_id: String,
    harness: String,
    harness_version: Option<String>,
    cwd: String,
    project: Option<ProjectBinding>,
    namespaces_in_scope: Vec<String>,
}
```

`ProjectBinding` fields:

```rust
struct ProjectBinding {
    canonical_id: String,
    alias: Option<String>,
    resolved_via: ProjectBindingSource,
}

enum ProjectBindingSource {
    YamlOverride,
    GitRemote,
}
```

Validation:

- `cwd` must be absolute. Stream E canonicalizes it via `std::fs::canonicalize`
  for project-binding lookup; any I/O error (including non-existence) returns
  `invalid_request`. Stream E does not enforce sandbox or symlink-escape
  policy — that is the harness/OS responsibility.
- `session_id` and `harness` must be non-empty after trim, bounded to 128 UTF-8
  bytes each.
- `harness_version` is bounded to 128 UTF-8 bytes when present.
- `budget_tokens` outside `512..=8000` returns `invalid_request`.

### 4.2 Project binding

`.memory-project.yaml` schema:

```yaml
canonical_id: proj_agent_memory
alias: agent-memory
```

Required fields:

- `canonical_id`: non-empty ASCII string matching `^[a-zA-Z0-9_-]{3,128}$`.
  The `:` character is reserved as the namespace separator (`project:<id>`)
  and is forbidden inside `canonical_id` itself.

Optional fields:

- `alias`: non-empty UTF-8 string bounded to 128 bytes after trim.

Unknown fields are invalid. Empty files, non-mapping YAML, duplicate keys, or
unsupported scalar types return `invalid_request`.

Parser requirements:

- `serde(deny_unknown_fields)` on the deserialization target.
- Duplicate-key detection is **not** delegated to `serde_yaml`'s default
  behavior (which silently keeps the last value). Stream E pre-parses the
  document with a low-level YAML event reader (e.g. `yaml-rust2`) and rejects
  any duplicate mapping key before invoking serde.
- Acceptance tests must cover: empty file, non-mapping root, duplicate keys
  (`canonical_id` declared twice), unknown field, `canonical_id` containing `:`,
  alias exceeding 128 bytes, non-ASCII `canonical_id`.

### 4.3 Project-binding caching

For Stream E v0.3, project binding is **recomputed on every request**. No cache.
Walking from `cwd` upward and shelling to `git remote get-url origin` is
acceptable per-call I/O for v0.3 traffic patterns (one `memory_startup` per
session, occasional `delta-block` per turn). A scoped cache keyed by
`(canonicalized_cwd, session_id)` with an explicit invalidation contract is a
post-v0.3 optimization and out of scope here.

Project binding resolves in this order:

1. Walk from `cwd` upward to find `.memory-project.yaml`. If present and valid,
   use its `canonical_id` and optional `alias`; `resolved_via = "yaml_override"`.
2. Else, find the nearest git worktree root and read `git remote get-url origin`.
   Normalize the URL into a canonical `host/path` string before hashing so that
   SSH, HTTPS, and git-protocol clones of the same upstream produce identical
   canonical ids. Normalization rules, applied in order:

   - SSH form `[user@]host:path` → `host/path` (treat the first `:` after the
     host as a path separator, never as a port).
   - HTTPS form `https://[user[:pass]@]host[:port]/path` → `host/path` (drop
     scheme, userinfo, and port).
   - HTTP form `http://...` → same rule as HTTPS.
   - Git form `git://[user@]host[:port]/path` → `host/path`.
   - File URLs `file:///abs/path` → the absolute filesystem path, after
     `std::fs::canonicalize`.
   - Bare local paths (no scheme, no SSH-style colon) → the absolute filesystem
     path, after `std::fs::canonicalize`.
   - Lowercase the hostname (DNS is case-insensitive); leave path case
     unchanged because forge path components may be case-sensitive.
   - Strip a single trailing `.git` from the path if present.
   - Strip any trailing `/` from the path.
   - Collapse repeated `/` runs in the path to a single `/`.
   - Trim leading/trailing whitespace from the entire input before any of the
     above.

   The canonical id is `proj_` plus lowercase SHA-256 hex of the normalized
   string; `resolved_via = "git_remote"`. Acceptance tests must cover SSH↔HTTPS
   equivalence on the same upstream, case-insensitive hostname equivalence, and
   `.git`-suffix equivalence so a worker cannot silently regress this rule.
3. Else, use no project binding; namespaces are `["me", "agent"]`.

Malformed `.memory-project.yaml` is `invalid_request`, not silent fallback.
Missing git or missing remote is not an error; it means no project namespace.

`namespaces_in_scope` order is stable:

1. `me`
2. `project:<canonical_id>` when project binding exists
3. `agent`

## 5. Recall block format

`memory_startup` emits this top-level shape, even when some sections are empty:

```xml
<memory-recall version="stream-e-v0.4" harness="codex" session="sess_abc123">
  <identity>
  </identity>
  <project-state project="agent-memory" resolved-via="git_remote">
  </project-state>
  <entity-recall entities="">
  </entity-recall>
  <recent-memory>
  </recent-memory>
  <pending-attention>
  </pending-attention>
  <recall-explanation policy="stream-e-v0.4" budget-tokens="3600" used-tokens="1420">
  </recall-explanation>
</memory-recall>
```

Rules:

- Section tags are always present and in the order above.
- Empty sections contain no placeholder prose.
- Memory entries use one line per fact:
  `- [<id>] <summary> (updated <YYYY-MM-DD>; source <source_kind>; confidence <0.00..1.00>)`
- Body snippets are included only for plaintext memories whose
  `retrieval_policy.index_body` is true and only inside entity/recent sections.
- Summaries are bounded to 240 UTF-8 bytes per memory entry; snippets to 360
  UTF-8 bytes. Truncation is at the largest UTF-8 character boundary ≤ N bytes
  (no panics on multi-byte chars). When a value is truncated, the entry ends
  with the literal `…` (U+2026) before any closing punctuation.
- The entire block must fit within `budget_tokens` according to the estimator,
  **including the always-on wrapper tags**. The estimator counts the rendered
  bytes of every emitted character (open tags, attributes, whitespace, closing
  tags, content). Implementations may pre-compute a constant scaffold cost
  (~60–80 estimated tokens for the empty-section frame) and subtract it from
  the available content budget before per-section allocation.

## 6. Candidate collection

Stream E builds a request-local candidate set from Stream A APIs, using the
v0.3 `MemoryQuery` extension (see §1.1) so candidate enumeration is served from
the SQLite index without full envelope hydration:

1. **Per-section index queries** using the extended `MemoryQuery`:
   - `<identity>`: `MemoryQuery { namespace_prefix: Some("me"), passive_recall_only: true, .. }`
   - `<project-state>`: `MemoryQuery { namespace_prefix: Some(format!("project:{canonical_id}")), passive_recall_only: true, .. }`
   - `<recent-memory>`: each in-scope namespace prefix in turn with
     `updated_since: Some(now - 7d)`.
   - Each query also sets `status: Some(Active)` and a second pass with
     `status: Some(Pinned)` (or, if the field becomes `Vec<MemoryStatus>` in a
     future Stream A revision, both in one call).
2. `read_memory_envelope(id)` only for candidates that survive index filtering
   and need their structured frontmatter (e.g. for entity-id resolution that
   isn't tag-indexed).
3. `query_chunks(ChunkQuery { text: Some(query), ..Default::default() })` only
   for user-message deltas or entity lookup terms, never for blanket startup
   enumeration.

Candidate filters applied **before** ranking (most served by the index query;
the rest by frontmatter inspection on hydrated candidates):

- status must be `active` or `pinned` (index-served);
- `retrieval_policy.passive_recall` must be true (index-served);
- `requires_user_confirmation` must be false;
- `write_policy.human_review_required` must be false;
- `review_state` must be absent or one of `approved`, `accepted`, or `none`;
- memory scope must be visible from the active namespace set (index-served via
  `namespace_prefix`);
- sensitivity must be compatible with `retrieval_policy.max_scope`;
- encrypted records are represented as metadata-only unless Stream D supplied a
  safe index projection already present in Stream A metadata.

After collection and before scoring, the candidate set is sorted ascending by
memory id. This eliminates SQLite row-order nondeterminism from any subsequent
scoring/tie-break path.

No Stream E code may parse raw Markdown files directly as a bypass around Stream
A. If Stream A lacks a query needed for efficient collection beyond the v0.3
extension in §1.1, raise it as a Stream A spec amendment rather than creating a
private Stream E database.

## 7. Entity and alias resolution

Entity matching uses canonical frontmatter only:

- `Frontmatter.entities[].id`
- `Frontmatter.entities[].label`
- `Frontmatter.entities[].aliases[]`
- `Frontmatter.aliases[]`
- normalized tags for project/tool nouns

Normalization:

- Unicode NFKC normalization when the dependency is already present; otherwise
  ASCII case-folding plus whitespace collapse is acceptable for v0.3 and must be
  documented in the implementation.
- Case-insensitive match.
- Hyphen, underscore, slash, and space are equivalent separators.
- A match shorter than three alphanumeric characters is ignored unless it is an
  exact entity id.

Startup entity seeds:

- project alias and canonical id;
- basename of `cwd`;
- immediate parent directory basename;
- tags and aliases from pinned identity/project-state memories.

Delta entity seeds:

- all startup seeds;
- normalized tokens and quoted phrases from the submitted message;
- exact memory ids in the message.

Alias collisions:

- If one alias maps to multiple entity ids in the same namespace, do not emit a
  fact based on that alias. Emit exactly **one** `RecallOmission` per collision
  with `reason = "ambiguous_alias"`, `alias = Some(<surface form>)`,
  `colliding_ids = <every entity id the alias resolved to, sorted
  lexicographically>`, and `id = None`. Do not emit one omission per colliding
  id; the alias-grouping is the relationship the explanation needs to preserve.
  When the same alias collides in multiple sections (e.g. both `<entity-recall>`
  and `<project-state>`), emit one omission per `(section, alias)` pair so each
  section's explanation is self-contained.

## 8. Ranking and budgets

Stream E ranks candidates in deterministic section-specific passes.

### 8.1 Section budgets

Default startup budget: 3600 estimated tokens.

Section budget caps:

| Section | Cap |
| --- | ---: |
| identity | 20% |
| project-state | 30% |
| entity-recall | 30% |
| recent-memory | 10% |
| pending-attention | 5% |
| recall-explanation | 5% |

Unused section budget may flow downward to later content sections, except
`recall-explanation` always retains enough space for omitted-item summaries.

Default delta budget: 400 estimated tokens.

### 8.2 Ranking formula

For facts inside a section:

```text
score =
  status_weight
  + scope_weight
  + entity_match_weight
  + recency_weight
  + confidence_weight
  + source_weight
```

Weights:

- `status_weight`: pinned `100`, active `50`.
- `scope_weight`: exact project namespace `30`, me `25`, agent `15`.
- `entity_match_weight`: exact id `40`, exact label/alias `25`, tag match `10`.
- `recency_weight`: updated within 7 days `10`, within 30 days `5`, otherwise
  `0`.
- `confidence_weight`: `floor(confidence * 10)`.
- `source_weight`: user `10`, agent_primary `5`, subagent/tool/file `3`.

Tie-breakers:

1. higher score;
2. pinned before active;
3. newer `updated_at`;
4. lexicographic memory id.

No LLM calls, embedding calls, network calls, or nondeterministic randomization
are allowed in Stream E v0.3 ranking tests.

## 9. Section semantics

### 9.1 `<identity>`

Includes pinned or active memories under `me/identity/` and `me/preferences/`
or with tags `identity`, `preference`, or `standing-order`.

Must not include project-specific claims unless they are also in `me` scope.

### 9.2 `<project-state>`

Included only when project binding exists.

Sources, in priority order:

1. project memories tagged `state`, `invariant`, `decision`, or `open-question`;
2. project memories whose entities or aliases match the project binding;
3. recent project decisions if `include_recent` is true.

Open questions and review counts are summarized as counts plus memory ids, not
as unapproved claims.

### 9.3 `<entity-recall>`

Includes active/pinned memories whose entity ids, labels, aliases, tags, or
chunk hits match startup or delta seeds.

For plaintext records, one bounded snippet may be emitted when it helps identify
the remembered fact. For encrypted records, emit only summary and safe
descriptors; never emit body text or masked-body projections.

### 9.4 `<recent-memory>`

Included only when `include_recent` is true. Includes active/pinned memories in
scope updated within the last seven days, after higher-priority sections have
already selected their entries. Duplicate ids already emitted in earlier
sections are skipped.

### 9.5 `<pending-attention>`

May include:

- count of candidate/quarantined review items from Stream C review projection;
- count of operator repair findings from Stream A doctor;
- count of encrypted lifecycle operations that failed closed;
- count of malformed project binding/config findings.

It must not quote the body or claim text of candidate/quarantined memories.

### 9.6 `<recall-explanation>`

Contains:

- selected memory ids by section;
- matched entity ids/aliases/tags;
- budget used and omitted counts by section;
- omission reasons from the closed `OmissionReason` enum (see §3.3).

Omission-list bounding:

- Omissions are sorted by the lexicographic tuple
  `(section, reason, alias.unwrap_or(""), id.unwrap_or(""))` for stable
  ordering. `alias` participates in the sort key only because v0.4 introduced
  it; for non-`ambiguous_alias` reasons `alias` is `None` and the empty-string
  fallback makes the sort total without privileging legacy entries.
- The serialized omission list is truncated to **64 entries** in any single
  response. When truncated, the response includes `omitted_truncated_count: u32`
  carrying the number of dropped entries. The aggregate per-section omitted
  counts (`RecallSectionExplanation.omitted_count`) remain accurate regardless
  of truncation.

Explanation metadata may include ids and safe summaries. It must not include
encrypted plaintext, candidate/quarantine claim bodies, or raw secret-like
input. All free-form prose synthesized by Stream E into this section (e.g.
ambiguous-alias collision messages) must pass through `safe_plaintext_fragment`
(see §1.1) before serialization.

## 10. Privacy and encrypted-memory behavior

Stream E treats Stream D as the only authority for decryption and privacy
classification.

Required behavior:

- `memory_startup` never calls `memory_reveal`.
- `memory_startup` never accepts a flag to reveal encrypted content.
- `memory_get` remains the explicit bounded preview path; `memory_reveal`
  remains the explicit audited decrypt path.
- Encrypted records with `MemoryContent::Ciphertext` or
  `MemoryContent::MetadataOnly` may appear only as metadata references:
  id, summary, tags, entity labels, safe source descriptors, and safe
  `privacy_descriptors`.
- If a safe metadata field itself matches Stream D secret/high-risk rules during
  recall assembly, Stream E omits that field and records
  `omitted.reason = "review_pending"` when the memory has pending review state;
  otherwise it records `omitted.reason = "encrypted_body_hidden"`.

Stream E does not reclassify every recalled memory from scratch. It calls
`memory_privacy::safe_plaintext_fragment` (see §1.1) before emitting any
*newly synthesized* free-form text — specifically: explanation prose,
ambiguous-alias collision messages, hook diagnostic lines, and any echoed-back
CLI argument substring that appears inside an error message body. Incoming
flags themselves are not "sanitized" — they are validated as in §4.1; the
helper only runs on output text that Stream E composes for emission.

## 11. Error codes

Stream E adds or uses these daemon protocol error codes:

| Code | Retryable | Meaning |
| --- | --- | --- |
| `invalid_request` | false | Bad cwd, empty session/harness, invalid budget, malformed project config, or unsupported non-null `since_event_id`. |
| `substrate_error` | true | Stream A read/query/doctor failed. |
| `recall_unavailable` | true | Startup block cannot be assembled because required substrate/index state is under repair. |
| `privacy_error` | false | Stream D safety check refused recall output metadata. |
| `not_implemented` | false | Reserved only for explicit future features such as event-based `since_event_id` deltas. |

`memory_startup` itself must not return `not_implemented` after Stream E lands,
**with one exception**: a request carrying a non-null `since_event_id` returns
`not_implemented` per §3.1, because event-based deltas are explicitly deferred.
All other field combinations must produce either a successful `Startup`
response or a non-`not_implemented` typed error.

## 12. Hook integration contracts

### 12.1 Claude Code

Installer-created hook scripts call:

- `memoryd recall startup-block` on SessionStart;
- `memoryd recall delta-block` on UserPromptSubmit.

If startup recall fails, the hook prints no recall block and writes a single
diagnostic line to stderr. It must not block the harness longer than 800ms in
normal 1k-memory smoke fixtures.

### 12.2 Codex

Installer-created `AGENTS.md` guidance may instruct Codex to call
`memory_startup` first, but the durable contract is the MCP tool response shape
in this spec. If a native startup-hook path exists, it must use the same
`startup-block` output and must not invent a second recall format.

### 12.3 Cursor and generic MCP clients

Cursor rules and generic MCP clients receive the same MCP `memory_startup`
contract. If they cannot inject pre-prompt context, integration status is
degraded but the daemon response remains identical.

## 13. Performance requirements

Release-gate fixture sizes (warm path, after first-call cold-start has run):

- 200 memories: startup recall p95 <= 80ms.
- 1,000 memories: startup recall p95 <= 250ms.
- 1,000 memories delta block with a non-matching prompt: p95 <= 60ms.
- 1,000 memories delta block with five matching entities: p95 <= 120ms.

Cold-start budget (first `memory_startup` call after `memoryd` boot, paying
SQLite open + index attach + initial project-binding I/O):

- 1,000 memories: cold-start single-call <= 600ms.

The benchmark must record:

- memory count;
- encrypted metadata-only count;
- candidate/quarantine count;
- hardware profile;
- budget tokens;
- selected memory count;
- omitted memory count;
- whether the run was cold-start or warm.

Stream E uses the v0.3 `MemoryQuery` extension (§1.1) to keep candidate
collection on the index path. Hydrating Stream A envelopes for every
status=active record is forbidden in the steady state; if a future workload
proves the §1.1 extension insufficient, raise it as a Stream A spec amendment
rather than creating a private Stream E database.

### 13.1 Observability counters

`memoryd` increments in-process counters on every recall invocation, surfaced
through the existing `Status` request payload. None of these are persisted to
disk in v0.3 (see §15 deferral); they reset on daemon restart.

- `recall.startup_invoked_total`
- `recall.startup_failed_total{code}`
- `recall.delta_invoked_total`
- `recall.delta_failed_total{code}`
- `recall.budget_exhausted_total{section}`

`StatusResponse` is extended additively:

```rust
pub struct StatusResponse {
    pub state: String,
    pub guidance: String,
    pub recall: RecallStatusCounters,
}

pub struct RecallStatusCounters {
    pub startup_invoked_total: u64,
    pub startup_failed_total: BTreeMap<String, u64>,
    pub delta_invoked_total: u64,
    pub delta_failed_total: BTreeMap<String, u64>,
    pub budget_exhausted_total: BTreeMap<String, u64>,
}
```

Serialized shape:

```json
{
  "state": "ready",
  "guidance": "memoryd handlers are backed by the Stream A substrate.",
  "recall": {
    "startup_invoked_total": 1,
    "startup_failed_total": { "invalid_request": 1 },
    "delta_invoked_total": 0,
    "delta_failed_total": {},
    "budget_exhausted_total": { "entity-recall": 2 }
  }
}
```

Counters are present on every successful `Status` response. A fresh daemon
returns zeroes and empty maps.

Acceptance test asserts the counters increment on a single `memory_startup`
call and on an `invalid_request` rejection.

## 14. Acceptance signals

Implementation is complete when these tests/docs exist and pass:

- `crates/memoryd/tests/startup_recall_mcp.rs`
  - MCP `memory_startup` forwards to the daemon and no longer returns
    `not_implemented`.
  - request validation rejects missing/relative cwd, empty session id, and
    invalid budgets.
  - response shape includes `session_binding`, `recall_block`,
    `budget_used_tokens`, and `recall_explanation`.
- `crates/memoryd/tests/startup_recall_privacy.rs`
  - encrypted records are descriptor-findable but never body-recalled;
  - `memory_startup` does not reveal ciphertext;
  - candidate/quarantined encrypted review items affect pending-attention counts
    without leaking claim text.
- `crates/memoryd/tests/startup_recall_governance.rs`
  - active/pinned records can recall;
  - candidate/quarantined/tombstoned/superseded records cannot recall as facts;
  - `retrieval_policy.passive_recall = false` suppresses recall.
- `crates/memoryd/tests/startup_recall_ranking.rs`
  - ranking is deterministic;
  - tie-breakers are id-stable across at least two scenarios where multiple
    candidates land on the identical aggregate score;
  - candidate iteration is sorted by id before scoring (verified by feeding
    pre-shuffled candidate sets and asserting identical output);
  - budget exhaustion produces stable omissions in explanation metadata;
  - an alias that resolves to two or more entity ids in the same namespace
    emits exactly one `RecallOmission` with `reason = "ambiguous_alias"`,
    `alias = Some(<surface form>)`, `colliding_ids` containing every matched
    entity id sorted lexicographically, and `id = None`; non-collision
    omissions still serialize without `alias` or `colliding_ids` keys.
- `crates/memoryd/tests/startup_recall_determinism.rs`
  - given the same fixture repo state, request context, budget, and
    `TimeSource` fixture, two consecutive `memory_startup` calls produce
    byte-identical `recall_block` and `recall_explanation` outputs;
  - includes a fixture with multi-byte UTF-8 (CJK + emoji) to lock in
    char-boundary truncation behavior;
  - includes a fixture where the omission list exceeds 64 entries to lock in
    `omitted_truncated_count` semantics.
- `crates/memoryd/tests/startup_recall_project_binding.rs`
  - `.memory-project.yaml` wins over git remote;
  - malformed project config fails closed;
  - no-git cwd degrades to `me` + `agent` namespaces;
  - SSH (`git@github.com:foo/bar.git`) and HTTPS
    (`https://github.com/foo/bar.git`) clone-URL forms of the same upstream
    produce identical `canonical_id` and identical `namespaces_in_scope`;
  - hostname case differences (`GitHub.com` vs `github.com`) produce identical
    `canonical_id`;
  - trailing `.git` and trailing `/` differences produce identical
    `canonical_id`.
- `crates/memoryd/tests/recall_cli.rs`
  - `startup-block` prints only the recall block to stdout;
  - `delta-block` prints `<memory-delta empty="true" />` on no match;
  - CLI errors keep diagnostics on stderr.
- `crates/memory-substrate/tests/memory_query_extension.rs`
  - the v0.3 `MemoryQuery` extension (§1.1) returns expected rows for each new
    filter (`status`, `namespace_prefix`, `passive_recall_only`,
    `updated_since`) without falling back to envelope hydration;
  - existing default-filter behavior is preserved.
- `crates/memory-privacy/tests/safe_plaintext_fragment.rs`
  - `safe_plaintext_fragment` returns `Allow` for benign text;
  - returns `OmitEncryptedBodyHidden` for secret/high-risk class hits;
  - returns `OmitReviewPending` for text whose final Stream D routing would be
    `PrivacyStorageAction::EncryptAtRest`;
  - is deterministic across repeated invocations.
- `docs/api/stream-e-passive-recall-api.md`
  - documents MCP, daemon, and CLI request/response examples.
- `docs/api/stream-a-public-api.md` and `docs/api/stream-d-privacy-api.md`
  - updated to reflect the §1.1 surface additions.
- `README.md` and `CLAUDE.md`
  - note Stream E shipped only after the tests above pass.

## 15. Explicit deferrals

These are intentionally outside Stream E v0.3:

- persistent recall-count and last-recalled mutation;
- live peer activity, claim locks, and event subscriptions;
- semantic embeddings for recall ranking beyond existing Stream A query APIs;
- LLM summarization or compression during startup;
- automatic hook installation across all harnesses;
- Stream F dream-question surfacing, except future pending-attention counts when
  Stream F creates canonical question memories;
- dashboard visualizations of recall explanations.

If an implementation needs one of these to pass the v0.3 acceptance tests, the
spec should be revised before coding continues.
