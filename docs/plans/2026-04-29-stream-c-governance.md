# Stream C Governance Implementation Plan

**Goal:** Build Stream C governance on top of the shipped Stream A substrate and Stream B daemon: policy-loaded memory writes, grounding verification, contradiction detection, tombstone matching, supersession chains, and a visible quarantine/review queue.

**Architecture:** Stream A remains the only layer that mutates canonical memory Markdown, event logs, and indexes. Stream C adds a `memory-governance` crate for deterministic policy/decision logic and wires `memoryd` handlers/MCP forwarding through that engine. Non-deterministic or provider-backed behavior, such as LLM contradiction tiebreaking, is behind traits with deterministic test doubles; Stream D privacy is not implemented here, but Stream C must remove Stream B's unsafe structured-write placeholders and fail closed when a write requires unavailable privacy classification.

**Tech Stack:** Rust 2021 workspace, `tokio`, `serde`/`serde_yaml`, `thiserror`, `memory-substrate` public API, `memoryd` Unix-socket JSON protocol, table-driven integration tests, deterministic fake providers for governance decisions.

---

## Plan Revision History

- **v0.1 / 2026-04-29:** Initial Stream C plan after Stream B completion. Grounded in `docs/specs/system-v0.1.md` §§11, 14, 19; `docs/specs/stream-a-core-substrate-v1.1.md`; existing `crates/memoryd` Stream B code; and the current workspace layout.

## Orchestrator Operating Model

The root orchestrator does not directly implement tasks except for integration conflict resolution and final gate coordination. Subagents do the substantive work in separate task branches/worktrees.

Every implementation, review, QA, performance, security, and docs subagent prompt must include this exact line:

> Mandatory skills: clean-code, tdd, rust-engineer.

Map `tdd` to the available `tdd-workflow` skill when invoking Codex skills. Rust workers must load `/Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md` and follow vertical TDD: failing behavior test, minimal implementation, green test, refactor while green.

### Orchestrator Responsibilities

1. Create/update the live task DAG with `update_plan` before spawning workers.
2. Spawn only bounded subagent tasks with non-overlapping owned files inside each parallel batch.
3. Never overwrite uncommitted user changes. Current known dirty paths at plan time: `.codex/skills/diagnose`, `.codex/skills/spec-quality-checklist`, `.codex/skills/tdd`, `.codex/skills/writing-plans`, `.codex/autoloop.project.json`, `fuzz/corpus/**`.
4. Integrate branches in dependency order; rerun narrow gates after each merge and full gates at the end.
5. Run code-review subagents after implementation batches, not as an optional closeout.
6. Keep Stream A substrate changes minimal and only where Stream C needs a public lifecycle API that cannot be implemented safely in `memoryd`.

### Required Subagent Prompt Preamble

Use this preamble for every build/review subagent, then add the task-specific section below:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Repository: /Users/treygoff/Code/agent-memory.
You are implementing Stream C governance. Stream A is a shipped substrate contract; do not casually rewrite it. Use vertical TDD: first add the failing behavior test named in your task, run it and record the failure, implement the smallest correct slice, rerun the narrow gate, then refactor only while green. Do not touch files outside your Owned files. Do not edit current dirty user-owned files under .codex/skills or fuzz/corpus.
```

## Stream C Boundaries And Non-goals

**In scope:**

- Versioned policy schema and policy loader for `policies/*.yaml`.
- Deterministic policy decision engine with typed outcomes.
- Grounding verification for user/agent/subagent/source refs that can be resolved locally.
- Contradiction detection pipeline: candidate normalization, top-K retrieval, tiebreak trait, duplicate/refinement/contradiction outcomes.
- Tombstone matching and refusal.
- Supersession chain writes that update both old and new records consistently.
- Quarantine/review queue backed by canonical memory statuses, not a hidden side DB.
- MCP/daemon support for `memory_write`, `memory_supersede`, and `memory_forget`.

**Out of scope:**

- Stream D privacy filter, age encryption, regex secret scanner, or masked synthesis.
- Stream E startup recall block assembly. `memory_startup` should remain not implemented unless a task explicitly proves it is just a review-queue/status stub.
- Stream G TUI/web UI. Stream C may add CLI JSON review commands only to make quarantines visible/testable.
- Real external LLM calls. Add provider traits and deterministic fakes; production provider wiring can land later.

**Fail-closed rule:** if a structured memory write requires privacy classification that Stream D has not supplied, the governance path must return a structured refusal rather than silently assuming `ClassificationOutcome::Trusted`.

## Parallelization Map

- **Batch 0:** Tasks 1-2 sequentially; they define the contract and shared crate skeleton.
- **Batch 1:** Tasks 3, 4, and 5 can run in parallel after Task 2; owned files do not overlap.
- **Batch 2:** Tasks 6 and 7 are sequential because supersession writes depend on contradiction outcome shape and may add substrate lifecycle API.
- **Batch 3:** Tasks 8 and 9 are sequential because both touch daemon protocol/MCP files.
- **Batch 4:** Tasks 10, 11, 12, and 13 review lanes run after implementation integration.

Before spawning any parallel batch, run this owned-file duplicate check against this plan:

```bash
rg '\*\*Owned files:\*\*' docs/plans/2026-04-29-stream-c-governance.md \
  | sed 's/.*\*\*Owned files:\*\* *//' \
  | tr ',' '\n' \
  | sed 's/`//g' \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | rg -v '^$' \
  | sort \
  | uniq -d
```

