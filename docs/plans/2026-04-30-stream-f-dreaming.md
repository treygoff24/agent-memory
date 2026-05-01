# Stream F Dreaming Implementation Plan

**Goal:** Build Stream F dreaming from `docs/specs/stream-f-dreaming-v0.2.md`: `memory_observe`, git-synced substrate fragments, harness-CLI dream passes, lease-elected daily runs, Pass 2 candidate promotion with grounding rehydration, cleanup, Stream E dream-question surfacing, CLI/admin docs, and full release verification.

**Architecture:** The main Codex CLI agent is the orchestrator. Subagents do all substantive implementation, test, docs, security, performance, and review work in bounded file scopes; the orchestrator integrates, runs gates, and dispatches review/fix loops. Stream A remains the canonical repository/index substrate, Stream B remains the daemon/MCP bridge, Stream C remains governance/review authority, Stream D remains privacy/masking/encryption authority, and Stream E remains recall-block assembly. Stream F adds a `memoryd::dream` module plus additive substrate/config/tree/protocol/recall surfaces without creating a second canonical memory layer.

**Tech Stack:** Rust 2021 workspace, `tokio`, `serde`/`serde_json`, `chrono`, `ulid`, `thiserror`, `tempfile`, Stream A `memory-substrate`, Stream C `memory-governance`, Stream D `memory-privacy`, Stream E `memoryd::recall`, Unix-socket daemon protocol, MCP forwarder, git transport, vertical TDD, and release-gate bench fixtures.

---

## Source Contract

Normative sources:

- `docs/specs/stream-f-dreaming-v0.2.md`
- `docs/specs/system-v0.1.md` §11-12
- `docs/reviews/stream-f-codex-spec-review.md` only as lineage already incorporated into v0.2
- shipped Stream A-E code and docs in this repo

Do not edit or overwrite spec files unless Trey explicitly asks. This plan creates `docs/plans/2026-04-30-stream-f-dreaming.md` and implementation work should treat `docs/specs/stream-f-dreaming-v0.2.md` as the active contract.

## Codex CLI Orchestrator Contract

The main GPT agent running in Codex CLI is the orchestrator. The orchestrator may:

1. Maintain the task DAG and current status.
2. Spawn subagents for every implementation/review/docs/perf/security lane.
3. Enforce non-overlapping owned-file scopes for parallel batches.
4. Integrate subagent changes in dependency order.
5. Resolve integration conflicts caused by accepted subagent outputs.
6. Run narrow and full gates.
7. Spawn fix subagents for every blocking review finding.

The orchestrator must not casually implement feature code directly. If a gate fails or a review finding appears, create a bounded fix task and assign it to the correct subagent with the mandatory skills below. Tiny mechanical plan/doc integration edits are acceptable for the orchestrator; feature code is not.

## Mandatory Skills For Every Subagent

Every implementation, test, docs, review, security, performance, and QA subagent prompt must include this exact line:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
```

Interpret Trey's "Rust Engineering SQL" request as the repo-local Rust Engineering **skill** requirement. Every subagent must load the repo-local Rust skill:

```text
Load /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md.
```

Review subagents must also explicitly load `clean-code` and apply it as a review lens. Implementation subagents must use vertical TDD: one failing behavior test, narrow RED command, minimal implementation, narrow GREEN command, refactor only while green.

### Required Subagent Prompt Preamble

Use this preamble for every subagent:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Load /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md.
Repository: /Users/treygoff/Code/agent-memory.
You are working on Stream F dreaming from docs/specs/stream-f-dreaming-v0.2.md. Treat Stream A as the only canonical substrate/index, Stream B as daemon/MCP, Stream C as governance/review, Stream D as privacy/masking/encryption, and Stream E as recall assembly. Use vertical TDD: write one failing behavior test, run it and record the RED failure, implement the smallest correct slice, rerun the narrow gate to GREEN, then refactor only while green. Do not touch files outside your Owned files. Do not edit spec files unless the task explicitly owns a docs amendment.
```

For review subagents append:

```text
This is a review-only lane unless explicitly assigned a fix task. Lead with findings ordered by severity. Apply clean-code review criteria plus Rust correctness, async safety, privacy, test quality, and spec compliance. If there are no findings, say so and list residual risks.
```

## Parallelization And Review Cadence

Parallel work is allowed only when owned files do not overlap inside the batch. The orchestrator must run a batch-specific owned-file duplicate check before spawning any parallel implementation batch.

Full-plan owned-file duplicates are expected because sequential tasks touch shared choke points such as `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/main.rs`, and docs. Duplicates are forbidden only inside a parallel batch.

Batch duplicate check template:

```bash
cat > /tmp/stream-f-batch-owned-files.txt <<'LIST'
Task X: path/to/file.rs
Task Y: path/to/other.rs
LIST
cut -d: -f2- /tmp/stream-f-batch-owned-files.txt \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | sort \
  | uniq -d
```

Expected for each parallel batch: no output.

### Review gates

- **Review Gate A - Contract/surface review:** after Tasks 1-3A. Clean-code + API-contract reviewers inspect substrate/config/tree/merge seams before daemon/public-surface work starts.
- **Review Gate B - Observe/security review:** after Tasks 5-7. Clean-code + security reviewers inspect MCP/daemon observe write path, privacy routing, and canonical isolation.
- **Review Gate C - Harness/pipeline review:** after Tasks 8-11. Clean-code + correctness/security reviewers inspect harness subprocess safety, masking, Pass 1/2/3 validation, and candidate write-back.
- **Review Gate D - Cleanup/recall/perf review:** after Tasks 12-14. Clean-code + performance reviewers inspect cleanup idempotence and Stream E hot-path overhead.
- **Final Review Gate E:** after Tasks 15-16. Independent clean-code, security, performance, API contract, and docs reviewers run before final gates.

Every review gate must produce a file in `docs/reviews/` or a concise orchestrator-captured report. All severity-1/2 findings must be fixed by scoped fix subagents, the same review lane must rerun, and severity-3 findings must either be fixed or logged with rationale before advancing.

---

## Task 1: Contract Map And Worktree Baseline

