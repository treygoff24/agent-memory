# Stream F Dreaming Spec v0.1

**Status:** first implementation contract for Stream F dreaming.
**Date:** 2026-04-30.
**Sources:** `docs/specs/system-v0.1.md` §12 (dreaming), §11 (governance machinery), and the shipped Stream A–E contracts.
**Non-source:** older drafts, brainstorm notes in `docs/handoff-2026-04-23.md`, and the handbook are background; they are not normative for this spec.

Stream F turns the agent-memory system from "memoryful at startup" into "memoryful over time." It implements the three-layer dreaming pipeline (substrate / journal / cleanup) described in system spec §12 — but with one consequential simplification: Stream F does **not** build a generic LLM provider, manage API keys, or own model selection. Instead, the daemon delegates each dream pass to whichever agent-harness CLI is already installed and authenticated on the dreaming device (`claude -p`, `codex exec`, etc.). If the user has installed a supported harness, dreaming works for free; if not, dreaming is unavailable and that is a reported, recoverable state, not a failure.

This factoring is deliberate. Stream F's value is the cognitive pipeline (substrate fragment lifecycle, lease-elected daily journal runs, masked synthesis, grounding-rehydrated promotion, idempotent cleanup), not the LLM call surface. By piggybacking on the user's existing harness CLI, Stream F inherits the user's auth, billing, model choice, and offline behavior without owning any of them.

## 1. Scope and dependency boundaries

Stream F owns:

- substrate-fragment write surface (`memory_note` with `kind: observation|pattern|signal`) and the `substrate/` per-device JSONL file series with 14-day archival lifetime;
- daily journal layer with three LLM-backed passes (`why`, `what should change`, `uncomfortable question`) and a leased-device election;
- nightly cleanup layer (idempotent janitorial operations on canonical memory state);
- the harness-CLI provider abstraction that brokers dream passes through installed agent CLIs;
- per-scope CLI priority configuration and lease eligibility;
- masked-synthesis integration: dream prompts run on Stream D-masked text; restoration happens on Pass 2 candidate write-back only;
- candidate-promotion grounding rehydration — re-resolve cited source refs at promote time, skip on drift;
- Stream E `<pending-attention>` Pass-3-question hook (deferred from Stream E v0.5 §15);
- `memory dream {status,now,review,disable,enable}` CLI;
- new top-level repo paths: `substrate/`, `dreams/journal/`, `dreams/questions/`, `leases/`.

Stream F does not own:

- generic LLM provider abstraction with bring-your-own API keys, HTTP retries, token accounting, or cost ceilings;
- model selection — the user's chosen harness CLI determines the model;
- contradiction-tiebreak provider integration (Stream C ships the `ContradictionTiebreaker` trait; whether it later rides the same harness-CLI mechanism Stream F builds is an explicit follow-up, not part of v0.1);
- embedding inference (Stream A/B; out of scope and not unblocked by this spec);
- privacy-filter inference (Stream D; Layer 1 deterministic classifier remains the live path; ONNX model loading remains deferred);
- canonical memory mutation, governance lifecycle, privacy classification, encryption, recall block assembly — those remain Streams A/C/D/E respectively;
- dashboard UI for dream review (Stream G);
- live peer presence, claim locks, or cross-device journal merging (Stream I).

Stream F must not create a hidden second persistence layer. Substrate fragments are canonical files under git-synced `substrate/` (per-device prefix to avoid merge conflicts); journal/question outputs are canonical files under `dreams/`; leases are canonical files under `leases/`. All of them participate in Stream A's existing watcher / event log / git sync machinery without modification.

### 1.1 Cross-stream surface changes required by Stream F

Implementation of this spec lands surface additions on already-shipped streams. They are part of the Stream F v0.1 contract.

**Stream A — canonical tree-layout extension (Stream A spec amendment §16.x):**

Three new top-level directories must validate under `Substrate::tree::validate`:

- `substrate/<device_id>/<YYYY-MM-DD>.jsonl` — append-only per-device JSONL substrate fragments. `<device_id>` is the existing device identity (per Stream A `git::adopt_clone`). Files older than the configured fragment lifetime move to `substrate/archive/<device_id>/<YYYY-MM>.jsonl` (year-month bucketed for compactness).
- `dreams/journal/<scope>/<YYYY-MM-DD>.md` — Pass 1 narrative output. Scope is `me`, `project:proj_<id>`, `agent`, or `org:org_<id>`. Markdown body, no frontmatter (these are not canonical memories; they are NOT a grounding source per Stream C).
- `dreams/questions/<scope>/<YYYY-MM-DD>.md` — Pass 3 adversarial questions. Same shape and non-grounding rule as journal.
- `leases/journal.lease` — JSONL lease file (one record per active lease window per scope). Existing record format from system spec §12.2.

These paths are **not** subject to the canonical memory frontmatter schema. They have their own validators in `crates/memory-substrate/src/tree.rs`. Stream A's three-way merge driver must treat substrate JSONL files as append-only (concat + sort by `id`) and journal/question Markdown files as last-writer-wins by date+device (collisions are diagnostics, not blockers — two devices wrote the same scope's journal on the same date because the lease was contested).