Expected for the full-plan check: duplicates are allowed only for sequential aggregator files (`crates/memory-governance/src/lib.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/protocol.rs`) and read-only review markers. Before spawning a parallel batch, manually apply the same check to that batch's task block only; Batch 1 and Batch 4 implementation/docs/test/review lanes must have no write-file overlap.

---

### Task 1: Lock The Stream C Contract

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Also use `spec-quality-checklist` if available.
**Parallel:** no
**Blocked by:** none
**Owned files:** `docs/specs/stream-c-governance-v0.1.md`, `docs/plans/2026-04-29-stream-c-governance.md`
**Invariants:** Do not modify Stream A spec files. Treat `docs/specs/system-v0.1.md` as the source of Stream C intent, but convert only the Stream C slice into implementation-grade requirements.
**Out of scope:** Do not design Stream D privacy internals or Stream E recall ranking.

**Files:**

- Create: `docs/specs/stream-c-governance-v0.1.md`
- Modify: `docs/plans/2026-04-29-stream-c-governance.md` only if the spec review finds a plan/spec mismatch

**Step 1: Write the contract doc**

Create a concise but implementation-grade Stream C spec with these sections:

1. Scope and dependency boundaries.
2. Policy schema v1 and built-in policy names: `me-strict`, `project-standard`, `agent-strict`, `dreaming-strict`.
3. Governance decision state machine: `Promoted`, `Candidate`, `Quarantined`, `Refused`, `Duplicate`, `Refinement`, `Superseded`, `Tombstoned`.
4. Grounding ref resolution rules for v0.1: user writes may be self-grounded; agent/subagent/tool/file writes require resolvable refs; dream prose is never a source.
5. Tombstone rule schema and matching canonicalization.
6. Contradiction detection stages and provider trait boundary.
7. Supersession chain write invariants.
8. Review queue visibility requirements.
9. Stream D/E/G non-goals.

**Step 2: Review the plan against the new contract**

Run:

```bash
rg -n "Stream C|Governance|memory_write|memory_supersede|memory_forget|tombstone|quarantine|policy|grounding|contradiction" docs/specs/system-v0.1.md docs/specs/stream-c-governance-v0.1.md docs/plans/2026-04-29-stream-c-governance.md
```

Expected: all six Stream C bullets from `docs/specs/system-v0.1.md` §19 are represented in the new spec and plan.

**Verification plan:**

- Primary command: `pnpm exec oxfmt --check docs/specs/stream-c-governance-v0.1.md docs/plans/2026-04-29-stream-c-governance.md`
- Secondary check: human/orchestrator read for Stream D/E/G boundary creep.

---

### Task 2: Add The `memory-governance` Crate Skeleton And Decision Types

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `Cargo.toml`, `Cargo.lock`, `crates/memory-governance/Cargo.toml`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/src/decision.rs`, `crates/memory-governance/src/error.rs`, `crates/memory-governance/tests/decision_contract.rs`
**Invariants:** The new crate must not mutate files directly. It returns typed decisions; callers perform substrate writes.
**Out of scope:** No policy loader, no daemon wiring, no LLM/provider code in this task.

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/memory-governance/Cargo.toml`
- Create: `crates/memory-governance/src/lib.rs`
- Create: `crates/memory-governance/src/decision.rs`
- Create: `crates/memory-governance/src/error.rs`
- Test: `crates/memory-governance/tests/decision_contract.rs`