**Subagent type:** `backend_arch`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Also use `spec-quality-checklist`.  
**Parallel:** no  
**Blocked by:** none  
**Owned files:** `docs/reviews/stream-f-contract-map.md`, `docs/plans/2026-04-30-stream-f-dreaming.md`  
**Invariants:** Do not edit `docs/specs/stream-f-dreaming-v0.2.md`. Do not weaken acceptance signals.  
**Out of scope:** Production code.

**Files:**

- Create: `docs/reviews/stream-f-contract-map.md`
- Modify: `docs/plans/2026-04-30-stream-f-dreaming.md` only if this plan contradicts v0.2

**Steps:**

1. Write `docs/reviews/stream-f-contract-map.md` mapping every v0.2 acceptance bullet to an implementation task, owned files, and narrow gate.
   - Explicitly record the Stream F v0.2 wording erratum: `Substrate::read_memory_envelope(&MemoryId)` cannot accept dream/substrate/lease paths; the executable API contract is `Substrate::read_path_envelope(&RepoPath) -> ReadError::NotACanonicalMemory` for noncanonical Stream F paths. Do not create a redundant reader to satisfy impossible wording.
2. Capture current dirty-tree baseline with:
   ```bash
   git status --short
   ```
3. Verify the v0.2 spec terms are covered:
   ```bash
   rg -n "memory_observe|DreamNow|DreamStatus|HarnessCli|PassOutcome|grounding_rehydration|pending_attention|dreams/cleanup|lease_dirty_tree" docs/specs/stream-f-dreaming-v0.2.md
   ```
4. Check current code surfaces and record choke points:
   ```bash
   rg -n "RequestPayload|ResponsePayload|StatusResponse|ToolName|WriteNote|Startup|RecallStatusCounters|query_recall_index|validate_tree|EventKind" crates
   ```

**Verification plan:**

- Primary: human-readable contract map covers all Stream F §12 acceptance bullets.
- Secondary: `rg -n "TBD|TODO|unclear|not covered" docs/reviews/stream-f-contract-map.md` returns no unresolved blockers except explicit implementation tasks.

---

## Task 2: Stream A Tree Layout, Canonical Isolation, And Dream Validators

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Task 1  
**Owned files:** `crates/memory-substrate/src/tree/layout.rs`, `crates/memory-substrate/src/tree/validate.rs`, `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/error.rs`, `crates/memory-substrate/src/api.rs`, `crates/memory-substrate/tests/dream_canonical_isolation.rs`, `crates/memoryd/tests/dream_canonical_isolation.rs`, `docs/api/stream-a-public-api.md`  
**Invariants:** Dream files are valid Stream A repo files but never canonical memories. Existing canonical memory validation/indexing behavior must not regress.  
**Out of scope:** `memory_observe` daemon handling, harness CLI, dream pipeline.

**Files:**

- Modify: `crates/memory-substrate/src/tree/layout.rs`
- Modify: `crates/memory-substrate/src/tree/validate.rs`
- Modify: `crates/memory-substrate/src/model.rs`
- Modify: `crates/memory-substrate/src/error.rs`
- Modify: `crates/memory-substrate/src/api.rs`
- Test: `crates/memory-substrate/tests/dream_canonical_isolation.rs`
- Acceptance Test: `crates/memoryd/tests/dream_canonical_isolation.rs`
- Docs: `docs/api/stream-a-public-api.md`

**Step 1: RED test**

Create `crates/memory-substrate/tests/dream_canonical_isolation.rs` covering substrate-level behavior and `crates/memoryd/tests/dream_canonical_isolation.rs` as the spec-named acceptance wrapper covering the same behavior through daemon-visible APIs:

- frontmatter-free `dreams/journal/me/2026-04-30.md` validates;
- `dreams/questions/project/proj_abc/2026-04-30.jsonl` validates only with valid JSONL `{entities, question}` records;
- `dreams/cleanup/dev_local/2026-04-30.json` validates only as a JSON object;
- `substrate/dev_local/2026-04-30.jsonl`, `encrypted/substrate/dev_local/2026-04-30.jsonl`, and `leases/journal.lease` validate as non-memory files;
- malformed dream JSONL fails tree validation with a typed validation error;
- existing `Substrate::read_path_envelope(&RepoPath)` returns a newly added `ReadError::NotACanonicalMemory { path }` variant for dream/substrate/lease paths. Task 1 records the spec's impossible `read_memory_envelope(&MemoryId)` wording as an erratum; implementation must not add a redundant path reader;
- `query_memory`, `query_recall_index`, and `query_chunks` never return dream/substrate/lease files.

Run:

```bash
cargo test -p memory-substrate --test dream_canonical_isolation
cargo test -p memoryd --test dream_canonical_isolation
```

Expected: FAIL because dream validators and `NotACanonicalMemory` are not implemented.

**Step 2: GREEN implementation**

- Extend repo bootstrap directories for `dreams/cleanup` and encrypted substrate if missing.
- Add helper predicates such as `is_noncanonical_stream_f_path` and dedicated validators in `tree::validate`.
- Ensure canonical Markdown walker ignores `dreams/journal/**.md` before frontmatter parsing.
- Add `ReadError::NotACanonicalMemory { path: RepoPath }` and route noncanonical Stream F paths to it before frontmatter parsing.
- Keep indexing functions restricted to canonical memory paths.

**Step 3: GREEN command**

```bash
cargo test -p memory-substrate --test dream_canonical_isolation
cargo test -p memoryd --test dream_canonical_isolation
cargo test -p memory-substrate --test tree_validation
```

**Verification plan:**

- Primary: `cargo test -p memory-substrate --test dream_canonical_isolation && cargo test -p memoryd --test dream_canonical_isolation`
- Secondary: `cargo test -p memory-substrate --test tree_validation --test api_write_read --test memory_query_extension`

---

## Task 3: Stream A Config, Events, Substrate Fragment Model, And Append/Archive Primitives

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Task 2  
**Owned files:** `crates/memory-substrate/src/config/mod.rs`, `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/events/log.rs`, `crates/memory-substrate/src/api.rs`, `crates/memory-substrate/tests/config_loading.rs`, `crates/memory-substrate/tests/event_kind_schema.rs`, `crates/memory-substrate/tests/dream_substrate_primitives.rs`  
**Invariants:** Existing config without `dreams:` keeps loading with defaults. No silent fallback for active embeddings. Substrate appends must be atomic and contained under repo root.  
**Out of scope:** Daemon `memory_observe` handler and privacy routing.