**Stream B — `memory_note` MCP tool extension:**

```rust
// existing
struct MemoryNoteRequest {
    text: String,
}
// new in Stream F v0.1:
struct MemoryNoteRequest {
    text: String,
    #[serde(default)]
    kind: NoteKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entities: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NoteKind {
    #[default]
    Note,         // current behavior: canonical memory under notes/ namespace
    Observation,  // substrate fragment, kind=observation
    Pattern,      // substrate fragment, kind=pattern
    Signal,       // substrate fragment, kind=signal
}
```

Default `NoteKind::Note` preserves the shipped behavior exactly. Non-`Note` kinds bypass the canonical memory write path and append a substrate fragment to the current device's substrate file. The MCP manifest and daemon protocol both gain the new fields with `serde(default)` so existing clients remain compatible.

**Stream C — grounding rehydration enforcement:**

Stream C v0.1 already accepts `grounding_rehydration_required: true` on candidate proposals; Stream F is the first writer that sets this flag and the first consumer that needs it enforced. Stream F lands the enforcement: at promote time, every cited `source.ref` in a dream-authored candidate is re-resolved against the live substrate. If any cited file is missing, content-shifted beyond a configured Levenshtein/hash threshold, or pointing at a substrate fragment that has aged past the fragment lifetime, the candidate is **not promoted** and is moved to `status: quarantined` with `reason: grounding_rehydration_failed`. This is a deterministic check; it does not call any LLM.

**Stream D — masked-synthesis session integration:**

Stream D ships `MaskingSession` with salt-table-based round-trip token replacement. Stream F is the first user. The contract is unchanged; Stream F just adheres to it: every dream prompt input passes through `MaskingSession::mask`; Pass 2 candidate output passes through `MaskingSession::unmask` before write-back; the salt table is daemon-local, never written to disk, and cleared via `MaskingSession::end` at the end of each per-scope dream run regardless of pass success/failure. Pass 1 and Pass 3 outputs are journal markdown that does NOT go through unmask — they remain as the model produced them, which is masked.

**Stream E — `<pending-attention>` Pass-3 hook:**

Stream E v0.5 §15 explicitly defers Pass 3 question surfacing to Stream F. Stream F lands the wiring: the recall-block builder reads `dreams/questions/<scope>/<YYYY-MM-DD>.md` for the most recent date <= today for each scope in `namespaces_in_scope`, performs entity/alias intersection against the active recall seed set, and emits matching questions as `<pending-attention>` line items. Surfacing rules:

- one line per matching question, capped at 3 questions per scope, capped at 8 total in the section;
- format: `- [<scope>] <question text>` with question text bounded to 240 UTF-8 bytes (same rule as memory summaries);
- questions are surfaced **only** when at least one entity in the question text matches an entity in the active recall seed set; pure-prose questions with no entity hooks never surface;
- safe-fragment classification (Stream D `safe_plaintext_fragment`) runs on every question before emission; classified-unsafe questions are silently omitted (these are masked-synthesis outputs and should be safe by construction, but defense-in-depth);
- this hook runs in Stream E's startup-recall hot path; reading the questions file must be O(file size) with no LLM call, no rerun of Pass 3, and no I/O outside the file read.

The `policy` attribute on the recall block does **not** bump for this addition (additive feature; existing `stream-e-v0.5` policy string covers it). A Stream E spec amendment to v0.6 documents the unhide.

## 2. Safety invariants

1. **Dream prose is never a grounding source.** Pass 1 narrative and Pass 3 questions are explicitly excluded from Stream C grounding-ref resolution. A candidate citing `dreams/journal/...` or `dreams/questions/...` as `source.ref` is refused at write time with `WriteFailureKind::DreamProseAsSource`.

2. **Pass 2 candidates always go to the candidate queue.** Stream F never auto-promotes a Pass-2-authored candidate. `dreaming-strict` policy applies. Candidates may be promoted later by Stream G review UI or by `memoryd review approve`; Stream F does not bypass that gate.

3. **Masked synthesis is mandatory.** Every dream prompt's input text is masked through `MaskingSession::mask` before any harness CLI is invoked. The salt table is daemon-process-local, in-RAM only, and is cleared at end-of-run. A test that exercises a dream pass must verify no unmasked sensitive token reaches the harness CLI's argv or stdin.

4. **No `memory_reveal` from any dream pass.** Encrypted memories may contribute only their summary, tags, entity labels, and Stream D safe descriptors to Pass 1/2/3 inputs — same constraint as Stream E recall.

5. **Lease holder is honest.** The lease record contains the device id; only the device that wrote the active lease may write that scope's journal/question files for the lease's date. A device that observes a foreign device's active lease must not run a dream for that scope, even if it has a faster/different harness CLI installed.

6. **Cleanup is commutative and idempotent.** Every cleanup operation produces the same final state regardless of execution order, repetition, or concurrent execution by sibling devices. Cleanup never deletes a memory body — it can only flip status, archive substrate fragments, or compact event log entries past retention.