**Step 1: Write the failing test**

`crates/memory-governance/tests/decision_contract.rs` should assert:

- `GovernanceDecision::refused("grounding", ...)` serializes to snake_case JSON with stable reason codes.
- `GovernanceDecision::promoted(id, namespace)` includes `policy_applied` and optional `supersedes`.
- `GovernanceError` implements `std::error::Error` and produces no `anyhow::Error` in public API types.

Run:

```bash
cargo test -p memory-governance decision_contract
```

Expected: fail because the crate does not exist.

**Step 2: Implement the skeleton**

Add a library crate with:

```rust
pub mod decision;
pub mod error;

pub use decision::{GovernanceDecision, GovernanceRefusalReason, GovernanceStatus, NextAction};
pub use error::{GovernanceError, GovernanceResult};
```

Use `thiserror` for typed errors. Public DTOs should derive `Debug`, `Clone`, `PartialEq`, `Serialize`, `Deserialize` where appropriate.

**Step 3: Run the narrow gate**

```bash
cargo test -p memory-governance decision_contract
cargo fmt --all -- --check
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memory-governance decision_contract`
- Secondary checks: `cargo clippy -p memory-governance --all-targets --all-features -- -D warnings`

---

### Task 3: Policy Loader, Built-in Policies, And Policy Dry-run

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 1
**Blocked by:** Task 2
**Owned files:** `crates/memory-governance/src/policy.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/policy_contract.rs`, `crates/memory-governance/tests/fixtures/policies/me-strict.yaml`, `crates/memory-governance/tests/fixtures/policies/project-standard.yaml`, `crates/memory-governance/tests/fixtures/policies/agent-strict.yaml`, `crates/memory-governance/tests/fixtures/policies/dreaming-strict.yaml`
**Invariants:** Missing or invalid policy files fail closed. Built-in defaults are available for tests and bootstrap, but production decisions must record whether a policy came from disk or built-in fallback.
**Out of scope:** Do not modify `crates/memory-substrate/src/tree/layout.rs` in this task.

**Files:**

- Create: `crates/memory-governance/src/policy.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Test: `crates/memory-governance/tests/policy_contract.rs`
- Create fixtures under `crates/memory-governance/tests/fixtures/policies/`

**Step 1: Write failing policy tests**

Tests must cover:

1. All four built-in policy fixture files parse.
2. Unknown YAML keys are rejected.
3. `confidence_floor` must be `0.0..=1.0`.
4. Policy names and versions round-trip into `policy_applied` strings such as `agent-strict@v3`.
5. `policy_for_scope(Scope::Agent)` resolves to `agent-strict`.

Run:

```bash
cargo test -p memory-governance policy_contract
```

Expected: fail before `policy.rs` exists.

**Step 2: Implement loader and schema**

Implement:

- `PolicySet::load_from_dir(path: &Path) -> GovernanceResult<Self>`
- `PolicySet::builtin() -> Self`
- `PolicySet::policy_for_candidate(&CandidateContext) -> GovernanceResult<&Policy>`
- `Policy::policy_applied() -> String`

Use `#[serde(deny_unknown_fields)]` on schema structs.

**Step 3: Add dry-run decision helper**

Add `Policy::dry_run(&CandidateContext) -> PolicyPreview` that reports:

- selected policy;
- confidence floor pass/fail;
- review gate triggers;
- grounding requirement;
- tombstone enforcement mode.

**Step 4: Run narrow gate**

```bash
cargo test -p memory-governance policy_contract
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memory-governance policy_contract`
- Secondary checks: `cargo fmt --all -- --check`

---

### Task 4: Grounding Verification

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 1
**Blocked by:** Task 2
**Owned files:** `crates/memory-governance/src/grounding.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/grounding_contract.rs`, `crates/memory-governance/tests/fixtures/grounding/live-source.md`
**Invariants:** Non-user writes without resolvable grounding are refused. Dream journal prose is explicitly rejected as a grounding source.
**Out of scope:** No network URL fetching. No Privacy Filter.

**Files:**