**Files:**

- Modify: `crates/memory-substrate/src/config/mod.rs`
- Modify: `crates/memory-substrate/src/model.rs`
- Modify: `crates/memory-substrate/src/events/log.rs`
- Modify: `crates/memory-substrate/src/api.rs`
- Test: `crates/memory-substrate/tests/config_loading.rs`
- Test: `crates/memory-substrate/tests/event_kind_schema.rs`
- Test: `crates/memory-substrate/tests/dream_substrate_primitives.rs`

**Step 1: RED config tests**

Add tests that:

- load default `DreamsConfig` and `EventsConfig` when omitted;
- reject unknown CLI names, bad scope override keys, and out-of-range numeric values;
- reject `pending_attention_per_scope_cap > pending_attention_total_cap`;
- parse all v0.2 config keys and preserve values;

Run:

```bash
cargo test -p memory-substrate --test config_loading dreams_config
```

Expected: FAIL.

**Step 2: RED substrate primitive tests**

Create `dream_substrate_primitives.rs` covering:

- append plaintext substrate fragment under `substrate/<device>/<date>.jsonl`;
- append encrypted substrate record under `encrypted/substrate/<device>/<date>.jsonl` with no `text` field;
- append emits `EventKind::SubstrateFragmentWritten`;
- archival moves expired plaintext fragments into `substrate/archive/<device>/<YYYY-MM>.jsonl` idempotently;
- archive output is concat + sort by `id`.

Run:

```bash
cargo test -p memory-substrate --test dream_substrate_primitives
```

Expected: FAIL.

**Step 3: GREEN implementation**

- Add `DreamsConfig`, `EventsConfig`, `ObserveKind`, `SubstrateFragmentRecord`, `EncryptedSubstrateFragmentRecord`, descriptor DTOs, and validation helpers.
- Add `EventKind::SubstrateFragmentWritten { id, path, classification }` or the minimum shape required by v0.2.
- Add `Substrate::append_substrate_fragment` and `Substrate::archive_expired_substrate_fragments` or equivalent public APIs for memoryd to consume.
- Use existing atomic write/append containment patterns; do not introduce ad hoc writes outside substrate APIs.

**Step 4: GREEN commands**

```bash
cargo test -p memory-substrate --test config_loading
cargo test -p memory-substrate --test event_kind_schema
cargo test -p memory-substrate --test dream_substrate_primitives
```

**Verification plan:**

- Primary: the three commands above.
- Secondary: `cargo test -p memory-substrate --test atomic_write --test event_log_identity --test event_log_recovery`

---

## Task 3A: Stream F Merge-Driver Semantics

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Task 3  
**Owned files:** `crates/memory-substrate/src/merge/mod.rs`, `crates/memory-substrate/src/merge/three_way.rs`, `crates/memory-substrate/src/merge/field_rules.rs`, `crates/memory-substrate/tests/dream_merge_rules.rs`, `crates/memory-substrate/tests/merge_rules.rs`, `docs/api/stream-a-public-api.md`  
**Invariants:** Existing canonical Markdown merge behavior must not regress. Stream F noncanonical files follow the v0.2 merge rules exactly.  
**Out of scope:** Daemon lease/pipeline behavior.

**Files:**

- Modify: `crates/memory-substrate/src/merge/mod.rs`
- Modify: `crates/memory-substrate/src/merge/three_way.rs`
- Modify: `crates/memory-substrate/src/merge/field_rules.rs` only if path routing belongs there
- Test: `crates/memory-substrate/tests/dream_merge_rules.rs`
- Regression Test: `crates/memory-substrate/tests/merge_rules.rs`
- Docs: `docs/api/stream-a-public-api.md`

**Step 1: RED tests**

Create `crates/memory-substrate/tests/dream_merge_rules.rs` covering all Stream F v0.2 merge bullets:

- substrate JSONL files merge by concat + sort by `id`;
- dream question JSONL and `leases/journal.lease` merge by concat + sort by `(scope, ts, id)` with deterministic handling when `id` is absent from legacy/lease rows;
- dream journal Markdown files use last-writer-wins by `(scope_path, date, device)` and emit a diagnostic/quarantine marker on contested same-date same-scope writes;
- cleanup JSON files use last-writer-wins by `(device_id, date)`.

Run:

```bash
cargo test -p memory-substrate --test dream_merge_rules
```

Expected: FAIL because Stream F merge routing is not implemented.

**Step 2: GREEN implementation**

- Route Stream F path families before canonical Markdown merge logic.
- Keep JSONL merge code small and shared, with explicit sort keys per file family.
- Preserve canonical memory merge tests.

**Step 3: GREEN commands**

```bash
cargo test -p memory-substrate --test dream_merge_rules
cargo test -p memory-substrate --test merge_rules
```

**Verification plan:**

- Primary: `cargo test -p memory-substrate --test dream_merge_rules`
- Secondary: `cargo test -p memory-substrate --test merge_rules`

---

## Review Gate A: Contract, Stream A Surface, And Clean-Code Review

**Subagent types:** `reviewer`, `backend_arch`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.  
**Parallel:** yes  
**Blocked by:** Tasks 1-3A integrated and green  
**Owned files:** `docs/reviews/stream-f-review-gate-a-clean-code.md`, `docs/reviews/stream-f-review-gate-a-contract.md`  
**Invariants:** Review only. Do not edit production code.  
**Out of scope:** Harness/pipeline work.

**Review lanes:**

1. **Clean-code/Rust review:** inspect Tasks 2-3A diffs for naming, module boundaries, error types, no ad hoc IO, no overbroad functions, no unnecessary cloning.
2. **Contract review:** verify every Stream A v0.2 invariant is represented in tests, especially canonical isolation, config validation, and merge semantics.

**Commands reviewers should run:**