7. **Harness CLI calls fail closed.** A harness CLI subprocess that times out, exits non-zero, returns malformed structured output (after one retry for Pass 2), or signals authentication failure aborts the affected pass. The lease is released. Partial state (e.g., Pass 1 wrote successfully but Pass 2 failed) is committed: Pass 1's journal stays on disk; Pass 2 emits no candidates; Pass 3 still runs if Pass 1 succeeded.

8. **Substrate fragments respect classification.** A `memory_note` with `kind: observation|pattern|signal` runs through Stream D's `DeterministicPrivacyClassifier` over the text before any disk effect, exactly as canonical writes do. Refused tiers are refused with no fragment written. PII tiers route to an `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl` parallel file series (encrypted-at-rest); journal/question passes never decrypt these. The Pass 1 prompt sees only the plaintext substrate fragments; encrypted fragments contribute their (already safe) descriptor projection only.

9. **Output is reproducible across re-runs only at the prompt-input level.** Given the same substrate fragments, active memory set, masked-synthesis salt seed, and harness CLI choice, Stream F produces a byte-identical *prompt input*. The harness CLI's output is non-deterministic by construction; tests assert prompt determinism, not response determinism. The test fixture `EchoCli` lets acceptance tests pin response shape.

10. **Errors are typed.** Daemon protocol error codes are stable; CLI exit codes are stable; users do not parse free-form prose to detect failure.

## 3. Public surfaces

### 3.1 MCP `memory_note` extension

The existing `memory_note` MCP tool gains optional `kind` and `entities` fields:

```json
{ "text": "Third time investigating JWT validation in this repo — pattern emerging around key rotation.",
  "kind": "pattern",
  "entities": ["ent_auth_flow", "ent_jwt"] }
```

Backward compatibility: omitted `kind` defaults to `note`; omitted `entities` defaults to `[]`. The shipped `{ "text": "..." }`-only shape continues to write a canonical memory under the `notes/` namespace exactly as it does today.

### 3.2 Daemon protocol additions

```rust
// Existing variant, fields extended (additive):
RequestPayload::WriteNote {
    text: String,
    #[serde(default)] kind: NoteKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] entities: Vec<String>,
}

// New variant: explicit dream-trigger for `memory dream now`.
RequestPayload::DreamNow {
    scope: String,                 // "me" | "agent" | "project:..." | "org:..."
    force: bool,                   // bypass lease-already-held check; for tests/admin
    cli_override: Option<String>,  // bypass per-scope priority for one run
}

// New variant: dream status query. Read-only.
RequestPayload::DreamStatus {}
```

Response payloads:

```rust
ResponsePayload::WriteNote(WriteNoteResponse)            // existing; extended for substrate-fragment ack
ResponsePayload::DreamNow(DreamRunReport)
ResponsePayload::DreamStatus(DreamStatusReport)

struct WriteNoteResponse {
    id: String,
    summary: String,
    kind: NoteKind,                                       // echoes the kind that was written
    target: WriteNoteTarget,                              // canonical | substrate
}

struct DreamRunReport {
    scope: String,
    cli_used: String,                                     // "claude" | "codex" | ...
    pass_1: PassOutcome,
    pass_2: PassOutcome,
    pass_3: PassOutcome,
    duration_ms: u64,
}

struct PassOutcome {
    status: PassStatus,                                   // success | skipped | failed
    output_path: Option<String>,                          // dreams/journal/<scope>/<date>.md, etc.
    candidate_ids: Vec<String>,                           // populated only for Pass 2
    error_code: Option<String>,                           // populated only for status=failed
    duration_ms: u64,
}

#[serde(rename_all = "snake_case")]
enum PassStatus { Success, Skipped, Failed }

struct DreamStatusReport {
    enabled: bool,
    last_runs: Vec<ScopeRunSummary>,                      // most recent run per scope
    active_leases: Vec<LeaseRecord>,
    cli_inventory: Vec<HarnessCliStatus>,
    counters: DreamStatusCounters,
}
```

### 3.3 CLI surfaces

```bash
memoryd dream status [--repo .] [--runtime .memoryd]
memoryd dream now --scope <me|agent|project:...|org:...> [--force] [--cli claude|codex|...]
memoryd dream review --since <duration>             # walk recent journal/question/candidate output
memoryd dream enable
memoryd dream disable
```

`memoryd dream status` writes a structured human-readable report to stdout (JSON only with `--json`). `memoryd dream now` writes a `DreamRunReport` to stdout (JSON with `--json`, otherwise human-readable). `memoryd dream review` walks `dreams/journal/`, `dreams/questions/`, and the candidate queue under `dreaming-strict` for the requested window.

Exit codes match Stream E conventions:

- `0` — operation succeeded;
- `1` — `invalid_request` (bad scope, missing CLI override, malformed config);
- `2` — `dream_unavailable` (no eligible harness CLI installed/authenticated, or daemon unreachable);
- `3` — `privacy_error` (Stream D refused fragment write or unmask diagnostic);
- `4` — `dream_pass_failed` (one or more passes failed; partial output may be on disk).

