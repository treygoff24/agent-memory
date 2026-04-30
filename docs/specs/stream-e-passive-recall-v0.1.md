# Stream E Passive Recall Spec v0.1

**Status:** draft implementation contract for Stream E passive recall.
**Date:** 2026-04-30.
**Sources:** `docs/specs/system-v0.1.md` section 10 and the shipped Stream A-D contracts.
**Non-source:** older Stream A drafts and Stream C/D review notes are historical unless they describe a still-shipped API surface.

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
- `since_event_id`: reserved for future event-based deltas. v0.1 accepts null
  or absent only; non-null values return `not_implemented` so callers do not
  assume event deltas exist.
- `budget_tokens`: inclusive range `512..=8000`; default `3600`.

The current legacy MCP request shape `{ "include_recent": true }` is not
sufficient for production recall because it lacks binding context. During the
Stream E implementation, tests may keep one compatibility case only if the MCP
forwarder injects `cwd`, `session_id`, and `harness` from trusted adapter
context before contacting the daemon.

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
}

struct RecallSectionExplanation {
    name: String,
    selected_ids: Vec<String>,
    matched_entities: Vec<String>,
    budget_used_tokens: usize,
}

struct RecallOmission {
    id: Option<String>,
    section: String,
    reason: String,
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
          "policy": "stream-e-v0.1",
          "sections": [],
          "omitted": []
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

`startup-block` writes only the recall block text to stdout. `delta-block`
writes an empty stdout on no match; otherwise it writes exactly one
`<memory-delta>...</memory-delta>` block. Both commands return non-zero for
typed protocol errors and must not print debug logs to stdout.

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

- `cwd` must be absolute and must resolve without following a symlink outside
  the caller-visible filesystem path. If `cwd` does not exist, return
  `invalid_request`.
- `session_id` and `harness` must be non-empty after trim.
- `harness_version` is bounded to 128 UTF-8 bytes when present.
- `budget_tokens` outside `512..=8000` returns `invalid_request`.

### 4.2 Project binding

`.memory-project.yaml` schema:

```yaml
canonical_id: proj_agent_memory
alias: agent-memory
```

Required fields:

- `canonical_id`: non-empty ASCII string matching `^[a-zA-Z0-9_:-]{3,128}$`.

Optional fields:

- `alias`: non-empty UTF-8 string bounded to 128 bytes after trim.

Unknown fields are invalid. Empty files, non-mapping YAML, duplicate keys, or
unsupported scalar types return `invalid_request`.

Project binding resolves in this order:

1. Walk from `cwd` upward to find `.memory-project.yaml`. If present and valid,
   use its `canonical_id` and optional `alias`; `resolved_via = "yaml_override"`.
2. Else, find the nearest git worktree root and read `git remote get-url origin`.
   Normalize by trimming whitespace and removing a trailing `.git`; canonical id
   is `proj_` plus lowercase SHA-256 hex of the normalized remote URL;
   `resolved_via = "git_remote"`.
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
<memory-recall version="stream-e-v0.1" harness="codex" session="sess_abc123">
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
  <recall-explanation policy="stream-e-v0.1" budget-tokens="3600" used-tokens="1420">
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
- Summaries are bounded to 240 UTF-8 bytes per memory entry.
- Snippets are bounded to 360 UTF-8 bytes per memory entry.
- The entire block must fit within `budget_tokens` according to the estimator.

## 6. Candidate collection

Stream E builds a request-local candidate set from Stream A APIs:

1. `query_memory(MemoryQuery { include_metadata_only: true, ..Default::default() })`
   to enumerate indexed memories.
2. `read_memory_envelope(id)` for candidates needed after cheap metadata
   filtering.
3. `query_chunks(ChunkQuery { text: Some(query), ..Default::default() })` only
   for user-message deltas or entity lookup terms, never for blanket startup
   enumeration.

Candidate filters applied before ranking:

- status must be `active` or `pinned`;
- `retrieval_policy.passive_recall` must be true;
- `requires_user_confirmation` must be false;
- `write_policy.human_review_required` must be false;
- `review_state` must be absent or one of `approved`, `accepted`, or `none`;
- memory scope must be visible from the active namespace set;
- sensitivity must be compatible with `retrieval_policy.max_scope`;
- encrypted records are represented as metadata-only unless Stream D supplied a
  safe index projection already present in Stream A metadata.

No Stream E code may parse raw Markdown files directly as a bypass around Stream
A. If Stream A lacks a query needed for efficient collection, the v0.1
implementation may hydrate via existing Stream A read APIs and should document
the performance cost in tests.

## 7. Entity and alias resolution

Entity matching uses canonical frontmatter only:

- `Frontmatter.entities[].id`
- `Frontmatter.entities[].label`
- `Frontmatter.entities[].aliases[]`
- `Frontmatter.aliases[]`
- normalized tags for project/tool nouns

Normalization:

- Unicode NFKC normalization when the dependency is already present; otherwise
  ASCII case-folding plus whitespace collapse is acceptable for v0.1 and must be
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
  fact based on that alias. Add an explanation entry with
  `omitted.reason = "ambiguous_alias"` and list the colliding entity ids.

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
are allowed in Stream E v0.1 ranking tests.

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
- omission reasons from the stable set:
  `budget_exhausted`, `status_excluded`, `passive_recall_disabled`,
  `review_pending`, `encrypted_body_hidden`, `ambiguous_alias`,
  `namespace_out_of_scope`, `superseded`, `tombstoned`.

Explanation metadata may include ids and safe summaries. It must not include
encrypted plaintext, candidate/quarantine claim bodies, or raw secret-like input.

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

Stream E does not reclassify every recalled memory from scratch. It may call
Stream D's deterministic safe-fragment helper for newly assembled explanation
strings, CLI arguments, and hook messages before including them in output.

## 11. Error codes

Stream E adds or uses these daemon protocol error codes:

| Code | Retryable | Meaning |
| --- | --- | --- |
| `invalid_request` | false | Bad cwd, empty session/harness, invalid budget, malformed project config, or unsupported non-null `since_event_id`. |
| `substrate_error` | true | Stream A read/query/doctor failed. |
| `recall_unavailable` | true | Startup block cannot be assembled because required substrate/index state is under repair. |
| `privacy_error` | false | Stream D safety check refused recall output metadata. |
| `not_implemented` | false | Reserved only for explicit future features such as event-based `since_event_id` deltas. |

`memory_startup` itself must not return `not_implemented` after Stream E lands.

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

Release-gate fixture sizes:

- 200 memories: startup recall p95 <= 80ms.
- 1,000 memories: startup recall p95 <= 250ms.
- 1,000 memories delta block with a non-matching prompt: p95 <= 60ms.
- 1,000 memories delta block with five matching entities: p95 <= 120ms.

The benchmark must record:

- memory count;
- encrypted metadata-only count;
- candidate/quarantine count;
- hardware profile;
- budget tokens;
- selected memory count;
- omitted memory count.

Stream E may initially hydrate metadata by reading Stream A envelopes, but if
the 1,000-memory gate fails, the implementation must add a Stream A query/index
extension rather than creating a private Stream E database.

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
  - tie-breakers are id-stable;
  - budget exhaustion produces stable omissions in explanation metadata.
- `crates/memoryd/tests/startup_recall_project_binding.rs`
  - `.memory-project.yaml` wins over git remote;
  - malformed project config fails closed;
  - no-git cwd degrades to `me` + `agent` namespaces.
- `crates/memoryd/tests/recall_cli.rs`
  - `startup-block` prints only the recall block to stdout;
  - `delta-block` prints empty stdout on no match;
  - CLI errors keep diagnostics on stderr.
- `docs/api/stream-e-passive-recall-api.md`
  - documents MCP, daemon, and CLI request/response examples.
- `README.md` and `CLAUDE.md`
  - note Stream E shipped only after the tests above pass.

## 15. Explicit deferrals

These are intentionally outside Stream E v0.1:

- persistent recall-count and last-recalled mutation;
- live peer activity, claim locks, and event subscriptions;
- semantic embeddings for recall ranking beyond existing Stream A query APIs;
- LLM summarization or compression during startup;
- automatic hook installation across all harnesses;
- Stream F dream-question surfacing, except future pending-attention counts when
  Stream F creates canonical question memories;
- dashboard visualizations of recall explanations.

If an implementation needs one of these to pass the v0.1 acceptance tests, the
spec should be revised before coding continues.