```bash
cargo test -p memory-substrate --test dream_canonical_isolation
cargo test -p memoryd --test dream_canonical_isolation
cargo test -p memory-substrate --test dream_substrate_primitives
cargo test -p memory-substrate --test config_loading
cargo test -p memory-substrate --test dream_merge_rules
cargo fmt --all -- --check
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 4.

---

## Task 4: Dream Domain Module Skeleton, Scope Encoding, Prompt Templates, And DTOs

**Subagent type:** `backend_arch`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Review Gate A  
**Owned files:** `crates/memoryd/src/lib.rs`, `crates/memoryd/src/dream/mod.rs`, `crates/memoryd/src/dream/types.rs`, `crates/memoryd/src/dream/config.rs`, `crates/memoryd/src/dream/scope.rs`, `crates/memoryd/src/dream/prompts.rs`, `prompts/dream-pass-1-v1.md`, `prompts/dream-pass-2-v1.md`, `prompts/dream-pass-3-v1.md`, `crates/memoryd/tests/dream_scope_and_prompts.rs`  
**Invariants:** Keep public protocol DTOs in `protocol.rs`; keep dream internals private behind `memoryd::dream`. Prompt templates are embedded with `include_str!` or an equivalent compile-time embedding mechanism; dream runs must not depend on the daemon's current working directory to find templates. No harness subprocesses yet.  
**Out of scope:** MCP, CLI, actual pass execution.

**Steps:**

1. RED test `dream_scope_and_prompts.rs` for:
   - `me` -> `dreams/journal/me/<date>.md` and `dreams/questions/me/<date>.jsonl`;
   - `agent` -> `agent/<date>`;
   - `project:proj_abc` -> `project/proj_abc/<date>`;
   - `org:org_abc` -> `org/org_abc/<date>`;
   - invalid scope rejects with `invalid_request`;
   - prompt rendering is byte-identical for stable inputs across two render calls with the same substrate fragments, active memories, masking seed, and harness selection;
   - Pass 2 includes the evidence catalog and Pass 1/3 do not;
   - prompt template loading works from a temp current directory that does not contain `prompts/`, proving templates are embedded or resolved independent of process cwd.
2. Implement `dream::{types, config, scope, prompts}` and add prompt template files.
3. GREEN:
   ```bash
   cargo test -p memoryd --test dream_scope_and_prompts
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_scope_and_prompts`
- Secondary: `cargo test -p memoryd --lib dream::scope dream::prompts` if unit tests are added.

---

## Task 5: Protocol DTOs, Status Counters, And Client Compatibility

**Subagent type:** `backend_arch`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 2 with Task 6 after Task 4  
**Blocked by:** Task 4  
**Owned files:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/client.rs`, `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd/tests/handler_contract.rs`  
**Invariants:** Additive serde changes only. Old status clients tolerate new fields via defaults. Existing Stream B-E protocol tests stay green.  
**Out of scope:** Handler behavior beyond returning typed not-implemented for new requests if handler task has not landed.

**Steps:**

1. RED tests for serde roundtrips:
   - `RequestPayload::Observe`;
   - `RequestPayload::DreamNow`;
   - `RequestPayload::DreamStatus`;
   - `ResponsePayload::Observe`;
   - `DreamRunReport`, `PassOutcome`, `CandidateWriteResult`, `DreamStatusReport`, `ScopeRunSummary`, `HarnessCliStatus`, `PromptTransport`, `LeaseRecord`;
   - `StatusResponse` with additive `dreams: DreamStatusCounters` defaults when field absent.
2. Implement protocol DTOs and default counters.
3. GREEN:
   ```bash
   cargo test -p memoryd --test protocol_contract --test handler_contract
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test protocol_contract`
- Secondary: `cargo test -p memoryd --test server_smoke --test daemon_e2e`

---

## Task 6: MCP `memory_observe` Tool And Manifest Contract

**Subagent type:** `worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 2 with Task 5 after Task 4  
**Blocked by:** Task 4  
**Owned files:** `crates/memoryd/src/mcp.rs`, `crates/memoryd/tests/mcp_manifest.rs`, `crates/memoryd/tests/mcp_forward.rs`, `docs/api/stream-b-daemon-mcp-api.md`  
**Invariants:** `memory_note` remains unchanged and still only accepts `{ text }`. Dream CLI/admin commands are not MCP tools.  
**Out of scope:** Observe storage handler implementation.

**Steps:**

1. RED tests:
   - manifest has nine tools and includes `memory_observe`;
   - `memory_observe` schema validates `text`, `kind`, `entities` shape;
   - `ToolName::try_from("memory_observe")` works;
   - `memory_note` rejects `kind` as unknown/extra if current schema enforces that, or at least does not forward it;
   - `memory_observe` forwards to `RequestPayload::Observe`.
2. Implement MCP DTOs/manifest/forwarder.
3. GREEN:
   ```bash
   cargo test -p memoryd --test mcp_manifest --test mcp_forward
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test mcp_manifest --test mcp_forward`
- Secondary: `rg -n "memory_observe|memory_note" docs/api/stream-b-daemon-mcp-api.md crates/memoryd/src/mcp.rs`

---

## Task 7: `memory_observe` Handler, Privacy Routing, And Substrate Write Path

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Tasks 5-6  
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/dream_substrate_fragments.rs`, `crates/memoryd/tests/privacy_e2e.rs`  
**Invariants:** Stream D classifier runs before any disk effect. Secret/high-risk writes refuse with no fragment. PII writes encrypted substrate shape. `memory_note` behavior unchanged.  
**Out of scope:** Dream pass execution.

**Steps:**

1. RED tests in `dream_substrate_fragments.rs`:
   - `memory_observe` for `Observation|Pattern|Signal` appends plaintext fragment;
   - PII content routes to `encrypted/substrate/...` and includes descriptor/encryption shape;
   - secret content returns `privacy_error` or exact current refusal code mapped to `WriteFailureKind::SecretRefused`;
   - `memory_note { text }` still writes canonical memory only;
   - entity count and text length validation reject invalid input.
2. Implement `RequestPayload::Observe` branch in handlers using Stream D privacy APIs and Stream A append primitives.
3. Update handler error mapping for `invalid_request` and `privacy_error`.
4. GREEN:
   ```bash
   cargo test -p memoryd --test dream_substrate_fragments
   cargo test -p memoryd --test privacy_e2e
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_substrate_fragments`
- Secondary: `cargo test -p memoryd --test mcp_governance_forward --test mcp_forward --test privacy_e2e`