### 3.4 Configuration surface

Device-local `config.yaml` gains a `dreams` block. Per-scope CLI priority is **synced** (so all devices agree which CLI a scope dreams via); CLI **availability** is local (so a device that lacks a priority CLI is simply ineligible).

```yaml
# config.yaml — synced top-level keys
dreams:
  enabled: true
  default_cli_priority: [claude, codex]
  scope_overrides:
    me: [claude]
    project:proj_abc: [codex, claude]
    agent: [claude]
  per_pass_timeout_seconds: 300            # 5 min default per pass
  fragment_lifetime_days: 14
  pass_2_max_candidates: 8                 # cap per scope per run
  pass_3_max_questions: 12                 # cap per scope per run
  cleanup_run_hour_utc: 3                  # daily cleanup window start
```

Validation:

- `default_cli_priority` and each `scope_overrides[*]` value must be a non-empty list of known harness names; unknown names fail config load (fail-closed).
- `per_pass_timeout_seconds` must be in `[30, 1800]`; default 300.
- `fragment_lifetime_days` must be in `[1, 365]`; default 14.
- Caps must be in `[1, 64]`.

A separate device-local sentinel `~/.memoryd/dream-disabled` (created by `memoryd dream disable`) overrides synced `enabled: true`. This is how a single device opts out without changing the synced repo state — useful for "this device is offline, don't take leases."

## 4. Harness-CLI provider abstraction

### 4.1 Trait