- Create: `crates/memory-governance/src/grounding.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Test: `crates/memory-governance/tests/grounding_contract.rs`
- Create: `crates/memory-governance/tests/fixtures/grounding/live-source.md`

**Step 1: Write failing tests**

Cover:

- `SourceKind::User` can pass with explicit user context.
- `SourceKind::AgentPrimary` with `source.ref = "file:<absolute path>#L1-L3"` passes only when the file exists.
- Missing file refs fail with `GovernanceRefusalReason::Grounding`.
- `source.ref` under `dreams/journal/` fails even when file exists.
- Subagent refs require a session-spawn registry trait entry.

Run:

```bash
cargo test -p memory-governance grounding_contract
```

Expected: fail before module exists.

**Step 2: Implement resolver traits**

Implement:

- `GroundingVerifier`
- `GroundingContext`
- `SourceRefResolver` trait
- `FileSourceResolver`
- `SessionSpawnResolver` trait with test fake

Keep all return types typed; no stringly failure parsing.

**Step 3: Run narrow gate**

```bash
cargo test -p memory-governance grounding_contract
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memory-governance grounding_contract`
- Secondary check: `rg -n "unwrap\(|expect\(" crates/memory-governance/src/grounding.rs` and justify or remove any occurrences.

---

### Task 5: Tombstone Rule Matching

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 1
**Blocked by:** Task 2
**Owned files:** `crates/memory-governance/src/tombstone.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/tombstone_contract.rs`, `crates/memory-governance/tests/fixtures/tombstones/2026-04-29.jsonl`
**Invariants:** Tombstone matching must be deterministic and independent of vector/LLM providers. Tombstones refuse writes; they do not silently quarantine.
**Out of scope:** Do not implement history rewrite or secret leak runbook from Stream D.

**Files:**

- Create: `crates/memory-governance/src/tombstone.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Test: `crates/memory-governance/tests/tombstone_contract.rs`
- Create: `crates/memory-governance/tests/fixtures/tombstones/2026-04-29.jsonl`

**Step 1: Write failing tests**

Cover:

- Canonical claim hash ignores case and whitespace.
- Entity set order does not change the tombstone hash.
- A matching tombstone returns `GovernanceDecision::Refused { reason: Tombstone }` with `tombstone_ref` details.
- Malformed tombstone JSONL returns a typed load error and fails closed.

Run:

```bash
cargo test -p memory-governance tombstone_contract
```

Expected: fail before module exists.

**Step 2: Implement rule parsing and matching**

Define a v0.1 tombstone rule schema:

```rust
pub struct TombstoneRule {
    pub id: String,
    pub target_memory_id: Option<MemoryId>,
    pub content_hash: String,
    pub entity_hash: String,
    pub reason: TombstoneKind,
    pub reason_text: Option<String>,
    pub active: bool,
}
```

Implement `TombstoneIndex::load_jsonl_dir` and `TombstoneIndex::match_candidate`.

**Step 3: Run narrow gate**

```bash
cargo test -p memory-governance tombstone_contract
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memory-governance tombstone_contract`
- Secondary check: add property-style tests for canonicalization if the module becomes non-trivial.

---

### Task 6: Contradiction Detection Pipeline

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Tasks 3, 4, 5
**Owned files:** `crates/memory-governance/src/contradiction.rs`, `crates/memory-governance/src/engine.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/contradiction_contract.rs`, `crates/memory-governance/tests/engine_contract.rs`
**Invariants:** No production network calls. Provider-dependent classification is behind a trait. Duplicate/refinement/contradiction outcomes must be testable without nondeterminism.
**Out of scope:** No real LLM provider, no Stream E final ranking formula.

**Files:**