---

## Review Gate B: Observe, Privacy, MCP, And Clean-Code Review

**Subagent types:** `reviewer`, `security_auditor`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.  
**Parallel:** yes  
**Blocked by:** Tasks 5-7 integrated and green  
**Owned files:** `docs/reviews/stream-f-observe-clean-code-review.md`, `docs/reviews/stream-f-observe-security-review.md`  
**Invariants:** Review only.  
**Out of scope:** Pipeline review.

**Review focus:**

- No `memory_note(kind=...)` regression.
- `memory_observe` tool is agent-facing but `memoryd dream ...` is CLI/admin only.
- No disk write before privacy classification.
- Encrypted substrate shape leaks no plaintext.
- Handler functions are small, named, and testable.

**Commands:**

```bash
cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 8.

---

## Task 8: Harness CLI Registry, Echo/Claude/Codex Adapters, And Subprocess Hardening

**Subagent type:** `security_auditor` for implementation, because subprocess privacy is security-critical  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 3 with Task 9 after Review Gate B  
**Blocked by:** Review Gate B  
**Owned files:** `crates/memoryd/src/dream/harness.rs`, `crates/memoryd/src/dream/registry.rs`, `crates/memoryd/src/dream/error.rs`, `crates/memoryd/tests/dream_harness_cli.rs`, `crates/memoryd/Cargo.toml`  
**Invariants:** Prompt via stdin for v0.2 adapters. No prompt in argv/stderr/logs. Minimal env. Clean working directory. Timeout terminates child.  
**Out of scope:** Pass orchestration.

**Steps:**

1. RED tests in `dream_harness_cli.rs`:
   - `EchoCli` deterministic canned outputs;
   - stub `claude` PATH detection;
   - adapter argv for Claude/Codex contains no prompt;
   - prompt bytes appear only on stdin in test recorder;
   - env allowlist contains only documented keys;
   - cwd is scratch dir, not repo/project;
   - timeout sends SIGTERM then SIGKILL;
   - no v0.2 adapter declares `PromptTransport::Argv`.
2. Implement `HarnessCli`, `PromptTransport`, `HarnessCliError`, registry, `EchoCli`, `ClaudeCodeCli`, `CodexCli`. Leave `GeminiCli` behind a clearly disabled status if stdin support cannot be proven in test.
3. GREEN:
   ```bash
   cargo test -p memoryd --test dream_harness_cli
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_harness_cli`
- Secondary: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

## Task 9: Lease Election, Scheduled Retry Semantics, And Daemon Git Commits

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 3 with Task 8 after Review Gate B  
**Blocked by:** Review Gate B  
**Owned files:** `crates/memoryd/src/dream/lease.rs`, `crates/memoryd/src/dream/git.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/tests/dream_lease_election.rs`, `crates/memoryd/tests/dream_lease_scheduled_retry.rs`, `crates/memory-substrate/src/git/commit.rs`  
**Invariants:** Lease commits stage only `leases/journal.lease`. Manual runs fail fast. Scheduled runs retry only `lease_unavailable`, not `lease_held`. Dirty user work must not be co-committed.  
**Out of scope:** Pass execution after lease acquisition.

**Steps:**

1. RED tests for manual lease:
   - acquire succeeds;
   - active foreign lease returns `lease_held` exit-code semantics;
   - push race retries 3 times;
   - `--force` overrides;
   - dirty tree outside lease file returns `lease_dirty_tree`;
   - author/message format exactly matches v0.2;
   - the spec-named `dream_lease_election.rs` test invokes `memoryd dream now` through the CLI binary and asserts exit code 5 for `lease_held` and `lease_unavailable`.
2. RED tests for scheduled retry:
   - transient fetch failure eventually succeeds within window;
   - persistent failure records missed-run summary;
   - success next day resets consecutive missed runs;
   - retry window `0` disables scheduled retries.
3. Implement lease/git helper module. Prefer wrapping existing git helpers; do not duplicate shell logic in many places.
4. GREEN:
   ```bash
   cargo test -p memoryd --test dream_lease_election --test dream_lease_scheduled_retry
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_lease_election --test dream_lease_scheduled_retry`
- Secondary: `cargo test -p memory-substrate --test git_preflight --test git_adoption`

---

## Task 10: Dream Pass Pipeline, Masking, Evidence Catalog, And Candidate Queue Writes

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Tasks 8-9  
**Owned files:** `crates/memoryd/src/dream/run.rs`, `crates/memoryd/src/dream/pass1.rs`, `crates/memoryd/src/dream/pass2.rs`, `crates/memoryd/src/dream/pass3.rs`, `crates/memoryd/src/dream/evidence.rs`, `crates/memoryd/src/dream/masking.rs`, `crates/memoryd/src/dream/mod.rs`, `crates/memoryd/tests/dream_pass_pipeline.rs`  
**Invariants:** One `MaskingSession` per scope run. Pass 1/3 outputs stay masked. Pass 2 restore is the only restoration. Evidence refs must exist in catalog. Pass 2 never auto-promotes.  
**Out of scope:** Grounding rehydration at later promotion; cleanup; CLI rendering.

**Steps:**

1. RED tracer 1: Pass 1 with `EchoCli` writes masked journal file.
2. GREEN minimal Pass 1 implementation.
3. RED tracer 2: Pass 2 receives evidence catalog and accepts valid refs into candidate queue under `dreaming-strict`.
4. GREEN minimal Pass 2 implementation and candidate write adapter.
5. RED tracer 3: hallucinated evidence refs are rejected before governance.
6. GREEN evidence validation.
7. RED tracer 4: Pass 2 malformed JSON retries once, then fails Pass 2 while Pass 3 still runs.
8. GREEN retry/failure semantics.
9. RED tracer 5: Pass 3 JSONL writes only records with non-empty valid entity ids and masked questions.
10. GREEN Pass 3 parser/validator.
11. RED tracer 6: Pass 3 records with hallucinated entity ids are discarded and increment `dream_question_omitted_total{reason: malformed_record}`.
12. GREEN hallucinated-entity rejection/counter behavior.
13. RED tracer 7: empty Pass 1 output aborts the run, writes no Pass 2 candidates, and still drops the `MaskingSession`.
14. GREEN empty-Pass-1 abort/drop behavior.
15. RED tracer 8: `MaskingSession::restore` restores Pass 2 fields and Drop runs on success/failure.
16. GREEN masking ownership/teardown.

**Commands after each tracer:**

```bash
cargo test -p memoryd --test dream_pass_pipeline <test_name> -- --nocapture
```

Final GREEN:

```bash
cargo test -p memoryd --test dream_pass_pipeline
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_pass_pipeline`
- Secondary: `cargo test -p memoryd --test governance_e2e --test review_queue --test startup_recall_privacy`

---

## Task 11: Grounding Rehydration Enforcement For Dream Candidates

**Subagent type:** `backend_arch` or `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Task 10  
**Owned files:** `crates/memory-substrate/src/error.rs`, `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/frontmatter/schema.rs`, `crates/memory-substrate/src/frontmatter/parse.rs`, `crates/memory-substrate/src/frontmatter/serialize.rs`, `crates/memory-substrate/tests/frontmatter_schema.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/src/policy.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/dream/rehydration.rs`, `crates/memoryd/tests/dream_grounding_rehydration.rs`, `crates/memory-governance/tests/policy_contract.rs`  
**Invariants:** Deterministic, no LLM. Candidate with failed rehydration quarantines with `grounding_rehydration_failed`. Existing non-dream governance behavior unchanged.  
**Out of scope:** Generic tiebreak provider integration.