```rust
#[async_trait]
pub trait HarnessCli: Send + Sync {
    /// Stable identifier used in config (`claude`, `codex`, `gemini`, ...).
    fn name(&self) -> &'static str;

    /// Detect whether the underlying binary is available on PATH.
    /// O(1), cached, refreshed on `cli_inventory_refresh()`.
    fn is_installed(&self) -> bool;

    /// Probe authentication. Implementations call a cheap auth-only
    /// command (e.g. `claude --version` + `claude config get auth.user`).
    /// Returns `Ok(true)` only on confirmed authenticated state.
    async fn is_authenticated(&self) -> Result<bool, HarnessCliError>;

    /// Run a dream pass. `prompt` is masked. `expect_json` is true for Pass 2.
    /// Implementations construct the harness-specific argv (`-p`, `exec`, etc.),
    /// run the subprocess via `tokio::task::spawn_blocking`, apply the configured
    /// per-pass timeout, return the raw stdout text (or parsed-and-validated
    /// JSON for Pass 2).
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

Stream F v0.1 ships these adapters. Each has a small unit test that records the argv it would invoke without actually spawning the subprocess.

- `ClaudeCodeCli` — invokes `claude -p <prompt>`, reads stdout. Auth probe: `claude config get auth.user` exit code.
- `CodexCli` — invokes `codex exec --json <prompt>` for `expect_json=true`, otherwise `codex exec <prompt>`. Auth probe: `codex auth status` exit code.
- `GeminiCli` — invokes `gemini -p <prompt>`. Auth probe per upstream conventions; if the Gemini CLI does not yet have a non-interactive auth probe, this adapter reports `is_authenticated() = Ok(false)` with a hint string until upstream lands one.
- `EchoCli` — test-only, replays canned outputs from a `HashMap<PromptHash, String>` fixture, never spawns a subprocess. Used by every Stream F acceptance test.

`DroidCli` and `OpenCodeCli` are not in v0.1 but are explicitly addable as v0.2 adapters without spec changes. The `HarnessCli` trait is the durable contract; new adapters extend the registry, not the trait.

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
- captures stdout up to a 16 MiB cap (Pass 1/3 prose has been observed at ~30 KB; Pass 2 JSON at ~5 KB; the cap is paranoia, not policy);
- captures stderr up to 64 KiB and surfaces the tail in `HarnessCliError::SubprocessExit::stderr_tail` for diagnostic logging;
- never echoes the prompt to stderr, never writes the prompt to disk, never logs the prompt at any verbosity (the prompt may contain masked-but-still-private content).

## 5. Substrate layer

### 5.1 Fragment write surface

`memory_note` with `kind: observation|pattern|signal` appends a JSONL record to the current device's substrate file:

```jsonl
{"id":"sub_<ulid>","ts":"2026-04-30T14:22:10Z","device":"dev_<id>","session":"sess_<id>","harness":"claude-code","scope":"project:proj_<hex>","entities":["ent_auth_flow"],"kind":"observation","text":"User corrected: we don't use HS256; we use RS256 with a rotating key.","source_ref":"session:sess_<id>:turn:47","privacy_spans":[]}
```

Fragment ids are ULIDs so timestamps are monotonic per device and intra-second collisions are impossible. `device`, `session`, `harness`, `scope`, `source_ref`, and `privacy_spans` are populated identically to canonical Stream D writes from the same caller context. Stream F does not invent new metadata fields here.

The write path:

1. `memoryd` receives `WriteNote { text, kind: Observation|Pattern|Signal, entities }`.
2. Stream D `DeterministicPrivacyClassifier` runs over `text` and yields `ClassificationOutcome` plus `privacy_spans`.
3. Refused tiers (Secret, identity-theft) return `WriteFailureKind::SecretRefused` exactly as today; no fragment written.
4. PII tiers route to `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl` (age-encrypted file series, parallel to canonical encrypted memories). Pass 1 input never reads encrypted fragments.
5. All other tiers append to plaintext `substrate/<device_id>/<YYYY-MM-DD>.jsonl`.
6. Append is via the existing Stream A atomic-append pattern with `fsync` per record (substrate fragments are durable, like events).
7. The watcher emits a `SubstrateFragmentWritten` event onto the per-device JSONL event log. This event is informational only; nothing in the hot path consumes it. The event makes substrate-fragment activity observable to operator tooling.

### 5.2 Fragment lifetime and archival

A fragment's `ts` plus `dreams.fragment_lifetime_days` (default 14) determines its expiry. Cleanup (§7) archives expired fragments into `substrate/archive/<device_id>/<YYYY-MM>.jsonl` once per day per device. Archival is concat-and-sort-by-id; archival files are append-only across days; archival operations are idempotent (re-archiving a fragment that's already in the archive is a no-op).

Archived fragments remain visible to Pass 1 reads if they fall within the `--since` window the journal layer requested (default 7 days for daily, 30 for weekly). Archived fragments are not visible to substrate-fragment search by Stream B `memory_search`; substrate fragments are not in the canonical memory index. This is intentional: substrate is "raw observations the daemon's still chewing on," not "search me later."

## 6. Journal layer

### 6.1 Lease election

`leases/journal.lease` is JSONL, one record per active lease window per scope:

```jsonl
{"device":"dev_macbook","scope":"me","acquired_at":"2026-04-30T03:00:00Z","expires_at":"2026-04-30T04:00:00Z","run_id":"run_<ulid>"}
{"device":"dev_macbook","scope":"agent","acquired_at":"2026-04-30T03:00:10Z","expires_at":"2026-04-30T04:00:00Z","run_id":"run_<ulid>"}
```

A device attempting a dream for scope X:

1. `git fetch origin` (best-effort; on network failure, abort with `lease_unavailable`).
2. Read `leases/journal.lease`. Filter to records for scope X with `expires_at > now`.
3. If a non-empty subset remains, the scope is leased — abort this run with `lease_held{by_device}`. Do not retry within the lease window.
4. Otherwise, append a new lease record with `acquired_at = now`, `expires_at = now + lease_window` (default 1 hour), commit, push.
5. On push race (rejected non-fast-forward), re-fetch and goto 2. Up to 3 retries with 200ms backoff, then abort with `lease_unavailable`.
6. After successful push, the device holds the lease and may proceed with Pass 1.
7. After all passes complete (or any pass fails), append a `lease-released` record (or just let it expire — the lease record is informational, not durable state).

The lease window is `dreams.lease_window_seconds` config (default 3600). A lease can be force-released with `memoryd dream now --force` for admin/test use; this writes a release record bypassing expiry.

### 6.2 Pass 1 — "Why did this happen this way?"

Inputs:

- substrate fragments in scope from the last `pass_1_window_days` (default 7) excluding encrypted fragments;
- pinned and active memories in scope (read via Stream A `query_recall_index` with `passive_recall_only: true`);
- recent governance decisions (Stream C candidate-queue head + recent supersession/tombstone activity).

Processing:

1. Cluster fragments by entity co-occurrence — deterministic graph clustering on `entities[]` overlap, no LLM call. Tie-broken by fragment id.
2. Mask the entire input through `MaskingSession::mask`. The salt table now contains every masked token's reverse mapping.
3. Build the prompt from a versioned template (`prompts/dream-pass-1-v1.md`, checked into the repo). The template is a hand-written agent-targeted prompt that requests narrative prose, ~800 to ~2000 words, no JSON.
4. Call `HarnessCli::complete(prompt, expect_json: false, timeout: per_pass_timeout)`.
5. The response is masked-text Markdown. Write it directly to `dreams/journal/<scope>/<YYYY-MM-DD>.md`. Do **not** unmask — Pass 1 output is never a grounding source and never read back into anything that would attempt to use the masked tokens as facts.

Failure modes:

- `HarnessCliError::NotAuthenticated` → `pass_1: failed{auth}`, abort entire run, release lease.
- `HarnessCliError::Timeout` → `pass_1: failed{timeout}`, abort, release.
- `HarnessCliError::SubprocessExit` → `pass_1: failed{subprocess, code, stderr_tail}`, abort, release.
- Empty/whitespace-only response → `pass_1: failed{empty_output}`, abort, release. Pass 1 produced nothing useful; running Pass 2/3 against an empty Pass 1 input is wasted work.

### 6.3 Pass 2 — "What should change?"

Inputs:

- Pass 1 output (still masked);
- the same active-memory set Pass 1 saw;
- a JSON schema for candidate proposals.

Processing:

1. Build the prompt from `prompts/dream-pass-2-v1.md`. The template instructs the model to emit a JSON array of candidate-proposal objects matching this schema:

```json
[
  {
    "claim": "<masked claim text>",
    "namespace": "project:proj_abc",
    "kind": "decision",
    "evidence": [
      { "kind": "substrate_fragment", "ref": "sub_01J...", "excerpt": "<masked>" },
      { "kind": "memory", "ref": "mem_<id>", "excerpt": null }
    ],
    "confidence": 0.7,
    "rationale": "<short masked rationale>"
  }
]
```

2. Call `HarnessCli::complete(prompt, expect_json: true, timeout: per_pass_timeout)`.
3. Parse stdout as a JSON array of `Pass2Candidate`. On parse failure, retry once with a corrective preamble appended to the prompt (`Your previous response was not valid JSON. Please return only a JSON array conforming to the schema above.`). On second failure, `pass_2: failed{malformed_json, raw_tail}`, abort Pass 2, continue to Pass 3.
4. Validate every candidate against the schema and config caps:
   - `namespace` must match an in-scope namespace per §6.1's lease scope set;
   - `kind` must be a known canonical memory kind;
   - `confidence` must be in `[0, 1]`;
   - `evidence[]` must contain at least one entry with a resolvable `ref` (§6.3.1 below);
   - candidate count must not exceed `dreams.pass_2_max_candidates`.
5. For each surviving candidate, **unmask** the `claim`, `excerpt`, and `rationale` fields via `MaskingSession::unmask`. This is the only place in the dream pipeline where unmasking happens.
6. Write each unmasked candidate to the canonical candidate-write path (`Substrate::write_memory` with `status: candidate`, `policy: dreaming-strict`, `grounding_rehydration_required: true`). Stream C accepts or refuses each per its existing policy. Refusals are recorded in the `DreamRunReport.pass_2.candidates` with `accepted: false, reason: <code>`.
7. Mark Pass 2 as success iff at least one candidate was accepted into the queue. Zero accepted is `pass_2: skipped` (not failed) — the model produced output but governance refused everything. Operator can inspect the refusals via `memoryd dream review`.

#### 6.3.1 Evidence ref resolution

Every candidate's `evidence[]` entry must reference a real substrate fragment id (`sub_<ulid>`) or memory id (`mem_<id>`) that existed in Pass 1's input set. Pass 2 cannot fabricate evidence; refs must be drawn from the prompt's actual contents. Validation:

- `kind: substrate_fragment` → ref must be a fragment id present in this run's Pass-1 input window;
- `kind: memory` → ref must be an active/pinned memory id present in this run's input set;
- any other ref kind is rejected.

The model is instructed in the prompt to copy ref strings verbatim from the prompt input. Hallucinated refs are a deterministic-validation-layer reject, not an LLM judgment call.

### 6.4 Pass 3 — "What uncomfortable question is this system avoiding?"

Inputs:

- Pass 1 output (masked);
- a smaller summary of the active memory set (just memory ids, summaries, and entity tags — no full bodies);
- the previous N-day window's Pass 3 questions (so the model is encouraged to vary, not repeat).

Processing:

1. Build the prompt from `prompts/dream-pass-3-v1.md`. The template requests an adversarial self-critique pass producing a list of pointed questions, one per line, capped at `dreams.pass_3_max_questions`.
2. Call `HarnessCli::complete(prompt, expect_json: false, timeout: per_pass_timeout)`.
3. Write the response (still masked) to `dreams/questions/<scope>/<YYYY-MM-DD>.md`. Format: one question per line, no leading bullet (Stream E adds the bullet at recall-block render time).
4. **Do not unmask, do not promote, do not enter the candidate queue.** Pass 3 output is exclusively for Stream E `<pending-attention>` consumption.

### 6.5 Salt-table teardown

`MaskingSession::end` is called in a `finally`-style block after all three passes complete (success, partial, or full failure). The salt table is cleared from RAM. Any pass that ran after teardown (which should not happen, but defense-in-depth) sees empty restoration and emits diagnostic logs without writing.

## 7. Cleanup layer

Cleanup runs once per day per device at the configured `dreams.cleanup_run_hour_utc`. It does **not** require a lease; multiple devices running cleanup concurrently is safe by design.

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

## 8. Stream E hook for Pass 3 questions

(Detailed in §1.1 above.) Summary: Stream F's `dreams/questions/<scope>/<YYYY-MM-DD>.md` files are read by Stream E's recall-block builder during startup recall; intersecting questions surface in `<pending-attention>`. The contract is bounded (3 per scope, 8 total, 240-byte question text cap) and entity-gated (no entity match → no surface). This is the hook Stream E §15 deferred to Stream F.

## 9. Error codes

| Code | Retryable | Meaning |
|---|---|---|
| `invalid_request` | false | Bad scope, unknown CLI override, malformed config, malformed `memory_note` kind. |
| `dream_unavailable` | true | No eligible harness CLI installed/authenticated for this scope; or daemon unreachable from CLI. |
| `lease_held` | true | Another device holds this scope's lease; retry after `expires_at`. |
| `lease_unavailable` | true | `git fetch` or `git push` failed during lease acquisition; retry. |
| `dream_pass_failed` | false | At least one pass failed; details in per-pass `error_code`. |
| `privacy_error` | false | Stream D refused fragment write or unmask diagnostic. |
| `dream_disabled` | false | `dreams.enabled = false` synced or device-local sentinel present. |

`memoryd dream now --force` returns `lease_held` only when `--force` is omitted. With `--force`, the lease is overwritten and the run proceeds.

## 10. Performance requirements

Dreaming is **not** in any request hot path. Performance targets are operational, not user-facing.

- Per-pass timeout: 300s default, configurable in `[30, 1800]`.
- Total daily-run wall-clock per scope: should not exceed 20 minutes under normal harness CLI latency (Sonnet/Opus class models complete a 2k-word prose pass in ~15-60s; 3 passes × 60s = 3min, with 17min of headroom for tail latency / retries).
- Lease acquisition: p95 < 2 seconds (one `git fetch` + one read + one append + one push, no LLM).
- Substrate-fragment write: p95 < 5ms (same budget as canonical writes; the only added cost vs. canonical is one extra Stream D classifier pass on shorter text).
- Cleanup full pass over 10k canonical memories + 100k substrate fragments: p95 < 60 seconds.
- Stream E `<pending-attention>` Pass-3 read in startup hot path: must add ≤ 5ms to Stream E's existing p95 (file read + entity-intersect; no LLM, no decryption).

The release-gate bench fixture lives at `bench/stream-f-dreaming-results.darwin-arm64.json` and is updated only by explicit human-authored commits, per the established `bench/baseline.*.json` convention. The fixture covers: 1k-fragment Pass 1 prompt assembly, lease acquisition, substrate-fragment write throughput, cleanup full-pass.

## 11. Observability counters

`memoryd` adds a `dreams: DreamStatusCounters` field to `StatusResponse`, additive (old clients deserialize with zero/default counters):

```rust
pub struct DreamStatusCounters {
    pub substrate_fragments_written_total: BTreeMap<String, u64>,    // keyed by NoteKind
    pub dream_runs_invoked_total: u64,
    pub dream_runs_failed_total: BTreeMap<String, u64>,              // keyed by error code
    pub pass_failed_total: BTreeMap<String, u64>,                    // keyed by "pass_<n>:<code>"
    pub harness_cli_calls_total: BTreeMap<String, u64>,              // keyed by cli name
    pub harness_cli_auth_failures_total: BTreeMap<String, u64>,      // keyed by cli name
    pub cleanup_runs_invoked_total: u64,
    pub cleanup_findings_total: BTreeMap<String, u64>,               // keyed by finding type
}
```

Counters reset on daemon restart (consistent with Stream E recall counters). Persistence is explicitly deferred to a later stream (§13).

## 12. Acceptance signals

Implementation is complete when these tests/docs exist and pass:

- `crates/memoryd/tests/dream_substrate_fragments.rs`
  - `memory_note { kind: Note }` writes a canonical memory (regression test on shipped behavior);
  - `memory_note { kind: Observation|Pattern|Signal }` appends a substrate fragment to the current device's file;
  - PII content routes to `encrypted/substrate/...`;
  - Secret content is refused with `WriteFailureKind::SecretRefused`;
  - fragments outside the lifetime window are archived by the cleanup layer.
- `crates/memoryd/tests/dream_lease_election.rs`
  - single-device lease acquisition succeeds;
  - second device observing an active lease aborts with `lease_held`;
  - push race re-fetches and retries up to 3 times then aborts with `lease_unavailable`;
  - `--force` overrides an active lease.
- `crates/memoryd/tests/dream_pass_pipeline.rs` (uses `EchoCli` fixture exclusively)
  - Pass 1 produces `dreams/journal/<scope>/<date>.md`;
  - Pass 2 produces N candidates that land in the candidate queue under `dreaming-strict`;
  - Pass 2 candidates with hallucinated refs are rejected at validation;
  - Pass 2 candidates with valid refs are unmasked correctly via `MaskingSession`;
  - Pass 3 produces `dreams/questions/<scope>/<date>.md`;
  - empty Pass 1 output aborts the run;
  - malformed Pass 2 JSON triggers the one-shot retry; second malformed response fails Pass 2 but Pass 3 still runs;
  - Pass 1/3 outputs are never unmasked (assert masked tokens remain present);
  - the salt table is cleared after every run regardless of outcome.
- `crates/memoryd/tests/dream_grounding_rehydration.rs`
  - a Pass 2 candidate with a `source.ref` that no longer resolves at promote time is quarantined with `reason: grounding_rehydration_failed`;
  - a candidate whose cited substrate fragment has aged past the lifetime window is quarantined;
  - a candidate whose cited file content has shifted beyond the configured threshold is quarantined.
- `crates/memoryd/tests/dream_harness_cli.rs`
  - `EchoCli` replays canned outputs deterministically;
  - `ClaudeCodeCli::is_installed()` reflects PATH presence (test installs a stub `claude` binary);
  - subprocess timeout sends SIGTERM after `timeout` and SIGKILL after `+ 2s`;
  - subprocess never inherits the user's project working directory;
  - subprocess env contains only the documented allowlist;
  - prompt text never appears in stderr or any log output.
- `crates/memoryd/tests/dream_cleanup.rs`
  - cleanup is idempotent across re-runs;
  - cleanup is commutative across two concurrent devices (simulated);
  - every operation produces the documented findings shape;
  - `observed_at` refresh is deterministic with respect to file mtime fixtures.
- `crates/memoryd/tests/dream_recall_integration.rs`
  - `dreams/questions/<scope>/<date>.md` content surfaces in Stream E `<pending-attention>` when entities intersect the recall seed set;
  - questions without entity hooks do not surface;
  - section caps (3 per scope, 8 total) are respected;
  - `safe_plaintext_fragment`-classified-unsafe questions are silently omitted.
- `crates/memoryd/tests/dream_cli.rs`
  - `memoryd dream status` reports CLI inventory, last runs, and active leases;
  - `memoryd dream now --scope ... --cli ...` runs end-to-end with `EchoCli`;
  - `memoryd dream review --since 7d` lists journal/question/candidate output.
- `docs/api/stream-f-dreaming-api.md`
  - documents MCP, daemon, and CLI surfaces with worked examples;
  - top-of-document privacy disclosure naming the harness-CLI delegation and pointing to each upstream's data policy.
- `docs/api/stream-a-public-api.md`
  - notes the new top-level paths (`substrate/`, `dreams/`, `leases/`) and their merge-driver semantics.
- `docs/api/stream-b-daemon-mcp-api.md`
  - notes the `memory_note` extension.
- `docs/api/stream-d-privacy-api.md`
  - notes Stream F as the first user of `MaskingSession`.
- `docs/api/stream-e-passive-recall-api.md`
  - notes the Pass-3 question hook (cross-link only; no v0.6 spec bump unless Trey approves).
- `README.md` and `CLAUDE.md`
  - note Stream F shipped only after the tests above pass.

## 13. Explicit deferrals

These are intentionally outside Stream F v0.1:

- **Generic `LlmProvider` trait with bring-your-own API keys.** If a future user wants to dream without an installed harness CLI, that's a v0.2+ feature. v0.1 is harness-CLI-only.
- **Tiebreak provider integration.** Stream C's `ContradictionTiebreaker` trait may later ride the same `HarnessCli` abstraction Stream F builds, but v0.1 does not wire it. The integration is one task in a follow-up stream.
- **Embedding inference.** Untouched; not in scope.
- **Privacy Filter (ONNX) integration.** Untouched; not in scope.
- **Streaming model output.** All passes are blocking call-and-collect. Streaming is a v0.2+ optimization for long Pass 1 runs.
- **Cross-device journal merging.** Each scope's journal is written by whichever device held that scope's lease that day. Devices on different days produce different files; there is no "merge two devices' Pass 1 narratives into one." If two devices race past lease and both write the same date's file, Stream A's three-way merge driver records a quarantine event and operator chooses; this is a diagnostic, not normal operation.
- **Pass 2 auto-promotion.** Every Pass-2 candidate goes to the candidate queue under `dreaming-strict`. Auto-promotion bypasses the human/governance gate and is explicitly out of scope.
- **Real-time / event-driven dreaming.** v0.1 is daily + on-demand-via-CLI only. Triggering a dream pass on every Nth substrate fragment, on entity-cluster size threshold, or on user idle is a v0.2+ feature.
- **Persistent counters.** `DreamStatusCounters` are in-process only, reset on daemon restart. Persistence is deferred to a later stream that owns operational telemetry across all streams (Stream G or H, TBD).
- **Dashboard UI for dream review.** `memoryd dream review` is CLI only. Visual dashboard is Stream G.
- **Live cross-device dream visibility.** Stream I owns event subscriptions; Stream F's dream activity is visible via `dreams/...` files in git history but not via a real-time push channel.
- **Doctor projection in `<pending-attention>`.** Stream E v0.5 §9.5 deferred this; Stream F does not unblock it. A future stream lands a daemon-cached doctor projection.

If an implementation needs one of these to pass the v0.1 acceptance tests, the spec should be revised before coding continues.

## 14. Privacy disclosure (user-facing)

Every documentation surface that mentions dreaming — `docs/api/stream-f-dreaming-api.md`, the README's Stream F section, `memoryd dream status` output, and the `memoryd dream enable` first-run confirmation — must include this disclosure verbatim or in close paraphrase:

> Dreaming uses whichever agent-harness CLI you have installed and authenticated on this device (Claude Code, Codex CLI, Gemini, etc.). Dream prompts are masked through the agent-memory privacy filter before they leave the daemon, but the masked text is processed by the harness CLI's upstream model provider. The data, retention, and training policies of that provider apply. If you don't want dream content sent to a particular provider, set the per-scope CLI priority to exclude it, or run `memoryd dream disable` on this device.

This is not buried. It is the first paragraph of the public API doc and the first line of `memoryd dream status` when dreaming is enabled.