- Create: `crates/memory-governance/src/contradiction.rs`
- Create: `crates/memory-governance/src/engine.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Test: `crates/memory-governance/tests/contradiction_contract.rs`
- Test: `crates/memory-governance/tests/engine_contract.rs`

**Step 1: Write failing tests**

Tests must prove:

1. A candidate with same canonical claim hash as an active memory returns `Duplicate(existing_id)` without invoking the tiebreak provider.
2. A candidate above similarity threshold invokes `ContradictionTiebreaker` with candidate + top-K hits.
3. Fake tiebreaker result `Refinement` maps to a decision that asks caller to merge evidence rather than create a second active memory.
4. Fake tiebreaker result `Contradiction` maps to supersession or quarantine according to policy.
5. Below-threshold candidates proceed to policy promotion/candidate decision.

Run:

```bash
cargo test -p memory-governance contradiction_contract engine_contract
```

Expected: fail.

**Step 2: Implement provider traits**

Implement:

- `CandidateMemory`
- `ExistingMemorySummary`
- `SimilaritySearch` trait
- `ContradictionTiebreaker` trait
- `TiebreakOutcome::{Same, Refinement, Contradiction, Unclear}`
- `GovernanceEngine::evaluate_write`

The engine order must be:

1. Policy selection/validation.
2. Grounding verification.
3. Tombstone matching.
4. Duplicate/refinement/contradiction detection.
5. Final status decision: promoted/candidate/quarantined/refused.

**Step 3: Run narrow gate**

```bash
cargo test -p memory-governance contradiction_contract engine_contract
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memory-governance contradiction_contract engine_contract`
- Secondary checks: `cargo test -p memory-governance`.

---

### Task 7: Supersession Chain Lifecycle API

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 6
**Owned files:** `crates/memory-substrate/src/api.rs`, `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/events/mod.rs`, `crates/memory-substrate/tests/supersession_lifecycle.rs`, `crates/memory-governance/src/supersession.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/supersession_contract.rs`
**Invariants:** Supersession must leave the old memory `Superseded`, set `superseded_by`, cap validity metadata if available, create/write the new memory with `supersedes`, and keep the graph valid under `validate_tree`.
**Out of scope:** Do not rewrite the merge driver.

**Files:**

- Modify: `crates/memory-substrate/src/model.rs`
- Modify: `crates/memory-substrate/src/api.rs`
- Modify: `crates/memory-substrate/src/events/mod.rs` only if a new event kind is required
- Test: `crates/memory-substrate/tests/supersession_lifecycle.rs`
- Create: `crates/memory-governance/src/supersession.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Test: `crates/memory-governance/tests/supersession_contract.rs`

**Step 1: Write failing substrate test**

`crates/memory-substrate/tests/supersession_lifecycle.rs` should:

1. Initialize substrate.
2. Write an active old memory.
3. Call a new `Substrate::supersede_memory(SupersedeRequest)`.
4. Assert the old memory is `MemoryStatus::Superseded` and points to the new id.
5. Assert the new memory points back via `supersedes`.
6. Assert `validate_tree(..., FullySynced)` accepts the graph.
7. Assert an event was appended for the lifecycle change.

Run:

```bash
cargo test -p memory-substrate supersession_lifecycle
```

Expected: fail.

**Step 2: Implement minimal substrate lifecycle API**

Add public types:

- `SupersedeRequest { old_id, replacement: Memory, reason, classification, allow_best_effort_durability }`
- `SupersedeOutcome { old_id, new_id, old_outcome, new_outcome }`

Implementation note: if fully atomic two-file writes are not available, the API must explicitly order writes so the replacement is written first as `Candidate`/`Quarantined` when necessary, then the old memory is marked `Superseded`; on failure after one committed write, return a `WriteFailure` with committed outcome so the daemon stops accepting further lifecycle writes until repair is visible. Do not hide partial lifecycle state.

**Step 3: Write failing governance wrapper test**

`crates/memory-governance/tests/supersession_contract.rs` should prove contradiction decisions produce a typed `SupersessionPlan` that the daemon can execute.

**Step 4: Implement `supersession.rs` planner**

The governance crate should not write files; it should build a plan containing old id, new memory frontmatter mutations, reason, and expected status transitions.

**Step 5: Run narrow gates**

```bash
cargo test -p memory-substrate supersession_lifecycle
cargo test -p memory-governance supersession_contract
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memory-substrate supersession_lifecycle && cargo test -p memory-governance supersession_contract`
- Secondary checks: `cargo test -p memory-substrate tree_validation` to guard supersession graph invariants.

---

### Task 8: Quarantine And Review Queue

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 7
**Owned files:** `crates/memory-governance/src/review.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/review_queue_contract.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/tests/review_queue.rs`
**Invariants:** The review queue is derived from canonical memory statuses/frontmatter, not a hidden queue that can drift. Admin review commands remain CLI/daemon protocol only; they do not leak into MCP tools.
**Out of scope:** No TUI/web dashboard.

**Files:**

- Create: `crates/memory-governance/src/review.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Test: `crates/memory-governance/tests/review_queue_contract.rs`
- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/protocol.rs`
- Modify: `crates/memoryd/src/handlers.rs`
- Test: `crates/memoryd/tests/review_queue.rs`

**Step 1: Write failing governance review tests**

Cover:

- Quarantined, candidate requiring confirmation, and pending-review memories appear in queue.
- Active, pinned, superseded, archived, tombstoned memories do not appear.
- Queue items include id, summary, status, policy_applied, reason, and next actions.

Run:

```bash
cargo test -p memory-governance review_queue_contract
```

Expected: fail.