**Steps:**

1. RED tests:
   - missing cited substrate ref at promote time quarantines;
   - aged-out substrate ref quarantines;
   - content drift above `dreams.pass_2_drift_threshold` quarantines;
   - cited memory now `tombstoned|superseded|archived` quarantines;
   - valid refs promote through existing review approval path;
   - a candidate citing `dreams/journal/...` or `dreams/questions/...` as `source.ref` is refused at write time with `WriteFailureKind::DreamProseAsSource`;
   - `grounding_rehydration_required: true` round-trips through frontmatter/schema parsing/serialization and is set by Pass 2 dream-authored candidates.
2. Add `WriteFailureKind::DreamProseAsSource` in Stream A's write-failure enum and route dream-prose source refs to it before candidate persistence.
3. Add the explicit `grounding_rehydration_required` frontmatter/model/schema support if it is still absent; do not assume the field exists just because v0.2 says Stream C accepts it.
4. Implement a small rehydration module invoked only for dream-authored candidates carrying the `grounding_rehydration_required` marker.
5. Keep policy YAML schema compatibility in mind: do not reintroduce unknown old design-only keys rejected by Stream C.
6. GREEN:
   ```bash
   cargo test -p memoryd --test dream_grounding_rehydration
   cargo test -p memory-governance --test policy_contract
   cargo test -p memory-substrate --test frontmatter_schema
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_grounding_rehydration`
- Secondary: `cargo test -p memoryd --test review_queue --test governance_matrix_e2e`

---

## Review Gate C: Harness, Pipeline, Masking, And Governance Review

**Subagent types:** `reviewer`, `security_auditor`, `test_hardener`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.  
**Parallel:** yes  
**Blocked by:** Tasks 8-11 integrated and green  
**Owned files:** `docs/reviews/stream-f-pipeline-clean-code-review.md`, `docs/reviews/stream-f-pipeline-security-review.md`, `docs/reviews/stream-f-pipeline-test-review.md`  
**Invariants:** Review only unless assigned a follow-up fix task.  
**Out of scope:** Cleanup/recall.

**Review focus:**

- Prompt transport, env/cwd isolation, timeout kill behavior.
- No prompt logging and no unmasked sensitive text to harness stdin.
- Evidence catalog validation cannot be bypassed.
- Pass 2 candidate results report refusals correctly.
- Functions/modules are small, names are clear, async/blocking boundaries are safe.
- Tests are behavior-first and not overcoupled to internals.

**Commands:**

