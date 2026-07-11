# Stream F Dreaming Spec v0.3

**Status:** implementation contract for Stream F dreaming. Supersedes `stream-f-dreaming-v0.2.md`.
**Date:** 2026-04-30.
**Sources:** `docs/specs/system-v0.1.md` §12 (dreaming), §11 (governance machinery), the shipped Stream A–E contracts, and the Codex review at `docs/reviews/stream-f-codex-spec-review.md`.
**Non-source:** older drafts, brainstorm notes in `docs/handoff-2026-04-23.md`, and the handbook are background; they are not normative for this spec.

**Revision goal (v0.2 → v0.3):** make Pass 2 candidate privacy behavior dogfood-honest: EncryptAtRest-classified candidates are refused with a stable operator-visible reason (`privacy_required_encryption`) instead of being silently dropped or written encrypted for review. Dream summaries group candidate refusal counts by reason. This keeps dream prose narrative-only and avoids introducing encrypted candidate-review semantics before Stream D key-rotation work lands.

**Revision goal (v0.1 → v0.2):** integrate the Codex spec review (`docs/reviews/stream-f-codex-spec-review.md`) before implementation. The v0.1 cognitive-pipeline architecture and harness-CLI delegation are preserved. The contract surfaces are tightened in twelve places that would have caused implementation churn or correctness bugs:

1. **Substrate writes get their own MCP tool (`memory_observe`) instead of folding into `memory_note(kind=...)`.** Tool names are behavioral nudges and the durability promise differs (note → may become memory; observation → may never).
2. **Pass 3 question files become JSONL with an explicit `entities` sidecar** so Stream E `<pending-attention>` can entity-match against masked text without unmasking. v0.1's plain-Markdown shape silently broke entity surfacing under masking — a correctness bug, not a polish issue.
3. **Pass 2 prompts now include an explicit evidence catalog**, and validation rejects any candidate ref not in the catalog. v0.1 said refs must be "verbatim from the prompt input" without specifying the input shape.
4. **Harness CLI prompts pass via stdin, not argv**, where the harness supports it. Argv is visible in `/proc/<pid>/cmdline`, `ps`, and `top` to any local user — even masked text leaks via that surface. Adapters that cannot accept stdin must declare it explicitly and surface the limitation in `memoryd dream status`.
5. **The `MaskingSession` API references align with shipped Stream D code.** v0.1 invented `unmask` and `end`; the shipped surface is `new(id)`, `mask(&mut self, text, spans)`, `restore(&self, session_id, text)`, with teardown via `Drop`.
6. **Dream Markdown/JSONL files are explicitly excluded from canonical memory parsing/indexing.** v0.1 implied this; v0.2 makes it a normative invariant with a dedicated acceptance test.
7. **`PassOutcome` carries a structured `candidate_results: Vec<CandidateWriteResult>`** instead of a bare id list, so refusals (accepted: false, reason: <code>) are reportable through the protocol.
8. **CLI naming is normalized to `memoryd dream …`** everywhere; `memory dream …` slipped into v0.1 in two places.
9. **Scheduled-vs-manual lease semantics split:** `memoryd dream now` fails fast on `lease_unavailable`; the scheduled daily run retries within a bounded window so a transient `git fetch` blip at 03:00 does not silently erase the day's dream.
10. **Pending-attention caps tighten to 2/scope and 6 total**, with deterministic surfacing order (entity-overlap strength → file recency → novelty hash → lex tie-break) and per-reason omission counters.
11. **Filesystem path encoding for project/org scopes uses nested directories (`project/<id>/`)** instead of colon-bearing path segments (`project:<id>/`). Colons fight macOS HFS-legacy assumptions and slug conventions. The synthetic `namespace_prefix` strings used in shipped Stream A/E remain `project:<id>` — only Stream F's new on-disk paths change.
12. **Daemon-authored git commits get explicit author/message conventions and dirty-tree handling**, so lease and cleanup writes have a deterministic git footprint.

Other v0.1 → v0.2 changes adopted from the Codex review: missing config keys (`lease_window_seconds`, `pass_1_window_days`, `candidate_stale_days`, `pass_2_drift_threshold`, `events.compaction_days`, `dream_retry_window_minutes`) are now defined with defaults and validation ranges; `dreams/cleanup/` is added to the owned-paths list (v0.1 referenced it under §7 without listing it in scope); `ScopeRunSummary` and `HarnessCliStatus` are defined in the protocol section; the encrypted substrate JSONL shape is specified explicitly; the v0.1 wording bug "three new top-level directories" (which then listed four) is corrected.

Stream F turns the agent-memory system from "memoryful at startup" into "memoryful over time." It implements the three-layer dreaming pipeline (substrate / journal / cleanup) described in system spec §12 — but with one consequential simplification: Stream F does **not** build a generic LLM provider, manage API keys, or own model selection. Instead, the daemon delegates each dream pass to whichever agent-harness CLI is already installed and authenticated on the dreaming device (`claude -p`, `codex exec`, etc.). If the user has installed a supported harness, dreaming works for free; if not, dreaming is unavailable and that is a reported, recoverable state, not a failure.

This factoring is deliberate. Stream F's value is the cognitive pipeline (substrate fragment lifecycle, lease-elected daily journal runs, masked synthesis, grounding-rehydrated promotion, idempotent cleanup), not the LLM call surface. By piggybacking on the user's existing harness CLI, Stream F inherits the user's auth, billing, model choice, and offline behavior without owning any of them.

## 1. Scope and dependency boundaries

Stream F owns:

- substrate-fragment write surface (`memory_observe` MCP tool) and the `substrate/` per-device JSONL file series with 14-day archival lifetime;
- daily journal layer with three LLM-backed passes (`why`, `what should change`, `uncomfortable question`) and a leased-device election;
- nightly cleanup layer (idempotent janitorial operations on canonical memory state);
- the harness-CLI provider abstraction that brokers dream passes through installed agent CLIs;
- per-scope CLI priority configuration and lease eligibility;
- masked-synthesis integration: dream prompts run on Stream D-masked text; restoration happens on Pass 2 candidate write-back only;
- candidate-promotion grounding rehydration — re-resolve cited source refs at promote time, skip on drift;
- Stream E `<pending-attention>` Pass-3-question hook (deferred from Stream E v0.5 §15);
- `memoryd dream {status,now,review,enable,disable}` CLI;
- new top-level repo paths: `substrate/`, `encrypted/substrate/`, `dreams/journal/`, `dreams/questions/`, `dreams/cleanup/`, `leases/`;
- daemon-authored git commit conventions for lease, cleanup, and journal writes.

Stream F does not own:

- generic LLM provider abstraction with bring-your-own API keys, HTTP retries, token accounting, or cost ceilings;
- model selection — the user's chosen harness CLI determines the model;
- contradiction-tiebreak provider integration (Stream C ships the `ContradictionTiebreaker` trait; whether it later rides the same harness-CLI mechanism Stream F builds is an explicit follow-up, not part of v0.2);
- embedding inference (Stream A/B; out of scope and not unblocked by this spec);
- privacy-filter inference (Stream D; Layer 1 deterministic classifier remains the live path; ONNX model loading remains deferred);
- canonical memory mutation, governance lifecycle, privacy classification, encryption, recall block assembly — those remain Streams A/C/D/E respectively;
- dashboard UI for dream review (Stream G);
- live peer presence, claim locks, or cross-device journal merging (Stream I).

Stream F must not create a hidden second persistence layer. Substrate fragments are canonical files under git-synced `substrate/` (per-device prefix paths to avoid merge conflicts); journal/question/cleanup outputs are canonical files under `dreams/`; leases are canonical files under `leases/`. All of them participate in Stream A's existing watcher / event log / git sync machinery without modification, but **none of them parse or index as canonical memories** — they have their own validators (§1.1) and are explicitly excluded from `query_memory`, `query_recall_index`, and `query_chunks`.

### 1.1 Cross-stream surface changes required by Stream F

Implementation of this spec lands surface additions on already-shipped streams. They are part of the Stream F v0.2 contract.

**Stream A — canonical tree-layout extension (Stream A spec amendment §16.x):**

Six new top-level path families must validate under `Substrate::tree::validate` and must be excluded from canonical memory indexing:

- `substrate/<device_id>/<YYYY-MM-DD>.jsonl` — append-only per-device JSONL substrate fragments. `<device_id>` is the existing device identity (per Stream A `git::adopt_clone`). Files older than the configured fragment lifetime move to `substrate/archive/<device_id>/<YYYY-MM>.jsonl` (year-month bucketed for compactness).
- `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl` — age-encrypted parallel file series for fragments whose Stream D classification routes them to encrypted storage. Pass 1 input never reads encrypted fragments.
- `dreams/journal/<scope_path>/<YYYY-MM-DD>.md` — Pass 1 narrative output. Markdown body, no frontmatter (these are not canonical memories; they are NOT a grounding source per Stream C).
- `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl` — Pass 3 adversarial questions. JSONL, one record per line, with `{entities: [...], question: "..."}` shape (see §6.4). Same non-grounding rule as journal.
- `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json` — daily cleanup-run report. Single JSON object per file. Read by `memoryd dream review` and `memoryd doctor`.
- `leases/journal.lease` — JSONL lease file (one record per active lease window per scope). Existing record format from system spec §12.2; concrete shape in §6.1 below.