**Step 2: Implement review queue projector**

Implement `ReviewQueue::from_memory_envelopes` in governance crate.

**Step 3: Write failing daemon tests**

`crates/memoryd/tests/review_queue.rs` should initialize substrate with one quarantined memory and assert:

- `RequestPayload::ReviewQueue` returns it.
- `memoryd review --help` is available.
- MCP manifest still excludes admin review tools.

Run:

```bash
cargo test -p memoryd review_queue
```

Expected: fail.

**Step 4: Wire CLI/protocol/handler**

Add protocol variants:

- `ReviewQueue { limit: Option<usize> }`
- `ReviewApprove { id: String }`
- `ReviewReject { id: String, reason: String }`

Implement queue list first. Approve/reject may be minimal but must use substrate writes and preserve event/durability semantics.

**Step 5: Run narrow gates**

```bash
cargo test -p memory-governance review_queue_contract
cargo test -p memoryd review_queue mcp_manifest
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memoryd review_queue mcp_manifest`
- Secondary check: `cargo test -p memory-governance review_queue_contract`

---

### Task 9: Wire `memory_write`, `memory_supersede`, And `memory_forget` Through Governance

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 8
**Owned files:** `crates/memoryd/src/mcp.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/tests/mcp_governance_forward.rs`, `crates/memoryd/tests/governance_e2e.rs`, `crates/memoryd/tests/mcp_manifest.rs`, `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd/Cargo.toml`
**Invariants:** `memory_note` remains cheap substrate observation. `memory_write`, `memory_supersede`, and `memory_forget` are governed and must no longer return Stream B `not_implemented` stubs. `memory_startup` remains not implemented for Stream E.
**Out of scope:** No startup recall block assembly.

**Files:**

- Modify: `crates/memoryd/Cargo.toml`
- Modify: `crates/memoryd/src/mcp.rs`
- Modify: `crates/memoryd/src/protocol.rs`
- Modify: `crates/memoryd/src/handlers.rs`
- Modify: `crates/memoryd/src/cli.rs`
- Test: `crates/memoryd/tests/mcp_governance_forward.rs`
- Test: `crates/memoryd/tests/governance_e2e.rs`
- Modify tests as needed: `crates/memoryd/tests/mcp_manifest.rs`, `crates/memoryd/tests/protocol_contract.rs`

**Step 1: Write failing MCP forwarding tests**

Assert:

- `ToolRequest::MemoryWrite` forwards to a governed daemon payload and no longer short-circuits as `not_implemented`.
- `ToolRequest::MemorySupersede` forwards to a governed daemon payload.
- `ToolRequest::MemoryForget` forwards to a governed daemon payload.
- `ToolRequest::MemoryStartup` still returns `not_implemented` and names Stream E.

Run:

```bash
cargo test -p memoryd mcp_governance_forward
```

Expected: fail because Stream B short-circuits write/supersede/forget.

**Step 2: Extend daemon protocol DTOs**

Add request/response variants with bounded response shapes:

- `WriteMemory { body, title, tags, meta } -> GovernanceWriteResponse`
- `Supersede { old_id, content, reason, meta } -> GovernanceSupersedeResponse`
- `Forget { id, reason } -> GovernanceForgetResponse`

Responses should mirror system spec §14.1 statuses: promoted/candidate/quarantined/refused/tombstoned.

**Step 3: Write failing e2e tests**

`crates/memoryd/tests/governance_e2e.rs` should prove:

1. A grounded project write can become active or candidate per policy.
2. An ungrounded agent write is refused with `reason = grounding`.
3. A duplicate write returns existing id rather than creating a second active memory.
4. Supersede updates both old and new memory frontmatter.
5. Forget calls substrate tombstone path and removes the term from FTS hits.
6. Quarantined writes show in `ReviewQueue`.

Run:

```bash
cargo test -p memoryd governance_e2e
```

Expected: fail until handler wiring exists.

**Step 4: Wire handlers through `memory-governance`**

- Add `memory-governance` dependency to `crates/memoryd/Cargo.toml`.
- Build `CandidateMemory` from MCP/CLI input.
- Use `PolicySet::load_from_dir(repo/policies).or_else_builtin_with_warning()` only if the Stream C spec allows fallback; record source in response.
- Use `Substrate::query_chunks` through a `SimilaritySearch` adapter.
- Execute decisions with Stream A APIs: write candidate/active memory, supersede lifecycle, tombstone.
- Return structured refusal for missing Stream D privacy classification if the write explicitly asks for sensitive/confidential handling that cannot be classified locally.