```bash
cargo test -p memoryd --test dream_harness_cli --test dream_pass_pipeline --test dream_grounding_rehydration
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 12.

---

## Task 12: Cleanup Layer, Reports, Event Compaction, And Cleanup Git Commit Semantics

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 4 with Task 13 after Review Gate C  
**Blocked by:** Review Gate C  
**Owned files:** `Cargo.toml`, `crates/memoryd/Cargo.toml`, `crates/memoryd/src/dream/cleanup.rs`, `crates/memoryd/src/dream/report.rs`, `crates/memoryd/tests/dream_cleanup.rs`, `crates/memory-substrate/src/events/log.rs`, `crates/memory-substrate/src/events/mod.rs`  
**Invariants:** Cleanup is idempotent and commutative. Cleanup never deletes memory bodies. Dirty tree defers commit but writes report.  
**Out of scope:** Startup recall hook.

**Steps:**

1. RED tests for:
   - expired substrate archival idempotence;
   - stale candidate archival;
   - entity index rebuild no-op when projection matches;
   - lint/tombstone/supersession findings report, no auto-repair;
   - observed_at refresh deterministic with mtime fixture;
   - event compaction to `events/archive/<YYYY-MM>.jsonl.zst` using an explicit `zstd` workspace dependency;
   - cleanup report JSON shape;
   - cleanup-bot author/message;
   - dirty tree writes report and records `commit_deferred: true`;
   - two simulated devices running cleanup concurrently converge to the same archive/report state.
2. Add `zstd` to workspace dependencies and `memoryd` dependencies, then implement cleanup/report module with operations split into small functions.
3. GREEN:
   ```bash
   cargo test -p memoryd --test dream_cleanup
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_cleanup`
- Secondary: `cargo test -p memory-substrate --test event_log_recovery --test reindex_reconciliation`

---

## Task 13: Stream E Pass-3 `<pending-attention>` Hook And Omission Counters

**Subagent type:** `heavy_worker`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 4 with Task 12 after Review Gate C  
**Blocked by:** Review Gate C  
**Owned files:** `crates/memoryd/src/recall/counters.rs`, `crates/memoryd/src/recall/startup.rs`, `crates/memoryd/src/recall/types.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/src/recall/dream_questions.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/tests/dream_recall_integration.rs`, `crates/memoryd/tests/startup_recall_mcp.rs`, `docs/api/stream-e-passive-recall-api.md`  
**Invariants:** Startup hot path adds <=5ms p95 in bench. No LLM, no decryption, no Pass 3 rerun. Safe-fragment classifier runs before emission.  
**Out of scope:** Dream pass generation.

**Steps:**

1. RED tests in `dream_recall_integration.rs`:
   - matching entity ids surface in `<pending-attention>`;
   - empty `entities` records never surface;
   - records with non-empty `entities` that do not intersect the active recall seed set are omitted and increment `dream_question_omitted_total{reason: no_entity_match}`;
   - 2/scope and 6 total caps;
   - deterministic order: entity overlap -> recency -> novelty hash -> lex;
   - unsafe question omitted and counter increments;
   - malformed records omitted and counter increments;
   - cap omissions increment `cap_section` / `cap_total`.
   - startup recall output is byte-for-byte unchanged from the Stream E baseline when no dream-question files exist.
2. Implement `recall::dream_questions` parser/selector and add `dream_question_omitted_total` to counters.
3. Integrate into startup response assembly without changing `policy="stream-e-v0.5"`.
4. GREEN:
   ```bash
   cargo test -p memoryd --test dream_recall_integration --test startup_recall_mcp
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_recall_integration`
- Secondary: `cargo test -p memoryd --test startup_recall_determinism --test startup_recall_privacy --test recall_cli`

---

## Task 14: Dream CLI, Status, Review, Enable/Disable, And Main Wiring

**Subagent type:** `cli_developer`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** no  
**Blocked by:** Tasks 12-13  
**Owned files:** `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/dream/status.rs`, `crates/memoryd/src/dream/review.rs`, `crates/memoryd/tests/dream_cli.rs`, `crates/memoryd/tests/cli_contract.rs`  
**Invariants:** `memoryd dream ...` is CLI/admin only; no MCP forwarding. Human status output begins with privacy disclosure when enabled. JSON output is structured DTO. Exit codes match v0.2.  
**Out of scope:** Changing daemon serve lifecycle unless needed for DreamNow/DreamStatus dispatch.

**Steps:**

1. RED tests:
   - clap parses all dream subcommands/options;
   - status human first line contains privacy disclosure;
   - status JSON includes CLI inventory, leases, runs, counters;
   - dream now with `EchoCli` runs end-to-end;
   - review lists journal/question/candidate outputs;
   - enable/disable toggles `~/.memoryd/dream-disabled` or runtime-equivalent sentinel in tests;
   - `memoryd dream enable` first-run output includes the §14 privacy disclosure before enabling;
   - exit code 5 for manual lease failures.
2. Implement CLI args, main dispatch, daemon request handling for `DreamNow`/`DreamStatus`, and local sentinel helpers.
3. GREEN:
   ```bash
   cargo test -p memoryd --test dream_cli --test cli_contract
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dream_cli`
- Secondary: `cargo test -p memoryd --test daemon_e2e --test server_smoke`

---

## Review Gate D: Cleanup, Recall Hot Path, CLI, And Performance Review

**Subagent types:** `reviewer`, `performance_engineer`, `security_auditor`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.  
**Parallel:** yes  
**Blocked by:** Tasks 12-14 integrated and green  
**Owned files:** `docs/reviews/stream-f-cleanup-recall-clean-code-review.md`, `docs/reviews/stream-f-cleanup-recall-performance-review.md`, `docs/reviews/stream-f-cli-security-review.md`  
**Invariants:** Review only unless assigned a fix task.  
**Out of scope:** Docs finalization.

**Review focus:**

- Cleanup operations are idempotent/commutative and do not delete bodies.
- Recall hook performs bounded file reads only and increments omission counters accurately.
- CLI status disclosure is not buried.
- Runtime sentinel path is device-local, not repo-synced.
- Performance gates have real fixtures, not handwaving.

**Commands:**

```bash
cargo test -p memoryd --test dream_cleanup --test dream_recall_integration --test dream_cli
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 15.

---

## Task 15: Bench Fixture And Release Performance Evidence

**Subagent type:** `performance_engineer`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 5 with Task 16 after Review Gate D  
**Blocked by:** Review Gate D  
**Owned files:** `crates/memoryd/src/bin/stream_f_dream_bench.rs`, `bench/stream-f-dreaming-results.darwin-arm64.json`, `docs/reviews/stream-f-bench-evidence.md`, `crates/memoryd/Cargo.toml`  
**Invariants:** Bench fixture is deterministic. Assertion/smoke mode must not dirty the tree. Updating `bench/stream-f-dreaming-results.darwin-arm64.json` happens only through an explicit release/update mode and human-authored commit. Do not mask failing performance by raising thresholds.  
**Out of scope:** Product feature changes.

**Steps:**

1. Add bench binary covering:
   - 1k-fragment Pass 1 prompt assembly;
   - lease acquisition fixture;
   - substrate-fragment write throughput;
   - cleanup full-pass over representative fixture;
   - Stream E dream-question read overhead.
2. RED/green via an asserting command. The bench binary must exit nonzero if any v0.2 p95 budget fails and must not write `bench/stream-f-dreaming-results.darwin-arm64.json` in this mode:
   ```bash
   cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
   ```
3. Add a separate explicit update command for release evidence, not used by routine gates:
   ```bash
   cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --write-output bench/stream-f-dreaming-results.darwin-arm64.json
   ```
4. Write `docs/reviews/stream-f-bench-evidence.md` with each v0.2 budget, measured p95, pass/fail status, and residual risks.

**Verification plan:**

- Primary: `cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json`
- Secondary: `jq . bench/stream-f-dreaming-results.darwin-arm64.json`

---

## Task 16: API Docs, README, CLAUDE, And Privacy Disclosure

**Subagent type:** `worker` or `docs_researcher`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.  
**Parallel:** yes, Phase 5 with Task 15 after Review Gate D  
**Blocked by:** Review Gate D  
**Owned files:** `docs/api/stream-f-dreaming-api.md`, `docs/api/stream-a-public-api.md`, `docs/api/stream-b-daemon-mcp-api.md`, `docs/api/stream-d-privacy-api.md`, `docs/api/stream-e-passive-recall-api.md`, `README.md`, `CLAUDE.md`  
**Invariants:** Privacy disclosure appears at top of Stream F doc and first-line status behavior is documented. Docs must state `memory_note` unchanged and `memory_observe` new.  
**Out of scope:** Spec edits.