`<scope_path>` encoding (used for `dreams/journal/` and `dreams/questions/` only):

| Scope | Path |
|---|---|
| `me` | `me/<date>` |
| `agent` | `agent/<date>` |
| `project:<id>` | `project/<id>/<date>` |
| `org:<id>` | `org/<id>/<date>` |

This avoids colons in directory names. The synthetic `namespace_prefix` strings shipped in Stream A/E (`"project:proj_abc"`) remain unchanged on the wire; only the on-disk Stream F path encoding differs.

These paths are **not** subject to the canonical memory frontmatter schema. They have their own validators in `crates/memory-substrate/src/tree.rs`, which assert path-pattern conformance and JSONL/JSON well-formedness but do not invoke frontmatter parsing. Stream A's `query_memory`, `query_recall_index`, and `query_chunks` must skip these paths entirely. An acceptance test (§12) verifies that a frontmatter-free dream Markdown file is valid AND not indexed as a canonical memory.

Stream A's three-way merge driver must treat:

- substrate JSONL files as append-only (concat + sort by `id`);
- dream JSONL files (questions, lease) as append-only (concat + sort by `(scope, ts, id)`);
- dream Markdown files (journal) as last-writer-wins by date+device (collisions are diagnostics, not blockers — two devices wrote the same scope's journal on the same date because the lease was contested);
- cleanup JSON files as last-writer-wins by `(device_id, date)` (each device produces its own report; same device producing a second report on the same date overwrites).

**Stream B — new `memory_observe` MCP tool:**

`memory_note` is **not** modified. It continues to write a canonical memory and only that.

A new MCP tool `memory_observe` is added with this shape:

```json
{
  "tool": "memory_observe",
  "arguments": {
    "text": "Third time investigating JWT validation in this repo - pattern emerging around key rotation.",
    "kind": "pattern",
    "entities": ["ent_auth_flow", "ent_jwt"]
  }
}
```

```rust
pub struct MemoryObserveRequest {
    pub text: String,
    pub kind: ObserveKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserveKind {
    Observation,
    Pattern,
    Signal,
}
```

The MCP forwarder gains `memory_observe` as the 9th agent-facing tool (Search/Get/Note/Write/Supersede/Forget/Reveal/Startup + Observe). The daemon protocol gains `RequestPayload::Observe` and `ResponsePayload::Observe(ObserveResponse)`. Internally, `memory_observe` and `memory_note` share privacy classification, caller-context capture, and event emission; they diverge on storage routing (canonical write vs substrate-fragment append) and response shape.

**Stream C — grounding rehydration enforcement:**

Stream C v0.1 already accepts `grounding_rehydration_required: true` on candidate proposals; Stream F is the first writer that sets this flag and the first consumer that needs it enforced. Stream F lands the enforcement: at promote time, every cited `source.ref` in a dream-authored candidate is re-resolved against the live substrate. The candidate is **not promoted** and is moved to `status: quarantined` with `reason: grounding_rehydration_failed` if any of the following hold:

- a cited file is missing;
- a cited file's content has shifted beyond `dreams.pass_2_drift_threshold` (default: Levenshtein distance > 30% of original byte length);
- a cited substrate fragment has aged past the configured fragment lifetime;
- a cited memory is now `tombstoned`, `superseded`, or `archived`.

This is a deterministic check; it does not call any LLM.

**Stream D — masked-synthesis session integration (aligned to shipped API):**

Stream D ships `MaskingSession` with this surface (`crates/memory-privacy/src/masking.rs`):

```rust
pub struct MaskingSessionId(String);
pub struct MaskingSession { /* salt table, label counters, id */ }

impl MaskingSession {
    pub fn new(id: MaskingSessionId) -> Self;
    pub fn mask(&mut self, text: &str, spans: &[PrivacySpan]) -> PrivacyResult<String>;
    pub fn restore(&self, session_id: &MaskingSessionId, text: &str) -> PrivacyResult<String>;
}
```

Teardown is via `Drop`; there is no explicit `end()` method. The salt table is in-memory only (`#[derive(Clone, Debug)]` is on the struct, but it must never be serialized — the field documentation already states this).

Stream F is the first user. The contract is unchanged; Stream F adheres to it as follows:

- one `MaskingSession` per per-scope dream run, identified by `MaskingSessionId::new(format!("dream:{scope}:{run_id}"))`;
- every dream prompt input passes through `MaskingSession::mask`;
- Pass 2 candidate output (claim, excerpts, rationale) passes through `MaskingSession::restore` immediately before write-back;
- the session value is owned by the run-scope future; when the future ends (success, failure, panic, cancellation), the `MaskingSession` is dropped and the salt table is freed. Tests assert the session's `Drop` impl runs in the failure path;
- Pass 1 (journal markdown) and Pass 3 (questions JSONL) outputs are NOT restored — they remain in masked form on disk.

**Stream E — `<pending-attention>` Pass-3 hook:**

Stream E v0.5 §15 explicitly defers Pass 3 question surfacing to Stream F. Stream F lands the wiring: the recall-block builder reads `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl` for the most recent date <= today for each scope in `namespaces_in_scope`, performs entity intersection against the active recall seed set using each record's `entities` field, and emits matching questions as `<pending-attention>` line items.

Surfacing rules:

- one line per matching question, format: `- [<scope>] <question text>` with question text bounded to 240 UTF-8 bytes (same rule as memory summaries);
- questions are surfaced **only** when at least one entity in the record's `entities` field matches an entity in the active recall seed set; a record with empty `entities` never surfaces;
- safe-fragment classification (Stream D `safe_plaintext_fragment`) runs on every question text before emission; classified-unsafe questions are silently omitted (these are masked-synthesis outputs and should be safe by construction, but defense-in-depth);
- caps: **2 questions per scope**, **6 questions total per `<pending-attention>` section** (tightened from v0.1's 3/8 — startup cognitive load matters and a noisier section trains operators to ignore it);
- deterministic surfacing order when the cap is hit: (a) strongest entity overlap with the active recall seed set (count intersection), (b) most recent question file by `<YYYY-MM-DD>`, (c) novelty hash (skip questions whose `question` text hash matches any question surfaced in the last 7 days of recall blocks — this requires Stream E to maintain a small in-memory ring buffer of recently surfaced question hashes; `dreams.pending_attention_recent_window_days` config, default 7), (d) lexicographic tie-break on `(scope, question)`;
- per-reason omission counters in `RecallStatusCounters`: `dream_question_omitted_total{reason}` keyed by `cap_section | cap_total | no_entity_match | unsafe_fragment | malformed_record`;
- this hook runs in Stream E's startup-recall hot path; reading the questions file must be O(file size) with no LLM call, no rerun of Pass 3, and no I/O outside the file read and the safe-fragment classifier.

The `policy` attribute on the recall block does **not** bump for this addition (additive feature; existing `stream-e-v0.5` policy string covers it). A Stream E spec amendment to v0.6 documents the unhide.

## 2. Safety invariants

1. **Dream prose is never a grounding source.** Pass 1 narrative and Pass 3 questions are explicitly excluded from Stream C grounding-ref resolution. A candidate citing `dreams/journal/...` or `dreams/questions/...` as `source.ref` is refused at write time with `WriteFailureKind::DreamProseAsSource`.

2. **Pass 2 candidates always go to the candidate queue.** Stream F never auto-promotes a Pass-2-authored candidate. `dreaming-strict` policy applies. Candidates may be promoted later by Stream G review UI or by `memoryd review approve`; Stream F does not bypass that gate.

3. **Masked synthesis is mandatory.** Every dream prompt's input text is masked through `MaskingSession::mask` before any harness CLI is invoked. The salt table is daemon-process-local, in-RAM only, and is freed via `Drop` at end-of-run. A test that exercises a dream pass must verify no unmasked sensitive token reaches the harness CLI's stdin (or argv on adapters that fall back to argv).

4. **No `memory_reveal` from any dream pass.** Encrypted memories may contribute only their summary, tags, entity labels, and Stream D safe descriptors to Pass 1/2/3 inputs — same constraint as Stream E recall.

5. **Lease holder is honest.** The lease record contains the device id; only the device that wrote the active lease may write that scope's journal/question files for the lease's date. A device that observes a foreign device's active lease must not run a dream for that scope, even if it has a faster/different harness CLI installed.

6. **Cleanup is commutative and idempotent.** Every cleanup operation produces the same final state regardless of execution order, repetition, or concurrent execution by sibling devices. Cleanup never deletes a memory body — it can only flip status, archive substrate fragments, or compact event-log entries past retention.

7. **Harness CLI calls fail closed.** A harness CLI subprocess that times out, exits non-zero, returns malformed structured output (after one retry for Pass 2), or signals authentication failure aborts the affected pass. The lease is released. Partial state (e.g., Pass 1 wrote successfully but Pass 2 failed) is committed: Pass 1's journal stays on disk; Pass 2 emits no candidates; Pass 3 still runs if Pass 1 succeeded.

8. **Substrate fragments respect classification.** A `memory_observe` call runs through Stream D's `DeterministicPrivacyClassifier` over the text before any disk effect, exactly as canonical writes do. Refused tiers are refused with no fragment written. PII tiers route to `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl`; journal/question passes never decrypt these. The Pass 1 prompt sees only the plaintext substrate fragments; encrypted fragments contribute their (already safe) descriptor projection only.

9. **Dream files do not parse as canonical memories.** `dreams/journal/...md`, `dreams/questions/...jsonl`, `dreams/cleanup/...json`, `substrate/...jsonl`, and `leases/...lease` are all explicitly excluded from canonical memory parsing (`Substrate::read_memory_envelope` returns `NotACanonicalMemory` for these paths) and from canonical memory indexing (`query_memory`/`query_recall_index`/`query_chunks` skip them). Acceptance test §12 covers this.

10. **Prompts do not appear in argv when the harness supports stdin.** Each `HarnessCli` adapter declares its preferred prompt-transport mode; `Stdin` is preferred and required where the underlying CLI supports it. Adapters that fall back to argv must declare `PromptTransport::Argv` and surface the limitation in `memoryd dream status`. The privacy disclosure (§14) names this limitation per adapter.

11. **Output is reproducible across re-runs only at the prompt-input level.** Given the same substrate fragments, active memory set, masked-synthesis salt seed, and harness CLI choice, Stream F produces a byte-identical *prompt input*. The harness CLI's output is non-deterministic by construction; tests assert prompt determinism, not response determinism. The test fixture `EchoCli` lets acceptance tests pin response shape.

12. **Errors are typed.** Daemon protocol error codes are stable; CLI exit codes are stable; users do not parse free-form prose to detect failure.

## 3. Public surfaces

### 3.1 MCP tools

`memory_observe` (new in Stream F v0.2):

```json
{ "text": "Third time investigating JWT validation in this repo - pattern emerging around key rotation.",
  "kind": "pattern",
  "entities": ["ent_auth_flow", "ent_jwt"] }
```

Validation: `text` non-empty after trim and bounded to 16 KiB; `kind` ∈ `{observation, pattern, signal}`; `entities[]` bounded to 32 entries, each ≤ 128 UTF-8 bytes.

`memory_note` is unchanged from Stream B/D shipped behavior — it writes a canonical memory under `notes/` and accepts only `{ text }`. v0.1's proposal to add `kind` was withdrawn in v0.2.

Stream F does not add any other agent-facing MCP tools. `memoryd dream …` is CLI/admin only and is explicitly rejected from MCP forwarding (same pattern as `memoryd privacy`, `memoryd device`, `memoryd review`).

### 3.2 Daemon protocol additions

```rust
// New variant: memory_observe forwarder.
RequestPayload::Observe {
    text: String,
    kind: ObserveKind,
    #[serde(default)] entities: Vec<String>,
}

// New variant: explicit dream-trigger for `memoryd dream now`.
RequestPayload::DreamNow {
    scope: String,                   // "me" | "agent" | "project:..." | "org:..."
    force: bool,                     // bypass lease-already-held check; for tests/admin
    cli_override: Option<String>,    // bypass per-scope priority for one run
}

// New variant: dream status query. Read-only.
RequestPayload::DreamStatus {}
```

Response payloads:

```rust
ResponsePayload::Observe(ObserveResponse)
ResponsePayload::DreamNow(DreamRunReport)
ResponsePayload::DreamStatus(DreamStatusReport)

struct ObserveResponse {
    fragment_id: String,                                // sub_<ulid>
    target: ObserveTarget,                              // plaintext_substrate | encrypted_substrate
}

#[serde(rename_all = "snake_case")]
enum ObserveTarget { PlaintextSubstrate, EncryptedSubstrate }

struct DreamRunReport {
    scope: String,
    cli_used: Option<String>,                           // None when run aborted before CLI selection
    pass_1: PassOutcome,
    pass_2: PassOutcome,
    pass_3: PassOutcome,
    duration_ms: u64,
}

struct PassOutcome {
    status: PassStatus,                                 // success | skipped | failed
    output_path: Option<String>,                        // dreams/journal/<scope>/<date>.md, etc.
    candidate_results: Vec<CandidateWriteResult>,       // populated only for Pass 2
    error_code: Option<String>,                         // populated only for status=failed
    duration_ms: u64,
}

struct CandidateWriteResult {
    id: Option<String>,                                 // mem_<id> when accepted; None when refused
    accepted: bool,
    reason: Option<String>,                             // refusal reason code (governance code) when accepted=false
    source_ref_count: usize,                            // number of cited evidence refs after validation
}

#[serde(rename_all = "snake_case")]
enum PassStatus { Success, Skipped, Failed }

struct DreamStatusReport {
    enabled: bool,
    last_runs: Vec<ScopeRunSummary>,                    // most recent run per scope
    active_leases: Vec<LeaseRecord>,
    cli_inventory: Vec<HarnessCliStatus>,
    counters: DreamStatusCounters,
    privacy_disclosure: String,                         // verbatim §14 disclosure text
}

struct ScopeRunSummary {
    scope: String,
    last_run_at: Option<DateTime<Utc>>,
    last_run_outcome: Option<PassStatus>,               // worst pass status of the most recent run
    last_run_cli: Option<String>,
    consecutive_missed_runs: u32,                       // bounded retry-window misses since last success
}

struct HarnessCliStatus {
    name: String,                                       // "claude" | "codex" | "gemini" | ...
    is_installed: bool,
    is_authenticated: Option<bool>,                     // None when not yet probed this session
    prompt_transport: PromptTransport,                  // stdin | argv (per-adapter declared)
    last_probe_at: Option<DateTime<Utc>>,
    last_probe_error: Option<String>,
}

#[serde(rename_all = "snake_case")]
enum PromptTransport { Stdin, Argv }

struct LeaseRecord {
    device: String,
    scope: String,
    acquired_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    run_id: String,
}
```

### 3.3 CLI surfaces

```bash
memoryd dream status [--repo .] [--runtime .memoryd] [--json]
memoryd dream now --scope <me|agent|project:<id>|org:<id>> [--force] [--cli claude|codex|...] [--json]
memoryd dream review --since <duration> [--scope <...>]   # walk recent journal/question/candidate output
memoryd dream enable
memoryd dream disable
```

`memoryd dream status` writes a structured human-readable report to stdout (JSON only with `--json`); the privacy disclosure (§14) is the first line of the human-readable form. `memoryd dream now` writes a `DreamRunReport` to stdout (JSON with `--json`). `memoryd dream review` walks `dreams/journal/`, `dreams/questions/`, and the candidate queue under `dreaming-strict` for the requested window. `memoryd dream enable`/`disable` toggle the device-local sentinel `~/.memoryd/dream-disabled` (see §3.4).

Exit codes match Stream E conventions:

- `0` — operation succeeded;
- `1` — `invalid_request` (bad scope, missing CLI override, malformed config);
- `2` — `dream_unavailable` (no eligible harness CLI installed/authenticated, or daemon unreachable);
- `3` — `privacy_error` (Stream D refused fragment write or unmask diagnostic);
- `4` — `dream_pass_failed` (one or more passes failed; partial output may be on disk);
- `5` — `lease_held` or `lease_unavailable` (manual `dream now` only; scheduled runs do not surface as exit codes).

### 3.4 Configuration surface

Per-scope CLI priority is **synced** (so all devices agree which CLI a scope dreams via). CLI **availability** is local. A device-local sentinel `~/.memoryd/dream-disabled` (created by `memoryd dream disable`) overrides synced `enabled: true` for that one device.

```yaml
# config.yaml — synced top-level keys
dreams:
  enabled: true
  default_cli_priority: [claude, codex]
  scope_overrides:
    me: [claude]
    project:proj_abc: [codex, claude]
    agent: [claude]

  # Pass and timing knobs.
  per_pass_timeout_seconds: 300                # default 300; range [30, 1800]
  pass_1_window_days: 7                        # default 7; range [1, 90]; substrate fragments to include
  pass_2_max_candidates: 8                     # default 8; range [1, 64]
  pass_2_drift_threshold: 0.30                 # default 0.30; range [0.05, 0.90]; Levenshtein/byte-len ratio
  pass_3_max_questions: 12                     # default 12; range [1, 64]; written to disk
  pending_attention_per_scope_cap: 2           # default 2; range [1, 8]; surfaced in <pending-attention>
  pending_attention_total_cap: 6               # default 6; range [1, 24]
  pending_attention_recent_window_days: 7      # default 7; range [1, 30]; novelty-hash window

  # Lifecycle and cleanup.
  fragment_lifetime_days: 14                   # default 14; range [1, 365]
  candidate_stale_days: 30                     # default 30; range [1, 365]
  cleanup_run_hour_utc: 3                      # default 3; range [0, 23]

  # Lease and retry.
  lease_window_seconds: 3600                   # default 3600; range [60, 14400]
  dream_retry_window_minutes: 180              # default 180 (03:00-06:00); range [0, 720]
                                               # 0 disables scheduled retries (manual-only)

# Existing top-level events block gains one knob (Stream A amendment):
events:
  compaction_days: 90                          # default 90; range [7, 730]
```

Validation:

- `default_cli_priority` and each `scope_overrides[*]` value must be a non-empty list of known harness names; unknown names fail config load (fail-closed).
- All numeric values must fall within their declared ranges; out-of-range fails config load.
- `dreams.scope_overrides` keys must be valid scope strings (`me`, `agent`, `project:<id>`, `org:<id>`); invalid keys fail config load.

## 4. Harness-CLI provider abstraction

### 4.1 Trait

```rust
#[async_trait]
pub trait HarnessCli: Send + Sync {
    /// Stable identifier used in config (`claude`, `codex`, `gemini`, ...).
    fn name(&self) -> &'static str;

    /// How this adapter passes the prompt to its underlying CLI.
    /// Stdin is preferred and required where the underlying CLI supports it;
    /// argv is permitted only when the underlying CLI cannot accept stdin and
    /// the limitation is surfaced in `memoryd dream status` and in §14.
    fn prompt_transport(&self) -> PromptTransport;

    /// Detect whether the underlying binary is available on PATH.
    /// O(1), cached, refreshed on `cli_inventory_refresh()`.
    fn is_installed(&self) -> bool;

    /// Probe authentication via ordered provider-specific auth commands:
    /// prefer the current CLI surface first, fall back to a legacy command only
    /// when the preferred surface is clearly unsupported. Auth failure or
    /// timeout on a supported preferred command must not fall through to legacy.
    /// Returns `Ok(true)` only on confirmed authenticated state.
    async fn is_authenticated(&self) -> Result<bool, HarnessCliError>;

    /// Run a dream pass. `prompt` is masked. `expect_json` is true for Pass 2.
    /// Implementations construct the harness-specific argv (`-p`, `exec`, etc.),
    /// pass the prompt via stdin where supported, run the subprocess via
    /// `tokio::task::spawn_blocking`, apply the configured per-pass timeout,
    /// and return the raw stdout text (or parsed-and-validated JSON for Pass 2).
    async fn complete(
        &self,
        prompt: &str,
        expect_json: bool,
        timeout: Duration,
    ) -> Result<String, HarnessCliError>;
}

pub enum HarnessCliError {
    NotInstalled,
    NotAuthenticated { hint: String },
    Timeout { duration: Duration },
    SubprocessExit { code: Option<i32>, stderr_tail: String },
    MalformedJson { stage: JsonStage, raw: String },     // only for expect_json=true
    Io(std::io::Error),
}
```

### 4.2 Built-in implementations

Stream F v0.2 ships these adapters. Each has a unit test that records the argv it would invoke without actually spawning the subprocess.

| Adapter | Invocation | Stdin? | Auth probe |
|---|---|---|---|
| `ClaudeCodeCli` | `claude --print` (reads prompt from stdin) | yes | Prefer `claude auth status`; fallback to `claude config get auth.user` only when the preferred command is unsupported. |
| `CodexCli` | `codex exec --json -` (reads prompt from stdin when `-`) for `expect_json=true`, otherwise `codex exec -` | yes | Prefer `codex login status`; fallback to `codex auth status` only when the preferred command is unsupported. |
| `GeminiCli` | `gemini -p -` (reads prompt from stdin) | yes (when supported by upstream); falls back to argv with declared `PromptTransport::Argv` if upstream lacks stdin support at adapter ship time | per upstream conventions; `Ok(false)` with hint string if no probe is available |
| `EchoCli` | test-only, replays canned outputs from a `HashMap<PromptHash, String>` fixture, never spawns a subprocess | n/a | always `Ok(true)` |

**Auth probe invariant:** when the preferred supported command returns auth failure, timeout, or I/O error, the adapter must not invoke the legacy fallback. Legacy probes run only when stderr indicates the preferred command is unsupported (for example unrecognized subcommand).

Each adapter declares `prompt_transport()` truthfully. Adapters whose upstream CLI does not support stdin must declare `PromptTransport::Argv`; the daemon will surface the implication in `memoryd dream status` and in the privacy disclosure (§14). The `GeminiCli` adapter ships in v0.2 only if upstream supports stdin; otherwise it ships in v0.3 once upstream lands stdin or the `Argv` declaration is reviewed by Trey.

`DroidCli` and `OpenCodeCli` are not in v0.2 but are explicitly addable as v0.3 adapters without spec changes. The `HarnessCli` trait is the durable contract.

### 4.3 Registry and selection

`HarnessCliRegistry` is a daemon-process-local singleton:

```rust
pub struct HarnessCliRegistry {
    adapters: BTreeMap<&'static str, Arc<dyn HarnessCli>>,
}

impl HarnessCliRegistry {
    /// Iterate the configured priority list for a scope, returning the first
    /// adapter whose `is_installed()` AND `is_authenticated().await? == true`.
    /// `None` if no adapter qualifies — caller emits `dream_unavailable`.
    pub async fn select_for_scope(
        &self,
        scope: &Scope,
        config: &DreamsConfig,
    ) -> Option<Arc<dyn HarnessCli>>;
}
```

Selection is per-scope: the same daemon process may dream `me` via Claude and `project:foo` via Codex on the same day, if config says so and both are installed. Selection happens **after** the lease is acquired; if no eligible CLI is found, the lease is released within the same second and the run reports `dream_unavailable`.

### 4.4 Subprocess hardening

Every `HarnessCli::complete` call:

- runs the subprocess via `tokio::task::spawn_blocking` so it never stalls the daemon's tokio current-thread runtime;
- applies the per-pass timeout via `tokio::time::timeout`; on elapse, sends SIGTERM, waits 2s, sends SIGKILL;
- runs in a clean working directory (the dream device's per-device scratch dir under `~/.memoryd/dream-scratch/<run_id>/`); the harness CLI does not inherit project config from the user's git tree;
- runs with a minimal env: `PATH`, `HOME`, `TERM=dumb`, the harness CLI's own auth env vars (e.g., `ANTHROPIC_API_KEY`), and nothing else;
- writes the prompt to the subprocess's stdin where `prompt_transport() == Stdin`; closes stdin after writing; never echoes the prompt elsewhere;
- captures stdout up to a 16 MiB cap (Pass 1/3 prose has been observed at ~30 KB; Pass 2 JSON at ~5 KB; the cap is paranoia, not policy);
- captures stderr up to 64 KiB and surfaces the tail in `HarnessCliError::SubprocessExit::stderr_tail` for diagnostic logging;
- never echoes the prompt to stderr, never writes the prompt to disk, never logs the prompt at any verbosity (the prompt may contain masked-but-still-private content).

Argv-fallback adapters: when `prompt_transport() == Argv`, the prompt is passed as a single argv element. The adapter declares this. `memoryd dream status` shows `prompt_transport: argv` for that adapter and the privacy disclosure (§14) names the leak surface (`/proc/<pid>/cmdline`, `ps`, `top`). No adapter ships in v0.2 with argv fallback unless Trey approves the declaration.

## 5. Substrate layer

### 5.1 Fragment write surface

`memory_observe` appends a JSONL record to the current device's substrate file:

```jsonl
{"id":"sub_<ulid>","ts":"2026-04-30T14:22:10Z","device":"dev_<id>","session":"sess_<id>","harness":"claude-code","scope":"project:proj_<hex>","entities":["ent_auth_flow"],"kind":"observation","text":"User corrected: we don't use HS256; we use RS256 with a rotating key.","source_ref":"session:sess_<id>:turn:47","privacy_spans":[]}
```

Field semantics:

- `id` — `sub_<ulid>`. ULID gives monotonic per-device timestamps and zero intra-second collision.
- `ts` — RFC 3339 UTC.
- `device`, `session`, `harness` — populated from caller context exactly as canonical Stream D writes from the same caller do.
- `scope` — synthetic namespace string (`me`, `agent`, `project:<id>`, `org:<id>`); Stream F infers from the calling session's project binding (Stream E §4.2 mechanics) plus the caller's `kind` argument hints when ambiguous.
- `entities` — copied verbatim from the `memory_observe` request.
- `kind` — `observation | pattern | signal`.
- `text` — the masked / classified plaintext for plaintext fragments; for encrypted fragments the on-disk `text` is the age-encrypted ciphertext and a sibling `descriptor` field carries Stream D's safe descriptor projection (see §5.1.1).
- `source_ref` — Stream D source-ref string.
- `privacy_spans` — Stream D classifier output spans (used only by audit / cleanup; not consumed by Pass 1).

The write path:

1. `memoryd` receives `RequestPayload::Observe { text, kind, entities }`.
2. Stream D `DeterministicPrivacyClassifier` runs over `text` and yields `ClassificationOutcome` plus `privacy_spans`.
3. Refused tiers (Secret, identity-theft) return `WriteFailureKind::SecretRefused` exactly as today; no fragment written.
4. PII tiers route to `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl` (age-encrypted file series, parallel to canonical encrypted memories). Pass 1 input never reads these files.
5. All other tiers append to plaintext `substrate/<device_id>/<YYYY-MM-DD>.jsonl`.
6. Append is via the existing Stream A atomic-append pattern with `fsync` per record (substrate fragments are durable, like events).
7. The watcher emits a `SubstrateFragmentWritten` event onto the per-device JSONL event log. This event is informational; nothing in the hot path consumes it. The event makes substrate-fragment activity observable to operator tooling.

#### 5.1.1 Encrypted substrate JSONL shape

Encrypted-substrate records have the same JSONL frame as plaintext but with an explicit shape distinction:

```jsonl
{"id":"sub_<ulid>","ts":"...","device":"...","session":"...","harness":"...","scope":"...","entities":["ent_auth_flow"],"kind":"observation","encryption":{"recipient":"<age-pubkey>","ciphertext_b64":"..."},"descriptor":{"summary_safe":"User asked about auth flow integration","tag_safe":["auth"]},"source_ref":"...","privacy_spans":[{"label":"private_email","start":12,"end":34}]}
```

Differences vs plaintext:

- no `text` field; `encryption.ciphertext_b64` carries the age-encrypted plaintext;
- `descriptor` carries Stream D's safe-descriptor projection (the same projection produced for encrypted canonical memories), used as Pass 1 input in lieu of the ciphertext;
- everything else is identical.

Pass 1 input from encrypted substrate fragments uses only `id`, `ts`, `entities`, `kind`, `descriptor.summary_safe`, and `descriptor.tag_safe`. The classifier-produced `summary_safe` is itself masked-text-safe; Pass 1 still passes it through `MaskingSession::mask` for uniformity (no-op when no spans are detected, deterministic nonetheless).

### 5.2 Fragment lifetime and archival

A fragment's `ts` plus `dreams.fragment_lifetime_days` (default 14) determines its expiry. Cleanup (§7) archives expired fragments into `substrate/archive/<device_id>/<YYYY-MM>.jsonl` once per day per device. Archival is concat-and-sort-by-id; archival files are append-only across days; archival operations are idempotent (re-archiving a fragment that's already in the archive is a no-op).

Archived fragments remain visible to Pass 1 reads if they fall within the `--since` window the journal layer requested (default `dreams.pass_1_window_days = 7` for daily). Archived fragments are not visible to substrate-fragment search by Stream B `memory_search`; substrate fragments are not in the canonical memory index. This is intentional: substrate is "raw observations the daemon's still chewing on," not "search me later."

## 6. Journal layer

### 6.1 Lease election

`leases/journal.lease` is JSONL, one record per active lease window per scope:

```jsonl
{"device":"dev_macbook","scope":"me","acquired_at":"2026-04-30T03:00:00Z","expires_at":"2026-04-30T04:00:00Z","run_id":"run_<ulid>"}
{"device":"dev_macbook","scope":"agent","acquired_at":"2026-04-30T03:00:10Z","expires_at":"2026-04-30T04:00:00Z","run_id":"run_<ulid>"}
```

#### 6.1.1 Manual run (`memoryd dream now`)

Manual runs **fail fast**. A device attempting a dream for scope X:

1. `git fetch origin` (best-effort; on network failure, abort with `lease_unavailable` and exit code 5).
2. Read `leases/journal.lease`. Filter to records for scope X with `expires_at > now`.
3. If a non-empty subset remains, the scope is leased — abort this run with `lease_held{by_device}` and exit code 5. Do not retry within the lease window. (`--force` overrides this check; see below.)
4. Otherwise, append a new lease record with `acquired_at = now`, `expires_at = now + dreams.lease_window_seconds` (default 3600), commit, push.
5. On push race (rejected non-fast-forward), re-fetch and goto 2. Up to 3 retries with 200ms backoff, then abort with `lease_unavailable` and exit code 5.
6. After successful push, the device holds the lease and may proceed with Pass 1.
7. After all passes complete (or any pass fails), append a `lease-released` record (or just let it expire — the lease record is informational, not durable state).

`memoryd dream now --force` overwrites an active lease record, releases the existing lease, and proceeds. Used for admin and tests. Fails (`lease_unavailable`) only on network errors during the overwrite push.

#### 6.1.2 Scheduled run

The scheduled daily run (driven by an OS-level scheduler — launchd, systemd, cron — installed by the harness installer) **retries within a bounded window** so a transient network blip does not silently erase the day's dream:

1. At `dreams.cleanup_run_hour_utc` (note: the dream scheduler shares the cleanup-hour anchor for simplicity; cleanup happens after dreams), attempt the manual-run sequence above.
2. On `lease_unavailable` (fetch/push failure), record the failure in `dreams/cleanup/<device>/<date>.json` as a missed-run-attempt and retry with exponential backoff: 1min, 2min, 4min, 8min, 16min, 32min, capped at 32min between attempts.
3. Keep retrying until `dreams.dream_retry_window_minutes` (default 180 — i.e., until 06:00 UTC if anchor is 03:00) has elapsed since first attempt.
4. If still unavailable, record a missed-run summary in `dreams/cleanup/<device>/<date>.json` and increment `ScopeRunSummary.consecutive_missed_runs`. The next day's scheduled attempt resets the counter on success.
5. `lease_held` (another device holds the lease) is **not** retried — by design, only one device dreams a given scope per day, and this device is correctly deferring.

`dreams.dream_retry_window_minutes = 0` disables scheduled retries entirely; only manual `dream now` invocations dream that day. This is the device-local opt-out for users who don't want overnight retries.

#### 6.1.3 Daemon-authored git commits for lease writes

Lease append/release commits use a fixed author and message convention so they are recognizable in `git log` and operator tooling:

- Author: `memoryd lease-bot <noreply@memoryd.local>`
- Committer: same
- Message: `dream: lease <acquire|release> <scope> on <device_id>`
- Files staged: `leases/journal.lease` only. No other files are co-staged.

If the working tree is dirty (uncommitted user changes outside `leases/journal.lease`), the daemon does not commit. It logs `lease_dirty_tree` and aborts with `lease_unavailable`. This protects against accidentally co-committing user work-in-progress with daemon-authored lease writes.

### 6.2 Pass 1 — "Why did this happen this way?"

Inputs:

- substrate fragments in scope from the last `dreams.pass_1_window_days` (default 7), excluding encrypted fragments' ciphertext (encrypted fragments contribute their `descriptor` projection only);
- pinned and active memories in scope (read via Stream A `query_recall_index` with `passive_recall_only: true`);
- recent governance decisions (Stream C candidate-queue head + recent supersession/tombstone activity).

Processing:

1. Cluster fragments by entity co-occurrence — deterministic graph clustering on `entities[]` overlap, no LLM call. Tie-broken by fragment id.
2. Mask the entire input through `MaskingSession::mask`. The salt table now contains every masked token's reverse mapping.
3. Build the prompt from a versioned template (`prompts/dream-pass-1-v1.md`, checked into the repo). The template is a hand-written agent-targeted prompt that requests narrative prose, ~800 to ~2000 words, no JSON.
4. Call `HarnessCli::complete(prompt, expect_json: false, timeout: per_pass_timeout)`. Prompt passes via stdin per the adapter's `prompt_transport()`.
5. The response is masked-text Markdown. Write it directly to `dreams/journal/<scope_path>/<YYYY-MM-DD>.md`. Do **not** restore — Pass 1 output is never a grounding source and never read back into anything that would attempt to use the masked tokens as facts.

Failure modes:

- `HarnessCliError::NotAuthenticated` → `pass_1: failed{auth}`, abort entire run, release lease.
- `HarnessCliError::Timeout` → `pass_1: failed{timeout}`, abort, release.
- `HarnessCliError::SubprocessExit` → `pass_1: failed{subprocess, code, stderr_tail}`, abort, release.
- Empty/whitespace-only response → `pass_1: failed{empty_output}`, abort, release. Pass 1 produced nothing useful; running Pass 2/3 against an empty Pass 1 input is wasted work.

### 6.3 Pass 2 — "What should change?"

Inputs (the Pass 2 prompt is constructed deterministically from these — the exact JSON-shaped input is the **evidence catalog**):

- Pass 1 output (still masked);
- the same active-memory set Pass 1 saw;
- a JSON schema for candidate proposals;
- an explicit `evidence_catalog` containing every valid `sub_*` and `mem_*` ref the model is allowed to cite.

Processing:

1. Build the prompt from `prompts/dream-pass-2-v1.md`. The template wraps the inputs into a JSON document the model receives:

```json
{
  "pass_1_markdown": "...masked Pass 1 narrative...",
  "evidence_catalog": [
    {"kind":"substrate_fragment","ref":"sub_01J...","entities":["ent_auth_flow"],"excerpt":"...masked..."},
    {"kind":"memory","ref":"mem_20260430_...","entities":["ent_auth_flow"],"summary":"...masked..."}
  ],
  "candidate_schema": { "...": "..." }
}
```

The template instructs the model to emit a JSON array of candidate-proposal objects matching this schema:

```json
[
  {
    "claim": "<masked claim text>",
    "namespace": "project:proj_abc",
    "kind": "decision",
    "evidence": [
      { "kind": "substrate_fragment", "ref": "sub_01J...", "excerpt": "<masked>" },
      { "kind": "memory", "ref": "mem_20260430_...", "excerpt": null }
    ],
    "confidence": 0.7,
    "rationale": "<short masked rationale>"
  }
]
```

2. Call `HarnessCli::complete(prompt, expect_json: true, timeout: per_pass_timeout)`. Prompt passes via stdin.
3. Parse stdout as a JSON array of `Pass2Candidate`. On parse failure, retry once with a corrective preamble appended to the prompt (`Your previous response was not valid JSON. Please return only a JSON array conforming to the schema above.`). On second failure, `pass_2: failed{malformed_json, raw_tail}`, abort Pass 2, continue to Pass 3.
4. Validate every candidate against the schema, the evidence catalog, and config caps:
   - `namespace` must match an in-scope namespace per §6.1's lease scope set;
   - `kind` must be a known canonical memory kind;
   - `confidence` must be in `[0, 1]`;
   - `evidence[]` must contain at least one entry, every entry's `ref` must exist verbatim in the prompt's `evidence_catalog`, and every entry's `kind` must match the catalog entry's `kind`;
   - candidate count must not exceed `dreams.pass_2_max_candidates`.
5. For each surviving candidate, **restore** the `claim`, `excerpt`, and `rationale` fields via `MaskingSession::restore(&session_id, ...)`. This is the only place in the dream pipeline where restoration happens.
6. Write each restored candidate to the canonical candidate-write path (`Substrate::write_memory` with `status: candidate`, `policy: dreaming-strict`, `grounding_rehydration_required: true`). Stream C accepts or refuses each per its existing policy. Each result records into `PassOutcome.candidate_results: Vec<CandidateWriteResult>` with `id`, `accepted`, `reason` (governance code on refusal), and `source_ref_count` (post-validation count of cited refs).
7. Mark Pass 2 as success iff at least one candidate was accepted into the queue. Zero accepted is `pass_2: skipped` (not failed) — the model produced output but governance refused everything. Operator can inspect the refusals via `memoryd dream review`.

#### 6.3.1 Candidate privacy policy

Pass 2 candidates run the full deterministic Stream D privacy classifier after masked fields are restored and before any canonical write. The resulting `storage_action()` is normative:

- `Plaintext` and `Trusted` candidates may proceed through the existing `dreaming-strict` candidate-write path.
- `EncryptAtRest` candidates are refused. The candidate result MUST be `accepted: false`, `id: null`, `reason: "privacy_required_encryption"`, with `source_ref_count` preserving the validated evidence-ref count.
- `Refuse` candidates remain refused with the existing unsafe-candidate reason.

Stream F v0.3 deliberately does not create encrypted canonical dream candidates. Candidate review would need decrypt/reveal UX, key-availability diagnostics, and Stream D rotation semantics that are not part of this dogfood-readiness release. Refusing with `privacy_required_encryption` makes the boundary visible without leaking refused content.

`memoryd dream now` and any dream-run summary renderer MUST group Pass 2 refusal counts by reason so an operator can distinguish privacy-required-encryption refusals from hallucinated evidence refs, unsafe candidates, and governance policy refusals.

#### 6.3.2 Evidence ref validation rules

- every candidate must cite at least one valid catalog ref;
- candidates may cite multiple refs;
- every cited ref's `(kind, ref)` tuple must exist in the prompt's `evidence_catalog`;
- candidate claims may synthesize across cited refs, but uncited support does not count;
- hallucinated or out-of-window refs are deterministic rejects, not LLM-judgment calls.

This is strict, but the multi-fragment-synthesis concern from v0.1 is solved by allowing multiple citations, not by weakening validation.

### 6.4 Pass 3 — "What uncomfortable question is this system avoiding?"

Inputs:

- Pass 1 output (masked);
- a smaller summary of the active memory set (just memory ids, summaries, and entity tags — no full bodies);
- the previous N-day window's Pass 3 questions (so the model is encouraged to vary, not repeat).

Processing:

1. Build the prompt from `prompts/dream-pass-3-v1.md`. The template requests an adversarial self-critique pass producing JSONL output, one record per line, capped at `dreams.pass_3_max_questions`. Each record has the shape:

```jsonl
{"entities":["ent_auth_flow","ent_jwt"],"question":"What assumption about <PERSON_A>'s auth bug reports are we overfitting to?"}
```

The template explicitly instructs the model to populate `entities` from the entity ids it referenced in the question, drawing only from the entity set present in Pass 1 input. This is what lets Stream E entity-match against masked question text without unmasking.

2. Call `HarnessCli::complete(prompt, expect_json: false, timeout: per_pass_timeout)`. Prompt passes via stdin. (`expect_json: false` is intentional even though the output is JSONL — the JSONL is per-line-validated by Stream F, not whole-file-parsed by the harness CLI.)
3. Parse the response line by line. Discard malformed lines (record `dream_question_omitted_total{reason: malformed_record}` for telemetry). Validate each record:
   - `entities[]` non-empty, each entry ≤ 128 UTF-8 bytes;
   - every `entities[i]` must exist in the entity set present in Pass 1 input (entity hallucination is a deterministic reject, same posture as Pass 2 evidence refs);
   - `question` non-empty and ≤ 240 UTF-8 bytes (truncate-with-ellipsis if over; record as a soft warning, not a discard).
4. Write the validated records to `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl`. The questions remain masked; only the `entities` array is structured plaintext (entity ids are not masked — they are canonical ids).
5. **Do not restore, do not promote, do not enter the candidate queue.** Pass 3 output is exclusively for Stream E `<pending-attention>` consumption.

### 6.5 Salt-table teardown

The `MaskingSession` value is owned by the per-scope dream-run future. When the future ends — success, partial failure, full failure, panic, or cancellation — the session is dropped and the salt table is freed. Acceptance tests cover the failure-path drop with a `MaskingSession` instrumented to assert its `Drop` ran.

There is no explicit `end()` call. v0.1's reference to `MaskingSession::end` was an API misread; v0.2 aligns to the shipped `Drop`-based teardown.

## 7. Cleanup layer

Cleanup runs once per day per device at `dreams.cleanup_run_hour_utc + dreams.dream_retry_window_minutes` (so cleanup runs strictly after the dream window closes). It does **not** require a lease; multiple devices running cleanup concurrently is safe by design.

Operations, all idempotent and commutative:

1. **Substrate fragment archival.** Move expired fragments to `substrate/archive/<device_id>/<YYYY-MM>.jsonl`. Concat-and-sort. No-op if fragment is already archived.
2. **Stale candidate archival.** Candidates older than `dreams.candidate_stale_days` (default 30) with no governance review activity move to `status: archived`. Operator can re-promote by setting `status: candidate` again.
3. **Entity index rebuild.** Re-run Stream A entity-projection over canonical memories. No-op if projection already matches.
4. **Memory lint.** Validate frontmatter against the current schema; record findings in `memoryd doctor` output. Cleanup never auto-edits a memory body — lint findings are reports, not mutations.
5. **Tombstone integrity check.** Every tombstone has an active rule; every active rule points to a memory. Mismatches are reported, not auto-repaired.
6. **Supersession-chain orphan check.** Walk every chain in both directions. Dangling ends are reported.
7. **`observed_at` refresh.** For canonical memories whose `source.ref` resolves to a still-live file, update `observed_at` to the file's mtime. Idempotent.
8. **Event-log compaction.** Stream A event-log entries older than `events.compaction_days` (default 90) move to `events/archive/<YYYY-MM>.jsonl.zst`. Tail of live event log stays fast to read.

Cleanup writes a `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json` report summarizing the operations and any findings. The report is read by `memoryd dream review` and `memoryd doctor`.

### 7.1 Daemon-authored git commits for cleanup writes

Cleanup commits use a fixed author and message convention parallel to lease commits:

- Author: `memoryd cleanup-bot <noreply@memoryd.local>`
- Committer: same
- Message: `dream: cleanup <device_id> <date>` plus a one-line summary of operation counts (`fragments_archived=N candidates_archived=M lint_findings=K ...`).
- Files staged: only files cleanup actually mutated. `dreams/cleanup/<device>/<date>.json` is always staged on a non-trivial run.

If the working tree is dirty (uncommitted user changes outside the cleanup-mutated paths), cleanup proceeds but defers the commit — it writes its report and stages cleanup-mutated paths only, leaving the user to commit manually with their own work. The report records `commit_deferred: true`. This is identical posture to Stream A's existing daemon-authored writes.

## 8. Stream E hook for Pass 3 questions

(Detailed in §1.1 above.) Summary: Stream F's `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl` files are read by Stream E's recall-block builder during startup recall; intersecting questions surface in `<pending-attention>`. The contract is bounded (2 per scope, 6 total, 240-byte question text cap) and entity-gated using the records' explicit `entities` field (no entity match → no surface). Per-reason omission counters live in `RecallStatusCounters`. Deterministic surfacing order: entity-overlap strength → file recency → novelty hash → lex tie-break.

## 9. Error codes

| Code | Retryable | Meaning |
|---|---|---|
| `invalid_request` | false | Bad scope, unknown CLI override, malformed config, malformed `memory_observe` kind. |
| `dream_unavailable` | true | No eligible harness CLI installed/authenticated for this scope; or daemon unreachable from CLI. |
| `lease_held` | true (manual: bounce; scheduled: skip with no retry) | Another device holds this scope's lease; retry after `expires_at`. |
| `lease_unavailable` | true (manual: fail fast; scheduled: bounded retry) | `git fetch` or `git push` failed during lease acquisition; retry per §6.1.1/6.1.2. |
| `lease_dirty_tree` | false | Working tree has uncommitted user changes outside lease-staged files. |
| `dream_pass_failed` | false | At least one pass failed; details in per-pass `error_code`. |
| `privacy_error` | false | Stream D refused fragment write or restore diagnostic. |
| `dream_disabled` | false | `dreams.enabled = false` synced or device-local sentinel present. |

`memoryd dream now --force` returns `lease_held` only when `--force` is omitted. With `--force`, the lease is overwritten and the run proceeds.

## 10. Performance requirements

Dreaming is **not** in any request hot path. Performance targets are operational, not user-facing.

- Per-pass timeout: 300s default, configurable in `[30, 1800]`.
- Total daily-run wall-clock per scope: should not exceed 20 minutes under normal harness CLI latency (Sonnet/Opus class models complete a 2k-word prose pass in ~15-60s; 3 passes × 60s = 3min, with 17min of headroom for tail latency / retries).
- Lease acquisition: p95 < 2 seconds (one `git fetch` + one read + one append + one push, no LLM).
- Substrate-fragment write (`memory_observe`): p95 < 5ms (same budget as canonical writes; the only added cost vs. canonical is one extra Stream D classifier pass on shorter text).
- Cleanup full pass over 10k canonical memories + 100k substrate fragments: p95 < 60 seconds.
- Stream E `<pending-attention>` Pass-3 read in startup hot path: must add ≤ 5ms to Stream E's existing p95 (file read + entity-intersect + safe-fragment classifier per record; no LLM, no decryption).

The release-gate bench fixture lives at `bench/stream-f-dreaming-results.darwin-arm64.json` and is updated only by explicit human-authored commits, per the established `bench/baseline.*.json` convention. The fixture covers: 1k-fragment Pass 1 prompt assembly, lease acquisition, substrate-fragment write throughput, cleanup full-pass, and the Stream E `<pending-attention>` read overhead.

## 11. Observability counters

`memoryd` adds a `dreams: DreamStatusCounters` field to `StatusResponse`, additive (old clients deserialize with zero/default counters):

```rust
pub struct DreamStatusCounters {
    pub substrate_fragments_written_total: BTreeMap<String, u64>,    // keyed by ObserveKind
    pub dream_runs_invoked_total: u64,
    pub dream_runs_failed_total: BTreeMap<String, u64>,              // keyed by error code
    pub pass_failed_total: BTreeMap<String, u64>,                    // keyed by "pass_<n>:<code>"
    pub harness_cli_calls_total: BTreeMap<String, u64>,              // keyed by cli name
    pub harness_cli_auth_failures_total: BTreeMap<String, u64>,      // keyed by cli name
    pub cleanup_runs_invoked_total: u64,
    pub cleanup_findings_total: BTreeMap<String, u64>,               // keyed by finding type
}
```

Stream E's `RecallStatusCounters` gains one field for the Pass-3 hook (additive to the v0.5 shape):

```rust
pub struct RecallStatusCounters {
    // ... existing v0.5 fields ...
    pub dream_question_omitted_total: BTreeMap<String, u64>,
    // keyed by: "cap_section" | "cap_total" | "no_entity_match" | "unsafe_fragment" | "malformed_record"
}
```

Counters reset on daemon restart (consistent with Stream E recall counters). Persistence is explicitly deferred to a later stream (§13).

## 12. Acceptance signals

Implementation is complete when these tests/docs exist and pass:

- `crates/memoryd/tests/dream_substrate_fragments.rs`
  - `memory_observe { kind: Observation|Pattern|Signal }` appends a substrate fragment to the current device's file;
  - `memory_observe` with PII content routes to `encrypted/substrate/...` and writes the encrypted JSONL shape from §5.1.1;
  - `memory_observe` with secret content is refused with `WriteFailureKind::SecretRefused`;
  - `memory_note` is unchanged: it writes a canonical memory and only that (regression test);
  - fragments outside the lifetime window are archived by the cleanup layer.
- `crates/memoryd/tests/dream_canonical_isolation.rs` (the §1.1 invariant 9 test)
  - frontmatter-free `dreams/journal/<scope_path>/<date>.md` is path-valid under Stream A's tree validator;
  - `dreams/questions/<scope_path>/<date>.jsonl`, `dreams/cleanup/<device>/<date>.json`, `substrate/<device>/<date>.jsonl`, and `leases/journal.lease` are also path-valid;
  - none of the above are returned by `Substrate::query_memory`, `query_recall_index`, or `query_chunks`;
  - `Substrate::read_memory_envelope` on any of these paths returns `NotACanonicalMemory`.
- `crates/memoryd/tests/dream_lease_election.rs`
  - single-device manual lease acquisition succeeds;
  - second device observing an active lease aborts with `lease_held` and exit code 5;
  - push race re-fetches and retries up to 3 times then aborts with `lease_unavailable`;
  - `--force` overrides an active lease;
  - dirty working tree (file modified outside `leases/journal.lease`) aborts with `lease_dirty_tree`;
  - daemon-authored lease commit uses `memoryd lease-bot` author and the documented message format.
- `crates/memoryd/tests/dream_lease_scheduled_retry.rs`
  - simulated transient `git fetch` failure within the retry window eventually succeeds and runs the dream;
  - simulated persistent `git fetch` failure across the entire window records a missed-run summary with `consecutive_missed_runs: 1`;
  - next-day successful run resets `consecutive_missed_runs` to 0;
  - `dream_retry_window_minutes: 0` disables retries and aborts immediately on first failure.
- `crates/memoryd/tests/dream_pass_pipeline.rs` (uses `EchoCli` fixture exclusively)
  - Pass 1 produces `dreams/journal/<scope_path>/<date>.md`;
  - Pass 2 produces N candidates that land in the candidate queue under `dreaming-strict`;
  - Pass 2 candidates with hallucinated refs (refs not in evidence catalog) are rejected at validation;
  - Pass 2 candidates with valid refs are restored correctly via `MaskingSession::restore`;
  - Pass 3 produces `dreams/questions/<scope_path>/<date>.jsonl` with valid `{entities, question}` records;
  - Pass 3 records with hallucinated entity ids are discarded with `dream_question_omitted_total{reason: malformed_record}` increment;
  - empty Pass 1 output aborts the run;
  - malformed Pass 2 JSON triggers the one-shot retry; second malformed response fails Pass 2 but Pass 3 still runs;
  - Pass 1/3 outputs are never restored (assert masked tokens remain present);
  - the `MaskingSession` is dropped after every run regardless of outcome (instrumented `Drop` assertion).
- `crates/memoryd/tests/dream_grounding_rehydration.rs`
  - a Pass 2 candidate with a `source.ref` that no longer resolves at promote time is quarantined with `reason: grounding_rehydration_failed`;
  - a candidate whose cited substrate fragment has aged past the lifetime window is quarantined;
  - a candidate whose cited file content has shifted beyond `dreams.pass_2_drift_threshold` is quarantined.
- `crates/memoryd/tests/dream_harness_cli.rs`
  - `EchoCli` replays canned outputs deterministically;
  - `ClaudeCodeCli::is_installed()` reflects PATH presence (test installs a stub `claude` binary);
  - subprocess timeout sends SIGTERM after `timeout` and SIGKILL after `+ 2s`;
  - subprocess never inherits the user's project working directory;
  - subprocess env contains only the documented allowlist;
  - prompt text appears in subprocess stdin (per `prompt_transport(): stdin`) and **never** in argv, stderr, or any log output;
  - any adapter declaring `prompt_transport(): argv` is documented in the privacy disclosure (no v0.2 adapter ships with argv fallback unless reviewed).
- `crates/memoryd/tests/dream_cleanup.rs`
  - cleanup is idempotent across re-runs;
  - cleanup is commutative across two concurrent devices (simulated);
  - every operation produces the documented findings shape;
  - `observed_at` refresh is deterministic with respect to file mtime fixtures;
  - daemon-authored cleanup commit uses `memoryd cleanup-bot` author and the documented message format;
  - dirty-tree behavior: cleanup writes its report but defers the commit, recording `commit_deferred: true`.
- `crates/memoryd/tests/dream_recall_integration.rs`
  - `dreams/questions/<scope_path>/<date>.jsonl` content surfaces in Stream E `<pending-attention>` when entities intersect the recall seed set;
  - records with empty `entities` never surface;
  - section caps (2 per scope, 6 total) are respected;
  - deterministic surfacing order matches the §1.1 spec (entity-overlap → recency → novelty → lex);
  - `safe_plaintext_fragment`-classified-unsafe questions are silently omitted with `dream_question_omitted_total{reason: unsafe_fragment}` increment;
  - cap-driven omissions increment `cap_section` / `cap_total` per the documented keys.
- `crates/memoryd/tests/dream_cli.rs`
  - `memoryd dream status` reports CLI inventory, last runs, active leases, and the §14 privacy disclosure on the first line;
  - `memoryd dream now --scope ... --cli ...` runs end-to-end with `EchoCli`;
  - `memoryd dream review --since 7d` lists journal/question/candidate output;
  - `memoryd dream enable`/`disable` toggle the device-local sentinel.
- `docs/api/stream-f-dreaming-api.md`
  - documents MCP, daemon, and CLI surfaces with worked examples;
  - top-of-document privacy disclosure naming the harness-CLI delegation, the `prompt_transport` per adapter, the substrate-sync surface, and pointing to each upstream's data policy.
- `docs/api/stream-a-public-api.md`
  - notes the new top-level paths (`substrate/`, `encrypted/substrate/`, `dreams/`, `leases/`) and their merge-driver semantics.
- `docs/api/stream-b-daemon-mcp-api.md`
  - documents the `memory_observe` tool and the unchanged `memory_note`.
- `docs/api/stream-d-privacy-api.md`
  - notes Stream F as the first user of `MaskingSession` and confirms the `Drop`-based teardown contract.
- `docs/api/stream-e-passive-recall-api.md`
  - notes the Pass-3 question hook (cross-link only; no v0.6 spec bump unless Trey approves).
- `README.md` and `CLAUDE.md`
  - note Stream F shipped only after the tests above pass.

## 13. Explicit deferrals

These are intentionally outside Stream F v0.2:

- **Generic `LlmProvider` trait with bring-your-own API keys.** If a future user wants to dream without an installed harness CLI, that's a v0.3+ feature. v0.2 is harness-CLI-only.
- **Tiebreak provider integration.** Stream C's `ContradictionTiebreaker` trait may later ride the same `HarnessCli` abstraction Stream F builds, but v0.2 does not wire it. The integration is one task in a follow-up stream.
- **Embedding inference.** Untouched; not in scope.
- **Privacy Filter (ONNX) integration.** Untouched; not in scope.
- **Streaming model output.** All passes are blocking call-and-collect. Streaming is a v0.3+ optimization for long Pass 1 runs.
- **Cross-device journal merging.** Each scope's journal is written by whichever device held that scope's lease that day. Devices on different days produce different files; there is no "merge two devices' Pass 1 narratives into one." If two devices race past lease and both write the same date's file, Stream A's three-way merge driver records a quarantine event and operator chooses; this is a diagnostic, not normal operation.
- **Pass 2 auto-promotion.** Every Pass-2 candidate goes to the candidate queue under `dreaming-strict`. Auto-promotion bypasses the human/governance gate and is explicitly out of scope.
- **Real-time / event-driven dreaming.** v0.2 is daily + on-demand-via-CLI only. Triggering a dream pass on every Nth substrate fragment, on entity-cluster size threshold, or on user idle is a v0.3+ feature.
- **Persistent counters.** `DreamStatusCounters` and the new `RecallStatusCounters.dream_question_omitted_total` are in-process only, reset on daemon restart. Persistence is deferred to a later stream that owns operational telemetry across all streams (Stream G or H, TBD).
- **Dashboard UI for dream review.** `memoryd dream review` is CLI only. Visual dashboard is Stream G.
- **Live cross-device dream visibility.** Stream I owns event subscriptions; Stream F's dream activity is visible via `dreams/...` files in git history but not via a real-time push channel.
- **Doctor projection in `<pending-attention>`.** Stream E v0.5 §9.5 deferred this; Stream F does not unblock it. A future stream lands a daemon-cached doctor projection.
- **Argv-fallback harness CLI adapters.** No v0.2 adapter ships with `prompt_transport(): argv` unless Trey explicitly approves the declaration. The `HarnessCli` trait permits it; v0.2 ships only stdin-supporting adapters.

If an implementation needs one of these to pass the v0.2 acceptance tests, the spec should be revised before coding continues.

## 14. Privacy disclosure (user-facing)

Every documentation surface that mentions dreaming — `docs/api/stream-f-dreaming-api.md`, the README's Stream F section, `memoryd dream status` output (first line), and the `memoryd dream enable` first-run confirmation — must include this disclosure verbatim or in close paraphrase:

> Dreaming uses whichever agent-harness CLI you have installed and authenticated on this device (Claude Code, Codex CLI, Gemini, etc.). Dream prompts are masked through the agent-memory privacy filter before they leave the daemon, but the masked text is processed by the harness CLI's upstream model provider. The data, retention, and training policies of that provider apply. Where this device's selected harness CLI accepts prompts on stdin, the prompt is not visible to other local processes; where it does not, the prompt may be visible via process listing tools (`ps`, `top`, `/proc/<pid>/cmdline`). `memoryd dream status` shows the prompt-transport mode for each installed harness adapter. Substrate fragments written via `memory_observe` are git-synced as low-level durable telemetry; this means the private git repo's raw-observation surface is larger than its canonical-memory surface, even though substrate is not searchable as memory. If you don't want dream content sent to a particular provider, set the per-scope CLI priority to exclude it, or run `memoryd dream disable` on this device.

This is not buried. It is the first paragraph of the public API doc and the first line of `memoryd dream status` when dreaming is enabled.

---

## Amendment 2026-07-11 — `abstraction_compile` dream job (in-version, additive)

Ratified 2026-07-10 as §B of `docs/specs/2026-07-10-w2-spec-ratification-package.md` (Memora-lessons arc, plan `docs/plans/2026-07-10-memora-lessons-memorum-upgrades.md` §W2 task 6). New optional job type; no change to existing pass behavior.

- New dream job **`abstraction_compile`**: selects active/pinned memories lacking `abstraction` (or whose `abstraction_hash` predates the current body hash per repair policy — see the one-directional `source_body_hash` freshness semantics in Stream A v1.2 §A2 note), mints `abstraction` (≤8 words) + `cues` (0–3, `[Main Entity] + [Key Aspect]` guidance) via the **existing harness-CLI dream machinery** — no daemon-resident LLM.
- Output is untrusted input: machine-verified against Stream A v1.2 §A1 caps/charset before use; malformed output = skip item, log, continue (the `malformed_pass_2_json` lesson).
- Application = **governed supersede** through the standard write path, carrying a fresh `ClassificationOutcome` per the Stream A v1.2 §A4 composed pipeline (generation-context dual classification: drop-fields-keep-body + outcome rebind on sensitive generation).
- Structural fallback when no harness CLI is available: `abstraction` = `summary` truncated to caps, no cues, marked `source: structural` in the job report.
- This job is the single generation mechanism for the eval-corpus backfill (W4-prep), the live-corpus backfill (W5), and ongoing dream repair — production parity by construction.