**Step 5: Run narrow gates**

```bash
cargo test -p memoryd mcp_governance_forward governance_e2e protocol_contract mcp_manifest
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Expected: pass.

**Verification plan:**

- Primary command: `cargo test -p memoryd mcp_governance_forward governance_e2e`
- Secondary checks: `cargo test -p memoryd` and `cargo test -p memory-governance`.

---

### Task 10: Documentation And Operator Runbook

**Subagent type:** `docs_researcher`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 4
**Blocked by:** Task 9
**Owned files:** `docs/api/stream-c-governance-api.md`, `docs/runbooks/governance-review.md`, `README.md`, `CLAUDE.md`
**Invariants:** Docs must describe actual implemented commands and response shapes, not aspirational future UI.
**Out of scope:** No marketing copy, no Stream D/E/G docs beyond boundaries.

**Files:**

- Create: `docs/api/stream-c-governance-api.md`
- Create: `docs/runbooks/governance-review.md`
- Modify: `README.md`
- Modify: `CLAUDE.md`

**Step 1: Write docs from implementation**

Document:

- `memory_write`, `memory_supersede`, `memory_forget` request/response examples.
- Refusal reasons and retryability.
- Review queue CLI usage.
- Policy file locations and dry-run behavior.
- Why `memory_startup` is still Stream E.

**Step 2: Run docs checks**

```bash
pnpm exec oxfmt --check docs/api/stream-c-governance-api.md docs/runbooks/governance-review.md README.md CLAUDE.md
rg -n "not yet implemented; planned for Stream C|Stream B is shipped|Streams B–I have not started" README.md CLAUDE.md crates/memoryd/src docs || true
```

Expected: no stale Stream C-not-implemented claims except intentionally historical docs.

**Verification plan:**

- Primary command: `pnpm exec oxfmt --check docs/api/stream-c-governance-api.md docs/runbooks/governance-review.md README.md CLAUDE.md`
- Secondary check: stale-claim `rg` above.

---

### Task 11: Governance Test Hardening

**Subagent type:** `test_hardener`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 4
**Blocked by:** Task 9
**Owned files:** `crates/memory-governance/tests/governance_matrix.rs`, `crates/memoryd/tests/governance_matrix_e2e.rs`, `crates/memory-test-support/src/governance.rs`, `crates/memory-test-support/src/lib.rs`
**Invariants:** Tests must assert behavior through public APIs or test-support helpers, not private implementation internals.
**Out of scope:** No full Stream H eval harness yet.

**Files:**

- Create: `crates/memory-governance/tests/governance_matrix.rs`
- Create: `crates/memoryd/tests/governance_matrix_e2e.rs`
- Modify/Create: `crates/memory-test-support/src/governance.rs`
- Modify: `crates/memory-test-support/src/lib.rs`

**Step 1: Add a matrix test helper**

Create table-driven fixtures for:

- User write, grounded agent write, ungrounded agent write, subagent write.
- Duplicate, refinement, contradiction, tombstone hit.
- Scope policies: me/project/agent/dreaming.

**Step 2: Run failing matrix tests**

```bash
cargo test -p memory-governance governance_matrix
cargo test -p memoryd governance_matrix_e2e
```

Expected: expose at least one missing edge from implementation; if all pass immediately, the subagent must explain why coverage is not duplicative.

**Step 3: Fix only test-support gaps**

This task should not change production implementation except for small compile-only trait visibility issues. Any behavior bug found here is reported to the orchestrator and routed back to the owning implementation task.

**Verification plan:**

- Primary command: `cargo test -p memory-governance governance_matrix && cargo test -p memoryd governance_matrix_e2e`
- Secondary checks: `cargo test --workspace --all-targets --all-features`

---

### Task 12: Security And Poisoning Review Subagent

**Subagent type:** `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 4
**Blocked by:** Task 9
**Owned files:** none; read-only review
**Invariants:** Review must be adversarial and line-cited. Do not patch directly.
**Out of scope:** Do not review Stream A broadly except where Stream C changed it.

**Files:**

- Read-only: `crates/memory-governance/**`, `crates/memoryd/src/{handlers,mcp,protocol,cli}.rs`, Stream C tests/docs
- Output: `docs/reviews/stream-c-security-review.md`