**Steps:**

1. Create `docs/api/stream-f-dreaming-api.md` with worked MCP, daemon, and CLI examples.
2. Create `docs/api/stream-b-daemon-mcp-api.md` if it is still missing; otherwise update it in place. This file is required by the v0.2 spec and must document `memory_observe` plus unchanged `memory_note`.
3. Update Stream A/D/E docs with minimal cross-links and exact new surfaces.
4. Add provider disclosure references for each shipped adapter: `ClaudeCodeCli`, `CodexCli`, and any adapter that remains disabled/deferred.
5. Update README/CLAUDE status once tests are green.
6. Verify docs contain required phrases:
   ```bash
   rg -n "memory_observe|prompt_transport|Dreaming uses whichever agent-harness CLI|substrate fragments written via memory_observe|dream-disabled|dreaming-strict|upstream data policy|ClaudeCodeCli|CodexCli|substrate fragments.*git-synced" docs/api README.md CLAUDE.md
   ```

**Verification plan:**

- Primary: `rg -n "memory_observe|prompt_transport|Dreaming uses whichever agent-harness CLI|substrate fragments written via memory_observe|dream-disabled|dreaming-strict|upstream data policy|ClaudeCodeCli|CodexCli|substrate fragments.*git-synced" docs/api README.md CLAUDE.md`
- Secondary: `git diff --check docs/api README.md CLAUDE.md`

---

## Final Review Gate E: Full Independent Review Swarm

**Subagent types:** `reviewer`, `security_auditor`, `performance_engineer`, `test_hardener`, `backend_arch`  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Every review subagent must load clean-code.  
**Parallel:** yes  
**Blocked by:** Tasks 15-16  
**Owned files:** `docs/reviews/stream-f-final-clean-code-review.md`, `docs/reviews/stream-f-final-security-review.md`, `docs/reviews/stream-f-final-performance-review.md`, `docs/reviews/stream-f-final-test-review.md`, `docs/reviews/stream-f-final-api-contract-review.md`  
**Invariants:** Review-only. Findings must cite files/tests/spec clauses.  
**Out of scope:** New feature requests beyond v0.2.

**Review lanes:**

1. **Clean-code/Rust maintainability:** module boundaries, naming, function size, error handling, async boundaries.
2. **Security/privacy:** prompt transport, env allowlist, masking/restore, encrypted substrate, no reveal, no logs.
3. **Performance:** bench fixture, recall hot-path overhead, cleanup complexity, subprocess blocking isolation.
4. **Test hardening:** acceptance matrix coverage, vertical TDD evidence, fixture determinism, negative paths.
5. **API contract:** protocol/MCP/CLI docs match shipped DTOs and serde defaults.

**Commands reviewers should run as relevant:**

```bash
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents; severity-3 findings either fixed or explicitly documented as non-blocking with Trey-facing rationale.

---

## Task 17: Final Release Gate And Handoff

**Subagent type:** Orchestrator-run final gate. Optional `heavy_worker` may draft the report from captured output only after the orchestrator runs commands directly.  
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer for any optional report-drafting subagent.  
**Parallel:** no  
**Blocked by:** Final Review Gate E and all fixes  
**Owned files:** `docs/reviews/stream-f-final-gate-report.md`  
**Invariants:** Do not declare done unless all required gates pass or a blocker is documented with exact command/output.  
**Out of scope:** Opportunistic refactors after final review.

**Steps:**

1. Run targeted Stream F acceptance suite:
   ```bash
   cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives
   cargo test -p memory-substrate --test dream_merge_rules
   cargo test -p memoryd --test dream_canonical_isolation
   cargo test -p memoryd --test dream_substrate_fragments --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_harness_cli --test dream_cleanup --test dream_recall_integration --test dream_cli
   cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
   ```
2. Run broader Rust gates:
   ```bash
   cargo test --workspace --all-targets --all-features
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   ```
3. Run repo boundary/docs gates:
   ```bash
   ./scripts/rust-boundary-check.sh
   pnpm exec oxfmt --check .
   pnpm exec oxlint .
   git diff --check
   ```
4. If this repo still uses the full release script successfully on the branch, run:
   ```bash
   BENCH_PROFILE=darwin-arm64 bash scripts/check.sh
   ```
5. Write `docs/reviews/stream-f-final-gate-report.md` with exact commands, pass/fail status, and any residual risks.

**Verification plan:**

- Primary: all commands above pass.
- Secondary: `git status --short` shows only intended Stream F changes.

---

## Execution DAG Summary

1. Task 1 contract map.
2. Task 2 Stream A canonical isolation.
3. Task 3 config/substrate primitives.
4. Task 3A Stream F merge rules.
5. Review Gate A, then fixes if needed.
6. Task 4 dream module skeleton.
7. Parallel Phase 2: Task 5 protocol + Task 6 MCP.
8. Task 7 observe handler/privacy write path.
9. Review Gate B, then fixes if needed.
10. Parallel Phase 3: Task 8 harness CLI + Task 9 leases.
11. Task 10 pass pipeline.
12. Task 11 grounding rehydration.
13. Review Gate C, then fixes if needed.
14. Parallel Phase 4: Task 12 cleanup + Task 13 recall hook.
15. Task 14 CLI/status/review/enable-disable.
16. Review Gate D, then fixes if needed.
17. Parallel Phase 5: Task 15 bench + Task 16 docs.
18. Final Review Gate E, then fixes if needed.
19. Task 17 orchestrator-run final release gate and handoff.

## Stop Conditions

Stop and ask Trey only if one of these occurs:

- v0.2 spec contradicts shipped Stream A-E code in a way that cannot be resolved additively.
- A required v0.2 behavior needs a new external dependency not already in the workspace and the dependency choice is not obvious.
- A harness CLI does not support stdin and would require an argv fallback adapter in v0.2, which the spec says needs Trey approval.
- Final gates expose unrelated pre-existing failures that cannot be isolated from Stream F changes.

Everything else should be handled by spawning scoped subagents, fixing findings, and rerunning gates.