**Review prompt:**

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Perform an adversarial security/poisoning review of Stream C governance changes. Focus on durable memory poisoning, fail-open policy fallback, grounding bypass, tombstone bypass, hidden promotion of subagent writes, unsafe privacy assumptions before Stream D, malformed protocol inputs, and MCP admin-tool leakage. Produce severity-ranked findings with exact path:line citations and concrete fixes. Write the report to docs/reviews/stream-c-security-review.md. Do not edit production code.
```

**Verification plan:**

- Primary command after report: `test -s docs/reviews/stream-c-security-review.md`
- Orchestrator action: route every P0/P1/P2 finding to the owning implementation subagent before final gates.

---

### Task 13: Correctness, Performance, And API Review Subagents

**Subagent types:** `reviewer`, `performance_engineer`, `atlasos_assistant_contract_checker` fallback to `backend_arch` if no Stream C-specific contract checker is available
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Batch 4
**Blocked by:** Task 9
**Owned files:** none; read-only review
**Invariants:** Reviews must be independent. Do not let one review substitute for another.
**Out of scope:** No broad style bikeshedding.

**Files:**

- Read-only: all Stream C production/test/doc changes
- Output: `docs/reviews/stream-c-correctness-review.md`, `docs/reviews/stream-c-performance-review.md`, `docs/reviews/stream-c-api-contract-review.md`

**Correctness review prompt:**

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Review Stream C for lifecycle correctness. Focus on policy ordering, duplicate/refinement/contradiction semantics, supersession graph consistency, tombstone refusal, review queue derivation, partial-write repair semantics, and whether tests prove each invariant. Produce exact path:line findings. Write docs/reviews/stream-c-correctness-review.md.
```

**Performance review prompt:**

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Review Stream C for latency and resource risks. Focus on policy load caching, repeated full-tree scans, query_chunks/top-K costs, JSONL tombstone parsing, lock contention around Substrate/index, and daemon request timeouts. Produce exact path:line findings with suggested benchmarks. Write docs/reviews/stream-c-performance-review.md.
```

**API contract review prompt:**

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Review Stream C daemon/MCP protocol compatibility. Focus on stable snake_case JSON, bounded response bodies, retryable flags, MCP manifest excluding admin tools, CLI/MCP separation, and docs matching implemented DTOs. Produce exact path:line findings. Write docs/reviews/stream-c-api-contract-review.md.
```

**Verification plan:**

- Primary command after reports:

```bash
test -s docs/reviews/stream-c-correctness-review.md
test -s docs/reviews/stream-c-performance-review.md
test -s docs/reviews/stream-c-api-contract-review.md
```

- Orchestrator action: patch every accepted blocker, then rerun the review-specific narrow gates.

---

## Final Integration Gate

Run after all implementation tasks, docs, hardening, and review fixes:

```bash
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
./scripts/rust-boundary-check.sh
pnpm exec oxfmt --check .
pnpm exec oxlint .
bash scripts/check.sh
git diff --check
```

Expected: all pass. If `bash scripts/check.sh` includes longer fuzz/perf gates, run it only after narrow task gates are green.

## Stream C Acceptance Criteria

Stream C is done only when all of these are true:

1. `memory_write` is governed and no longer a Stream B `not_implemented` stub.
2. `memory_supersede` updates a bidirectional supersession chain and validates under Stream A tree validation.
3. `memory_forget` tombstones through Stream A and prevents tombstoned content from resurfacing in FTS results.
4. Ungrounded non-user writes are refused with structured `grounding` errors.
5. Tombstone hits refuse writes deterministically.
6. Duplicate/refinement/contradiction decisions are deterministic under fake providers and trait-backed for future real providers.
7. Quarantined/pending-review items are visible via CLI/daemon protocol and remain excluded from MCP admin surfaces.
8. Stream D privacy gaps fail closed; no new path silently assumes `ClassificationOutcome::Trusted` for structured durable writes.
9. Code-review subagents have produced security, correctness, performance, and API-contract reports, and accepted blockers are fixed.
10. The final integration gate is green.

## Suggested Commit Sequence

Do not commit unless Trey explicitly asks during execution. If asked, commit in these logical units:

1. `spec: lock stream c governance contract`
2. `feat: add governance policy and decision engine`
3. `feat: add grounding tombstone and contradiction checks`
4. `feat: add supersession lifecycle writes`
5. `feat: wire governed memory write supersede forget`
6. `test: harden stream c governance matrix`
7. `docs: document stream c governance operations`
8. `fix: address stream c review findings`
