# Stream A Core Substrate Implementation Plan

**Goal:** Build Stream A from `docs/specs/stream-a-core-substrate-v1.0.md`: a Rust `memory-substrate` library, a standalone `memory-merge-driver` binary, and the fixture/fault/performance gates required before Streams B-I depend on it.

**Architecture:** Treat Markdown+YAML memory files as canonical, SQLite/FTS/vector data as derived, per-device JSONL as durable audit, and git as the sync transport. The repo becomes a Rust workspace with strict module seams: frontmatter/tree/config/ids first, then durable writes/events/index/git/merge, then public API hardening and end-to-end release gates.

**Tech Stack:** Rust workspace, `cargo`, `rustfmt`, Clippy, Dylint/Agent Linters, `rusqlite` with bundled SQLite, sqlite-vec adapter, FTS5, `notify`, git CLI wrapper, `yaml_serde`, `proptest`, `cargo-fuzz`, Criterion/custom perf harness, Oxfmt/Oxlint for docs/config/scripts, Specgate ownership/intent gates, GitHub Actions.

---

## Source inputs and live lookup notes

- Primary spec: `docs/specs/stream-a-core-substrate-v1.0.md`.
- Parent context read: `docs/specs/system-v0.1.md` and `docs/handoff-2026-04-23.md`.
- Current repo state before this plan: Stream A spec files are uncommitted user work; do not overwrite them. This plan adds only `docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md`.
- Latest local Agent Linters checkout was refreshed with `git pull --ff-only` in `/Users/treygoff/Code/agentlinters`; it fast-forwarded from `42004d2` to `91446bb` on `origin/master`.
- `ctx7` checked sqlite-vec docs under `/asg017/sqlite-vec`.
- Exa checked and fetched:
  - sqlite-vec Rust usage and API docs: `https://alexgarcia.xyz/sqlite-vec/rust.html`, `https://github.com/asg017/sqlite-vec/blob/main/site/api-reference.md`, `https://crates.io/crates/sqlite-vec`
  - Oxfmt/Oxlint docs: `https://oxc.rs/docs/guide/usage/formatter.html`, `https://oxc.rs/docs/guide/usage/formatter/cli.html`, `https://oxc.rs/docs/guide/usage/linter/ci`
  - YAML parser options: `https://crates.io/crates/yaml_serde`, `https://github.com/saphyr-rs/saphyr`
- Important external facts that affect implementation:
  - sqlite-vec is pre-v1; pin the exact crate version in `Cargo.lock`, generate adapter DDL from code, and make vector reconciliation mandatory.
  - sqlite-vec Rust integration exposes `sqlite3_vec_init`; register it through rusqlite's `sqlite3_auto_extension`, and pass float vectors as compact float32 bytes.
  - Oxfmt supports JS/TS/JSON/YAML/TOML/Markdown and is installed as `pnpm add -D oxfmt`; use `oxfmt` and `oxfmt --check`.
  - Oxlint is installed as `pnpm add -D oxlint`; use it for JS/TS scripts only, not Rust.
  - `yaml_serde` is the maintained YAML-org fork of `serde_yaml`; use it for serde-compatible YAML, while keeping an escape hatch to `saphyr-parser` if diagnostics/round-trip needs exceed serde values.
  - Local `specgate --help` currently describes "architectural intent for TypeScript projects"; use it for config/spec validation and ownership gates in this Rust repo, but do not claim it enforces Rust import graphs unless a Rust resolver lands.

## Non-negotiable execution rules

1. Codex root is the orchestrator. It owns sequencing, task-list updates, branch hygiene, conflict resolution, final review synthesis, and merge/push decisions.
2. Every worker/reviewer/QA subagent receives `/clean-code` and `/tdd` in the prompt. No exceptions.
3. Every implementation task uses vertical TDD:
   - write one failing behavior test;
   - run it and capture the expected failure;
   - implement the minimum code;
   - run the targeted test to green;
   - refactor only while green;
   - run the task gate.
4. No horizontal "write all tests first" pass. Large fixture matrices are added in thin slices, one behavior family at a time.
5. No fallbacks, no legacy compatibility bridge, no silent downgrade. If a platform/tool cannot satisfy the contract, return typed refusal and test it.
6. Public APIs must preserve the spec's committed outcome semantics. Any error after durable file commit must carry `WriteOutcome`.
7. SQLite remains derived. Do not add a hidden second index.
8. Sensitive/secret handling is write-refusal/routing logic. `secret` must never become persisted frontmatter.
9. Git operations use explicit arg vectors and repo-root validation; no shell string execution.
10. Every parallel subagent is constrained to owned files. Overlapping owned files require orchestrator serialization.

## Orchestrator/subagent operating model

### Root Codex orchestrator

**Skills:** `writing-plans`, `clean-code`, `tdd`, `debugging-systematic`, `receiving-code-review`, `spec-quality-checklist`, `exa-search`.

**Responsibilities:**

- Maintain the live todo list with `update_plan`.
- Keep the root branch clean and never overwrite Trey's uncommitted spec edits.
- Spawn subagents by the task map below.
- Pass each subagent a bounded prompt with:
  - exact task ID;
  - mandatory skills: `clean-code`, `tdd`;
  - additional task skills;
  - owned files;
  - blocked-by task IDs;
  - exact verification command(s);
  - "report tests run and files changed; do not touch files outside ownership."
- Use patient waits for substantial subagent work.
- After each worker task, spawn at least one reviewer/QA lane before accepting the diff.

### Custom subagent type map

| Work type | Subagent type | Mandatory skills passed | Additional skills passed | Notes |
| --- | --- | --- | --- | --- |
| Codebase mapping and ownership validation | `code_mapper` | `clean-code`, `tdd` | `spec-quality-checklist` | Maps existing files, confirms owned-file non-overlap, checks Specgate ownership model. |
| Dependency/API docs research | `docs_researcher` | `clean-code`, `tdd` | `exa-search`, `find-docs` | Uses `ctx7` first for library docs, Exa for high-signal source discovery. |
| Core Rust architecture | `backend_arch` | `clean-code`, `tdd` | `rust-engineer`, `debugging-systematic` | Designs public API and module seams before workers parallelize. |
| CLI and git/merge-driver work | `cli_developer` | `clean-code`, `tdd` | `rust-engineer`, `debugging-systematic` | Owns binary behavior, argv contracts, git CLI wrapper. |
| Heavy implementation slices | `heavy_worker` | `clean-code`, `tdd` | `rust-engineer`, `debugging-systematic` | Main worker for substrate modules. |
| Narrow fixture/config slices | `fast_worker` | `clean-code`, `tdd` | `rust-engineer` | Small, isolated test/fixture/config tasks. |
| Test hardening/fault injection | `test_hardener` | `clean-code`, `tdd` | `debugging-systematic`, `rust-engineer` | Crash matrix, property tests, fixture coverage, flaky-test cleanup. |
| Security/privacy review | `security_auditor` | `clean-code`, `tdd` | `harden`, `debugging-systematic` | Secret refusal, no plaintext leakage, git/path safety. |
| Performance gates | `performance_engineer` | `clean-code`, `tdd` | `performance`, `optimize`, `rust-engineer` | 10K corpus, p95 gates, benchmark JSON. |
| General code review | `reviewer` | `clean-code`, `tdd` | `receiving-code-review`, `caveman-review` | Findings only, grounded in files/lines/tests. |
| Final pre-merge gate | `review_guard` | `clean-code`, `tdd` | `receiving-code-review`, `spec-quality-checklist` | Reviews regression risk, gate completeness, release criteria. |
| Docs/runbook updates | `docs_editor` | `clean-code`, `tdd` | `doc`, `write-human` | Public API docs, operator repair docs, CI docs. |
| Plan validation | `plan_checker` | `clean-code`, `tdd` | `spec-quality-checklist` | Runs before implementation starts and again before parallel execution. |

### Standard subagent prompt prefix

```text
You are executing Stream A Task <ID>.
Mandatory skills: clean-code, tdd.
Additional skills: <task-specific list>.
Follow vertical TDD only: one failing behavior test, minimal implementation, green test, refactor.
Owned files only: <paths>.
Do not touch other files without asking the Codex orchestrator.
Run the exact verification commands below and report command output summaries.
If a spec/tooling conflict appears, stop and report the concrete conflict with file/path evidence.
```

## Repository shape to create

```text
.
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── .cargo/config.toml
├── .dylint/custom_lints/...
├── package.json
├── pnpm-lock.yaml
├── .oxfmtrc.json
├── .oxlintrc.json
├── specgate.config.yml
├── modules/
│   ├── stream-a-frontmatter.spec.yml
│   ├── stream-a-tree-config-ids.spec.yml
│   ├── stream-a-io-events.spec.yml
│   ├── stream-a-index-vector.spec.yml
│   ├── stream-a-git-merge.spec.yml
│   └── stream-a-tests-quality.spec.yml
├── crates/
│   ├── memory-substrate/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── api.rs
│   │   │   ├── error.rs
│   │   │   ├── model.rs
│   │   │   ├── tree/
│   │   │   ├── config/
│   │   │   ├── ids/
│   │   │   ├── frontmatter/
│   │   │   ├── markdown/
│   │   │   ├── index/
│   │   │   ├── watcher/
│   │   │   ├── events/
│   │   │   ├── git/
│   │   │   ├── merge/
│   │   │   └── runtime/
│   │   ├── tests/
│   │   └── benches/
│   ├── memory-merge-driver/
│   │   ├── Cargo.toml
│   │   ├── src/main.rs
│   │   └── tests/
│   └── memory-test-support/
│       ├── Cargo.toml
│       └── src/
├── fixtures/
│   ├── frontmatter/
│   ├── tree/
│   ├── merge/
│   ├── git/
│   ├── index/
│   └── perf/
├── fuzz/
│   ├── Cargo.toml
│   └── fuzz_targets/
├── scripts/
│   ├── check.sh
│   ├── install-agentlinters.sh
│   ├── rust-boundary-check.sh
│   ├── two-clone-convergence.sh
│   └── bench-gate.sh
└── .github/workflows/
    ├── stream-a-ci.yml
    ├── stream-a-fuzz.yml
    └── stream-a-perf.yml
```

## Quality gate stack

### Per-task narrow gate

Each task defines a targeted command. The default shape is:

```bash
cargo test -p memory-substrate <task_filter> -- --nocapture
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Full local gate

Create `scripts/check.sh` and make it the canonical local gate:

During Task 1, this script is allowed to exist before every referenced downstream script is implemented; Task 1 verification does not run `scripts/check.sh`. Starting in Task 11, `scripts/check.sh` must be fully executable and green.

```bash
#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
cargo test --workspace
cargo test --workspace --release
pnpm exec oxfmt --check .
pnpm exec oxlint .
specgate validate
specgate check --output-mode deterministic
specgate doctor ownership --project-root . --format json
./scripts/rust-boundary-check.sh
./scripts/two-clone-convergence.sh
./scripts/bench-gate.sh --tier smoke
```

### Release gate

Before declaring Stream A done:

```bash
cargo test --workspace --release
cargo fuzz run merge_driver -- -max_total_time=600
./scripts/two-clone-convergence.sh --full
./scripts/bench-gate.sh --tier release --output bench/results.json
./scripts/durability-probe-gate.sh --matrix apfs,tmpfs,ext4,einval,best-effort --output bench/durability-results.json
specgate validate
specgate check --output-mode deterministic
specgate doctor ownership --project-root . --format json
./scripts/check.sh
```

Run the release gate on both required hardware profiles:

- Apple Silicon laptop, 16 GB+.
- Linux x86_64 CI runner, >=4 vCPU, NVMe-class storage.

## Agent Linters / Oxfmt / Specgate setup

### Agent Linters

Use the refreshed local checkout as source of truth:

```bash
git -C /Users/treygoff/Code/agentlinters rev-parse --short HEAD
# expected in this planning run: 91446bb
```

Create `scripts/install-agentlinters.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

AGENTLINTERS_ROOT="${AGENTLINTERS_ROOT:-/Users/treygoff/Code/agentlinters}"
test -d "$AGENTLINTERS_ROOT/.git"

cp "$AGENTLINTERS_ROOT/assets/rust/rustfmt.toml" ./rustfmt.toml
cp "$AGENTLINTERS_ROOT/assets/rust/clippy.toml" ./clippy.toml
mkdir -p .cargo
cp "$AGENTLINTERS_ROOT/assets/rust/.cargo/config.toml" ./.cargo/config.toml
rm -rf .dylint
cp -R "$AGENTLINTERS_ROOT/assets/rust/.dylint" ./.dylint

cp "$AGENTLINTERS_ROOT/assets/typescript/.oxfmtrc.json" ./.oxfmtrc.json
cp "$AGENTLINTERS_ROOT/assets/typescript/.oxlintrc.json" ./.oxlintrc.json
```

This avoids the Agent Linters CLI accidentally skipping or copying over the workspace `Cargo.toml`.

Install the Rust custom-lint prerequisites in Task 1 and CI:

```bash
cargo install --locked cargo-dylint dylint-link
rustup toolchain install nightly-2025-09-18
rustup component add --toolchain nightly-2025-09-18 rustc-dev llvm-tools-preview
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
```

### Oxfmt/Oxlint

Create `package.json`:

```json
{
  "private": true,
  "packageManager": "pnpm@latest",
  "scripts": {
    "format": "cargo fmt --all && oxfmt .",
    "format:check": "cargo fmt --all -- --check && oxfmt --check .",
    "lint": "cargo clippy --workspace --all-targets --all-features -- -D warnings && oxlint .",
    "check": "bash scripts/check.sh"
  },
  "devDependencies": {
    "oxfmt": "latest",
    "oxlint": "latest"
  }
}
```

Run:

```bash
corepack enable
pnpm install
pnpm run format:check
pnpm run lint
```

### Specgate

Use Specgate for architecture/ownership and policy-diff gates. Because the current installed CLI identifies as TS/JS-oriented, pair it with `scripts/rust-boundary-check.sh` for Rust module import/boundary checks until Specgate has native Rust import resolution.

Create `specgate.config.yml`:

```yaml
spec_dirs:
  - "modules"
exclude:
  - "**/.git/**"
  - "**/target/**"
  - "**/node_modules/**"
  - "**/coverage/**"
  - "**/bench/results.json"
  - "**/fixtures/generated/**"
test_patterns:
  - "**/tests/**"
  - "**/*_test.rs"
  - "**/benches/**"
strict_ownership: true
strict_ownership_level: errors
```

Run after module specs exist:

```bash
specgate validate
specgate check --output-mode deterministic
specgate doctor ownership --project-root . --format json
```

## Task dependency graph

```text
Task 0  plan/repo preflight
Task 1  workspace/tooling scaffold
Task 2  public contracts/model/error seams
├─ Task 3  frontmatter/schema/serializer
├─ Task 4  tree/config/IDs
│  └─ Task 8  git init/adopt/preflight/fetch
├─ Task 5  markdown/events/durable transaction/repair queues
│  ├─ Task 7  watcher integration
│  └─ Task 10 public API async/blocking hardening
├─ Task 6  SQLite index/chunks/vector adapter
│  ├─ Task 7  watcher integration
│  └─ Task 11 e2e/fault/perf gates
└─ Task 9a semantic merge library
   └─ Task 9b merge-driver CLI/fuzz
      └─ Task 8 git merge integration

Task 12 docs/CI/release gates depends on Tasks 1-11.
Task 13 final review/revision depends on Tasks 1-12.
```

---

## Task 0: Implementation preflight and plan validation

**Parallel:** no
**Blocked by:** none
**Owned files:** `docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md`
**Subagent type:** `plan_checker` after this file is written
**Skills:** `clean-code`, `tdd`, `spec-quality-checklist`, `writing-plans`

**Invariants:** Do not modify spec files. Do not start implementation until plan review is complete.
**Out of scope:** Any code creation beyond this plan.

**Files:**

- Create: `docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md`
- Read: `docs/specs/stream-a-core-substrate-v1.0.md`
- Read: `docs/specs/system-v0.1.md`
- Read: `docs/handoff-2026-04-23.md`

**Step 1: Validate spec implementation readiness**

Run:

```bash
rg -n "TBD|TODO|open|deliberately does not decide|Acceptance signals|Overall acceptance" docs/specs/stream-a-core-substrate-v1.0.md
```

Expected: no substrate-blocking TBDs; §21 open items belong to later streams.

**Step 2: Review this plan**

Spawn `plan_checker` with this file and ask for:

- missing dependencies;
- overlapping owned files;
- vague acceptance criteria;
- missing verification commands;
- mismatch with spec acceptance signals.

**Step 3: Revise plan once**

Apply concrete review findings only. Do not expand into implementation.

**Verification plan:**

```bash
test -f docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md
rg -n "\*\*Owned files:\*\*" docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md
```

---

## Task 1: Workspace, tooling, and architecture gates

**Parallel:** no
**Blocked by:** Task 0
**Owned files:** `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `.cargo/**`, `.dylint/**`, `package.json`, `pnpm-lock.yaml`, `.oxfmtrc.json`, `.oxlintrc.json`, `specgate.config.yml`, `modules/**`, `scripts/install-agentlinters.sh`, `scripts/check.sh`, `scripts/rust-boundary-check.sh`, `.github/workflows/stream-a-ci.yml`, `crates/memory-substrate/Cargo.toml`, `crates/memory-substrate/src/lib.rs`, `crates/memory-substrate/tests/workspace_smoke.rs`, `crates/memory-merge-driver/Cargo.toml`, `crates/memory-merge-driver/src/main.rs`, `crates/memory-test-support/Cargo.toml`, `crates/memory-test-support/src/lib.rs`
**Subagent type:** `heavy_worker`, reviewed by `code_mapper` and `reviewer`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Tooling must not depend on globally installed `agentlint`, `oxfmt`, or `oxlint`. Rust workspace root `Cargo.toml` must not be overwritten by Agent Linters assets.
**Out of scope:** Substrate behavior implementation.

**Files:**

- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `crates/memory-substrate/Cargo.toml`
- Create: `crates/memory-substrate/src/lib.rs`
- Create: `crates/memory-merge-driver/Cargo.toml`
- Create: `crates/memory-merge-driver/src/main.rs`
- Create: `crates/memory-test-support/Cargo.toml`
- Create: `crates/memory-test-support/src/lib.rs`
- Create: `scripts/install-agentlinters.sh`
- Create: `scripts/check.sh`
- Create: `scripts/rust-boundary-check.sh`
- Create: `specgate.config.yml`
- Create: `modules/*.spec.yml`
- Create: `.github/workflows/stream-a-ci.yml`

**Step 1: Write the failing workspace smoke test**

Create `crates/memory-substrate/tests/workspace_smoke.rs`:

```rust
#[test]
fn workspace_exposes_substrate_version() {
    assert_eq!(memory_substrate::STREAM_A_SPEC_VERSION, "1.0");
}
```

Run:

```bash
cargo test -p memory-substrate workspace_exposes_substrate_version
```

Expected: FAIL because the workspace/crate/constant does not exist.

**Step 2: Create the minimal workspace**

Create the three crates and expose `pub const STREAM_A_SPEC_VERSION: &str = "1.0";`.

**Step 3: Install lint/format configs**

Run:

```bash
bash scripts/install-agentlinters.sh
corepack enable
pnpm install
```

Expected: `rustfmt.toml`, `clippy.toml`, `.dylint/`, `.oxfmtrc.json`, `.oxlintrc.json` exist; `pnpm-lock.yaml` is created.

**Step 4: Add Specgate ownership specs**

Create module specs for each owned domain:

- `modules/stream-a-frontmatter.spec.yml`
- `modules/stream-a-tree-config-ids.spec.yml`
- `modules/stream-a-io-events.spec.yml`
- `modules/stream-a-index-vector.spec.yml`
- `modules/stream-a-git-merge.spec.yml`
- `modules/stream-a-tests-quality.spec.yml`

The first pass can be ownership-only; do not claim Rust dependency enforcement unless `scripts/rust-boundary-check.sh` implements it.

**Step 5: Green the smoke test**

Run:

```bash
cargo test -p memory-substrate workspace_exposes_substrate_version
```

Expected: PASS.

**Verification plan:**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
cargo test --workspace
pnpm run format:check
pnpm run lint
specgate validate
specgate check --output-mode deterministic
specgate doctor ownership --project-root . --format json
```

---

## Task 2: Public model, error taxonomy, and module seams

**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memory-substrate/src/api.rs`, `crates/memory-substrate/src/error.rs`, `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/runtime/mod.rs`, `crates/memory-substrate/tests/api_contracts.rs`
**Subagent type:** `backend_arch`, implemented by `heavy_worker`, reviewed by `reviewer`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Public API names and outcome semantics must match spec §16. No raw mutable SQLite connection is exported. No public sync method hides blocking filesystem/SQLite/git/network work.
**Out of scope:** Actual module internals beyond stubbed typed errors/outcomes.

**Files:**

- Create: `crates/memory-substrate/src/api.rs`
- Create: `crates/memory-substrate/src/error.rs`
- Create: `crates/memory-substrate/src/model.rs`
- Create: `crates/memory-substrate/src/runtime/mod.rs`
- Serialized orchestrator touch: `crates/memory-substrate/src/lib.rs` from Task 1; add module exports only after Task 1 is complete.
- Test: `crates/memory-substrate/tests/api_contracts.rs`

**Step 1: Write public contract compile tests**

Create tests that compile against:

- `Substrate::open`
- `Substrate::init`
- `Substrate::adopt_clone`
- `Substrate::doctor`
- `read_memory`, `read_path`, `write_memory`, `write_encrypted`, `tombstone_memory`
- `next_memory_id`, `reindex`, `query_memory`, `query_chunks`, `update_embedding`
- `git_preflight`, `fetch_inspect`, `auto_commit`, `fetch_and_merge`, `push`
- `durability_tier`

Run:

```bash
cargo test -p memory-substrate api_contracts -- --nocapture
```

Expected: FAIL because types/functions do not exist.

**Step 2: Add public types and typed errors**

Define minimal structs/enums:

- `Roots`, `InitOptions`, `AdoptOptions`, `DoctorReport`
- `WriteRequest`, `EncryptedWriteRequest`, `TombstoneRequest`
- `WriteOutcome`, `RepairRequired`, `DurabilityTier`
- `MemoryId`, `RepoPath`, `OperationId`, `EventId`, `Sha256`
- error enums named in §16.6.

Use `thiserror` and explicit error variants. Avoid `anyhow` in public APIs.

**Step 3: Add module shells**

Expose module names from the spec:

```rust
pub mod config;
pub mod events;
pub mod frontmatter;
pub mod git;
pub mod ids;
pub mod index;
pub mod markdown;
pub mod merge;
pub mod tree;
pub mod watcher;
```

Each module may contain only placeholders until its task owns the implementation.

**Step 4: Green compile/API tests**

Run:

```bash
cargo test -p memory-substrate api_contracts
cargo doc -p memory-substrate --no-deps
```

Expected: PASS.

**Verification plan:**

```bash
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
cargo test -p memory-substrate api_contracts
```

---

## Task 3: Frontmatter parser, validator, and canonical serializer

**Parallel:** yes
**Blocked by:** Task 2
**Owned files:** `crates/memory-substrate/src/frontmatter/**`, `crates/memory-substrate/tests/frontmatter_*.rs`, `fixtures/frontmatter/**`
**Subagent type:** `heavy_worker`, reviewed by `test_hardener` and `reviewer`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Every §6 field/rule has positive and negative tests. Missing nullable/collection fields warn and materialize typed defaults. Higher schema versions are read-only. Unknown v1 fields are preserved. Canonical serialization is byte-stable.
**Out of scope:** Cross-file graph validation beyond frontmatter-local reference fields; owned by Task 4.

**Files:**

- Create: `crates/memory-substrate/src/frontmatter/mod.rs`
- Create: `crates/memory-substrate/src/frontmatter/schema.rs`
- Create: `crates/memory-substrate/src/frontmatter/parse.rs`
- Create: `crates/memory-substrate/src/frontmatter/validate.rs`
- Create: `crates/memory-substrate/src/frontmatter/serialize.rs`
- Create: `crates/memory-substrate/src/frontmatter/defaults.rs`
- Create: `fixtures/frontmatter/valid/*.md`
- Create: `fixtures/frontmatter/invalid/*.md`
- Test: `crates/memory-substrate/tests/frontmatter_schema.rs`
- Test: `crates/memory-substrate/tests/frontmatter_roundtrip.rs`

**Step 1: First red test**

Test: `parses_missing_nullable_fields_with_typed_defaults_and_warnings`.

Run:

```bash
cargo test -p memory-substrate parses_missing_nullable_fields_with_typed_defaults_and_warnings -- --nocapture
```

Expected: FAIL because parser does not exist.

**Step 2: Minimal parser/defaults**

Implement:

- frontmatter delimiter extraction;
- YAML parse with `yaml_serde`;
- typed defaults from §6.2;
- `ValidationWarning::AutoPopulatedNullableField`.

**Step 3: Add schema/cross-field slices one at a time**

For each field/rule:

1. Add one negative fixture.
2. Add one positive fixture if the shape is new.
3. Run the targeted test.
4. Implement the smallest validator extension.

Minimum named tests:

- `rejects_missing_required_scalar`
- `rejects_bad_enum`
- `rejects_bad_author_shape`
- `rejects_secret_sensitivity_on_disk`
- `rejects_invalid_lifecycle_matrix_pair`
- `validates_prospective_time_event_and_condition_triggers`
- `validates_tombstone_with_two_events`
- `preserves_unknown_v1_extras`
- `refuses_mutation_for_higher_schema_version`
- `serializes_canonical_key_order_byte_stably`

**Step 4: Add property tests**

Use `proptest` for:

- parse/serialize/parse stability;
- deterministic sorting of tags/aliases/IDs/evidence/entities/tombstones;
- unknown extras preservation under supported schema.

**Verification plan:**

```bash
cargo test -p memory-substrate frontmatter -- --nocapture
cargo test -p memory-substrate --test frontmatter_roundtrip
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

---

## Task 4: Tree layout, config loading, and ID allocation/recovery

**Parallel:** yes
**Blocked by:** Task 2; integrate with Task 3 after frontmatter parser lands
**Owned files:** `crates/memory-substrate/src/tree/**`, `crates/memory-substrate/src/config/**`, `crates/memory-substrate/src/ids/**`, `crates/memory-substrate/tests/tree_*.rs`, `crates/memory-substrate/tests/config_*.rs`, `crates/memory-substrate/tests/ids_*.rs`, `fixtures/tree/**`
**Subagent type:** `heavy_worker`, reviewed by `test_hardener`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Device identity lives only in local runtime state. ID allocation scans repo-visible high-water marks. Duplicate repair mints from the full repo-visible reserved set and rewrites references or quarantines. Case-folded path uniqueness is enforced.
**Out of scope:** Git commits and merge driver config; owned by Task 8.

**Files:**

- Create: `crates/memory-substrate/src/tree/mod.rs`
- Create: `crates/memory-substrate/src/tree/layout.rs`
- Create: `crates/memory-substrate/src/tree/validate.rs`
- Create: `crates/memory-substrate/src/config/mod.rs`
- Create: `crates/memory-substrate/src/config/load.rs`
- Create: `crates/memory-substrate/src/ids/mod.rs`
- Create: `crates/memory-substrate/src/ids/sequence.rs`
- Create: `crates/memory-substrate/src/ids/repair.rs`
- Create: `fixtures/tree/case_collision/`
- Create: `fixtures/tree/duplicate_ids/`
- Test: `crates/memory-substrate/tests/tree_validation.rs`
- Test: `crates/memory-substrate/tests/config_loading.rs`
- Test: `crates/memory-substrate/tests/id_allocation.rs`

**Step 1: Red test for init/adopt path contract**

Test: `fresh_init_creates_working_tree_dirs_and_tracked_bootstrap_files`.

Run:

```bash
cargo test -p memory-substrate fresh_init_creates_working_tree_dirs_and_tracked_bootstrap_files -- --nocapture
```

Expected: FAIL.

**Step 2: Implement layout creation and validation skeleton**

Create directories and bootstrap file descriptors, without git commit behavior.

**Step 3: Red/green ID allocation slices**

Add tests in this order:

- `allocates_10000_monotonic_ids_for_one_device`
- `rejects_device_mismatch_in_seq_json`
- `sequence_999999_succeeds_and_1000000_fails`
- `advances_seq_past_repo_visible_same_shard_ids`
- `adoption_regenerates_for_forced_shard_collision`

**Step 4: Red/green tree validation slices**

Add tests:

- duplicate frontmatter IDs fail;
- case-only path collisions fail on macOS and Linux;
- ID filename/frontmatter mismatch fails;
- supersession cycles fail;
- partial sync missing references warn;
- fully synced missing references error.

**Step 5: Red/green duplicate repair**

Test: `duplicate_repair_mints_next_unused_repo_visible_id_and_rewrites_safe_refs`.

If references cannot be rewritten safely, assert quarantine instead of silent drop.

**Verification plan:**

```bash
cargo test -p memory-substrate tree_validation config_loading id_allocation -- --nocapture
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

---

## Task 5: Durable Markdown writes, event logs, and repair queues

**Parallel:** yes after Tasks 3-4
**Blocked by:** Tasks 3, 4
**Owned files:** `crates/memory-substrate/src/markdown/**`, `crates/memory-substrate/src/events/**`, `crates/memory-substrate/src/runtime/faults.rs`, `crates/memory-substrate/tests/markdown_*.rs`, `crates/memory-substrate/tests/events_*.rs`, `fixtures/events/**`
**Subagent type:** `heavy_worker`, reviewed by `test_hardener` and `security_auditor`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`, `harden`

**Invariants:** Same-directory temp files only. CAS protects human edits. Parent directory fsync tier is probed and surfaced. Event append recovery truncates only one invalid trailing line. Every committed-but-incomplete state has durable repair metadata or a typed operator-required failure.
**Out of scope:** SQLite indexing internals; Task 6 plugs into the transaction hooks.

**Files:**

- Create: `crates/memory-substrate/src/markdown/mod.rs`
- Create: `crates/memory-substrate/src/markdown/atomic.rs`
- Create: `crates/memory-substrate/src/markdown/cas.rs`
- Create: `crates/memory-substrate/src/markdown/durability.rs`
- Create: `crates/memory-substrate/src/events/mod.rs`
- Create: `crates/memory-substrate/src/events/framing.rs`
- Create: `crates/memory-substrate/src/events/log.rs`
- Create: `crates/memory-substrate/src/events/recovery.rs`
- Create: `crates/memory-substrate/src/runtime/faults.rs`
- Test: `crates/memory-substrate/tests/markdown_atomic_write.rs`
- Test: `crates/memory-substrate/tests/event_log.rs`
- Test: `crates/memory-substrate/tests/event_kind_schema.rs`
- Test: `crates/memory-substrate/tests/repair_queues.rs`

**Step 1: Red test for stale-base CAS**

Test: `stale_base_write_returns_stale_base_and_leaves_file_unchanged`.

Run:

```bash
cargo test -p memory-substrate stale_base_write_returns_stale_base_and_leaves_file_unchanged -- --nocapture
```

Expected: FAIL.

**Step 2: Implement read/hash/CAS and same-directory temp writes**

Green the CAS test, then add `temp_file_is_created_in_target_parent`.

**Step 3: Add durability tier probe**

Tests:

- `full_tier_requires_parent_fsync_success`
- `best_effort_requires_per_write_opt_in`
- `refused_tier_blocks_open_without_force_unsafe`

Use fault injection, not platform coincidence.

**Step 4: Implement event framing and recovery**

Tests:

- complete append persists CRC32C-framed JSONL line;
- crash during final append truncates exactly final malformed line;
- non-final malformed line quarantines log;
- duplicate identical event is idempotent;
- duplicate ID with different checksum errors.

**Step 5: Add typed event-kind schema coverage**

Test: `every_spec_event_kind_has_typed_payload_fixture`.

Requirements:

- table-driven fixture for every §12.2 event kind;
- compile-time/serde coverage that each kind maps to a typed payload enum variant;
- public event constructors cannot accept free-form `serde_json::Value` payloads;
- fixture count equals the event-kind count in `docs/specs/stream-a-core-substrate-v1.0.md`.

Run:

```bash
cargo test -p memory-substrate every_spec_event_kind_has_typed_payload_fixture -- --nocapture
```

Expected before implementation: FAIL.

**Step 6: Implement committed-but-incomplete repair queues**

Fault-injection tests:

- index transaction failure after durable rename;
- event append failure after index;
- pending queue append failure after durable commit;
- marker write failure after queue failure;
- full startup scan marker as final fallback.

**Step 7: Add sensitive write refusal tests**

Tests:

- `secret_write_refuses_before_temp_file_index_or_event`
- `confidential_write_does_not_place_plaintext_in_repo_path`

**Verification plan:**

```bash
cargo test -p memory-substrate markdown_atomic_write event_log event_kind_schema repair_queues -- --nocapture
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
```

---

## Task 6: SQLite index, FTS chunks, and sqlite-vec adapter

**Parallel:** yes after Tasks 3-4
**Blocked by:** Tasks 3, 4
**Owned files:** `crates/memory-substrate/src/index/**`, `crates/memory-substrate/tests/index_*.rs`, `crates/memory-substrate/benches/**`, `fixtures/index/**`, `bench/**`
**Subagent type:** `heavy_worker`, reviewed by `performance_engineer` and `test_hardener`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `performance`, `optimize`

**Invariants:** SQLite is derived and rebuildable. `memory_chunks.chunk_rowid` is explicit `INTEGER PRIMARY KEY AUTOINCREMENT`. FTS external-content triggers use delete+insert keyed on `chunk_rowid`. Vector consistency is durable jobs plus stale-hash checks, never assumed transaction rollback. Metadata-only confidential/personal memories are excluded from FTS/vector results.
**Out of scope:** Embedding model inference worker; Stream B drains jobs.

**Files:**

- Create: `crates/memory-substrate/src/index/mod.rs`
- Create: `crates/memory-substrate/src/index/schema.rs`
- Create: `crates/memory-substrate/src/index/migrations.rs`
- Create: `crates/memory-substrate/src/index/chunking.rs`
- Create: `crates/memory-substrate/src/index/query.rs`
- Create: `crates/memory-substrate/src/index/vector.rs`
- Create: `crates/memory-substrate/src/index/sqlite_vec.rs`
- Test: `crates/memory-substrate/tests/index_schema.rs`
- Test: `crates/memory-substrate/tests/index_queries.rs`
- Test: `crates/memory-substrate/tests/vector_lifecycle.rs`
- Bench: `crates/memory-substrate/benches/stream_a_perf.rs`
- Output: `bench/results.json`

**Step 1: Red test for schema invariant**

Test: `chunk_rowid_survives_vacuum_for_fts_join`.

Run:

```bash
cargo test -p memory-substrate chunk_rowid_survives_vacuum_for_fts_join -- --nocapture
```

Expected: FAIL.

**Step 2: Implement migrations and FTS tables**

Create schema from §10.1 and a migration runner with `schema_migrations`.

**Step 3: Add indexer behavior slices**

Tests:

- `created_memory_replaces_row_and_derived_rows_transactionally`
- `update_removes_old_fts_terms`
- `delete_cascades_chunks_tags_aliases_entities_evidence_regressions_jobs`
- `rename_with_same_id_updates_path`
- `rename_with_new_id_deletes_old_and_upserts_new`
- `reindex_from_files_matches_incremental_index_state`

**Step 4: Add chunking contract**

Tests:

- chunk IDs change when chunk text changes;
- offsets are byte offsets into LF-normalized body;
- >1 MiB body is streamed/artifacted, not copied into one body column.

**Step 5: Implement sqlite-vec adapter**

Use `sqlite_vec::sqlite3_vec_init` with rusqlite auto-extension registration. DDL for vector tables is generated by adapter code and version-pinned by `Cargo.lock`.

Tests:

- `update_embedding_rejects_stale_chunk_hash`
- `delete_tombstone_or_sensitivity_change_purges_vectors_after_reconciliation`
- `startup_reconciliation_deletes_orphan_vectors`
- `startup_reconciliation_requeues_missing_vectors`

**Step 6: Query helper contract**

Tests:

- by ID/path;
- tag/entity/alias;
- namespace/scope/status/type/sensitivity/time;
- FTS snippets;
- vector search;
- hybrid score input assembly;
- metadata-only memory appears only when `include_metadata_only = true`.

**Verification plan:**

```bash
cargo test -p memory-substrate index_schema index_queries vector_lifecycle -- --nocapture
cargo bench -p memory-substrate --bench stream_a_perf -- --sample-size 10
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

---

## Task 7: File watcher integration and self-event suppression

**Parallel:** yes after Tasks 5-6
**Blocked by:** Tasks 5, 6
**Owned files:** `crates/memory-substrate/src/watcher/**`, `crates/memory-substrate/tests/watcher_*.rs`, `fixtures/watcher/**`
**Subagent type:** `heavy_worker`, reviewed by `test_hardener`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Suppression is hash-based. Stream A writes update index directly before returning. External edits inside suppression window are processed when file hash differs. Watcher overflow forces rescan; correctness does not depend on exact OS event counts.
**Out of scope:** Daemon lifecycle and live subscriptions beyond owned `WatchSubscription` handle.

**Files:**

- Create: `crates/memory-substrate/src/watcher/mod.rs`
- Create: `crates/memory-substrate/src/watcher/filter.rs`
- Create: `crates/memory-substrate/src/watcher/suppression.rs`
- Create: `crates/memory-substrate/src/watcher/subscription.rs`
- Test: `crates/memory-substrate/tests/watcher_suppression.rs`
- Test: `crates/memory-substrate/tests/watcher_rescan.rs`

**Step 1: Red test for self-event suppression**

Test: `substrate_write_indexes_even_when_watcher_event_is_suppressed`.

Run:

```bash
cargo test -p memory-substrate substrate_write_indexes_even_when_watcher_event_is_suppressed -- --nocapture
```

Expected: FAIL.

**Step 2: Implement suppression ledger**

Add `InFlight` and `Committed` entries keyed by op/path/hash with expiry.

**Step 3: Add external edit and overflow tests**

Tests:

- `external_edit_within_suppression_window_is_indexed_when_hash_differs`
- `event_between_rename_and_committed_promotion_is_suppressed`
- `watcher_overflow_emits_rescan_required_and_reindex_converges`
- `mass_changes_converge_to_fresh_reindex_state`

**Verification plan:**

```bash
cargo test -p memory-substrate watcher_suppression watcher_rescan -- --nocapture
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

---

## Task 8: Git init, clone adoption, preflight, commit, fetch/merge, and push

**Parallel:** yes after Tasks 4 and 9 core merge API
**Blocked by:** Tasks 4, 5, 9b
**Owned files:** `crates/memory-substrate/src/git/**`, `crates/memory-substrate/tests/git_*.rs`, `fixtures/git/**`, `scripts/two-clone-convergence.sh`
**Subagent type:** `cli_developer`, reviewed by `security_auditor` and `test_hardener`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`, `harden`

**Invariants:** Preflight refuses before merge when merge driver config/binary is missing. Clone adoption never inherits another machine's device ID/event sequence. Auto-commit uses durable events, not raw watcher events. Git commands run with explicit args and validated repo root.
**Out of scope:** Field-level merge rules; owned by Task 9a.

**Files:**

- Create: `crates/memory-substrate/src/git/mod.rs`
- Create: `crates/memory-substrate/src/git/command.rs`
- Create: `crates/memory-substrate/src/git/init.rs`
- Create: `crates/memory-substrate/src/git/adopt.rs`
- Create: `crates/memory-substrate/src/git/preflight.rs`
- Create: `crates/memory-substrate/src/git/sync.rs`
- Create: `crates/memory-substrate/src/git/commit.rs`
- Test: `crates/memory-substrate/tests/git_init_adopt.rs`
- Test: `crates/memory-substrate/tests/git_fetch_merge.rs`
- Test: `crates/memory-substrate/tests/git_preflight.rs`
- Script: `scripts/two-clone-convergence.sh`

**Step 1: Red test for fresh clone preflight**

Test: `fresh_clone_without_adoption_fails_preflight_with_repair_instruction`.

Run:

```bash
cargo test -p memory-substrate fresh_clone_without_adoption_fails_preflight_with_repair_instruction -- --nocapture
```

Expected: FAIL.

**Step 2: Implement init/adopt**

Tests:

- init writes `.gitattributes`, `.gitignore`, `config.yaml`, local device config, event log;
- merge driver is configured with absolute binary path or stable shim path;
- adoption regenerates local device identity when needed;
- adoption recreates untracked directories;
- adoption commits new event log if policy requires.

**Step 3: Implement preflight and inspect-only fetch**

Tests:

- missing driver binary refuses before merge;
- stale `.gitattributes` can be inspected without merge;
- unresolved conflict markers refuse;
- invalid quarantine files refuse.

**Step 4: Implement auto-commit**

Tests:

- groups changed paths by parsed metadata namespace;
- `git add -A` is constrained inside repo root;
- deterministic message includes required trailers.

**Step 5: Implement fetch/merge/push**

Tests:

- ahead-only branch does not incorrectly skip future behind state;
- behind/diverged branch merges;
- valid semantic quarantines produce events;
- duplicate ID repair/reindex/auto-commit runs after merge;
- JSONL union duplicates replay idempotently;
- push failures emit `GitPushFailed`.

**Verification plan:**

```bash
cargo test -p memory-substrate git_init_adopt git_fetch_merge git_preflight -- --nocapture
./scripts/two-clone-convergence.sh --smoke
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

---

## Task 9a: Semantic frontmatter merge library

**Parallel:** yes after Task 3
**Blocked by:** Task 3
**Owned files:** `crates/memory-substrate/src/merge/**`, `crates/memory-substrate/tests/merge_*.rs`, `fixtures/merge/library/**`
**Subagent type:** `heavy_worker`, reviewed by `test_hardener`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Merge library uses true 3-way field rules. Add/add quarantine preserves both logical memories mechanically. `updated_at` never stomps independent field edits. Library output must validate through Task 3 frontmatter rules.
**Out of scope:** CLI argv handling, writing `<ours>`, and fuzz target wiring; owned by Task 9b. Repo-level duplicate-ID repair/reference rewrites; owned by Task 4/8.

**Files:**

- Create: `crates/memory-substrate/src/merge/mod.rs`
- Create: `crates/memory-substrate/src/merge/three_way.rs`
- Create: `crates/memory-substrate/src/merge/field_rules.rs`
- Create: `crates/memory-substrate/src/merge/lifecycle.rs`
- Create: `crates/memory-substrate/src/merge/quarantine.rs`
- Test: `crates/memory-substrate/tests/merge_rules.rs`
- Fixtures: `fixtures/merge/library/**/*.yml`

**Step 1: Red test for independent scalar edits**

Test: `independent_scalar_edits_both_survive`.

Run:

```bash
cargo test -p memory-substrate independent_scalar_edits_both_survive -- --nocapture
```

Expected: FAIL.

**Step 2: Implement generic true 3-way rule**

Green independent edits without field-specific shortcuts.

**Step 3: Add field-rule slices**

Add tests for each §14.4 field family:

- immutable conflicts quarantine;
- summary/confidence same-field rules;
- safety-stricter retrieval/write policy keys;
- regression occurrence ID/G-counter merge;
- evidence ID/hash near-duplicate diagnostics;
- privacy scan model preservation;
- unknown extras true 3-way.

**Step 4: Add lifecycle matrix fixtures**

Generate fixture cases for every pair in §14.5. Each case asserts valid output or expected quarantine.

**Step 5: Add add/add and unparsable-side quarantine**

Tests:

- frontmatter IDs differ: valid quarantine contains primary plus `add_add_alternates`;
- original frontmatters and bodies recover byte-for-byte;
- same ID: valid quarantine signals duplicate-ID repair;
- invalid YAML with delimiters produces `unparsed_sides`;
- absent delimiters exits `1` and leaves Git conflict handling.

**Step 6: Add fixture manifest count**

Test: `merge_fixture_manifest_has_minimum_required_cases`.

Requirements:

- manifest covers at least 60 fixtures from §17.4;
- lifecycle pair matrix has every pair;
- each fixture maps to a spec acceptance bullet.

**Verification plan:**

```bash
cargo test -p memory-substrate merge_rules -- --nocapture
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
```

---

## Task 9b: Merge-driver CLI and fuzz target

**Parallel:** no
**Blocked by:** Task 9a
**Owned files:** `crates/memory-merge-driver/**`, `crates/memory-merge-driver/tests/**`, `fixtures/merge/cli/**`, `fuzz/**`
**Subagent type:** `cli_developer`, reviewed by `test_hardener`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** Merge driver is path-local and only writes `<ours>`. Valid semantic quarantine exits `0`; unsupported schema or unrepresentable conflicts exit `1`. Fuzzing never panics and never emits invalid YAML.
**Out of scope:** Field-rule implementation; owned by Task 9a.

**Files:**

- Modify: `crates/memory-merge-driver/src/main.rs`
- Test: `crates/memory-merge-driver/tests/merge_driver_cli.rs`
- Fixtures: `fixtures/merge/cli/**/*.yml`
- Fuzz: `fuzz/fuzz_targets/merge_driver.rs`

**Step 1: Red test for CLI required args**

Test: `merge_driver_requires_base_ours_theirs_and_path_args`.

Run:

```bash
cargo test -p memory-merge-driver merge_driver_requires_base_ours_theirs_and_path_args -- --nocapture
```

Expected: FAIL.

**Step 2: Implement CLI wrapper**

Tests:

- `--base --ours --theirs --path` required;
- writes only `<ours>`;
- schema version 2 exits `1` with "schema_version exceeds supported";
- semantic quarantine exits `0`;
- absent frontmatter delimiters exits `1` and leaves Git conflict handling.

**Step 3: Fuzz**

Run locally:

```bash
cargo fuzz run merge_driver -- -max_total_time=600
```

Expected: no panics and no invalid YAML output.

**Verification plan:**

```bash
cargo test -p memory-merge-driver -- --nocapture
cargo fuzz run merge_driver -- -max_total_time=600
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
```

---

## Task 10: Public API implementation, async/blocking boundaries, and startup reconciliation

**Parallel:** no
**Blocked by:** Tasks 5, 6, 7, 8, 9b
**Owned files:** `crates/memory-substrate/src/runtime/**`, `crates/memory-substrate/tests/api_*.rs`, `crates/memory-substrate/tests/startup_reconciliation.rs`, `crates/memory-substrate/tests/error_variant_coverage.rs`
**Subagent type:** `heavy_worker`, reviewed by read-only `backend_arch` and `review_guard`
**Skills:** `clean-code`, `tdd`, `rust-engineer`, `debugging-systematic`

**Invariants:** `Substrate::open` does not return until startup reconciliation is complete or operator-required error is returned. Public async methods offload blocking filesystem/SQLite/git/vector work. Cancellation cannot corrupt repo/index/event state; after durable commit, committed outcome is recorded.
**Out of scope:** New module behavior not required to wire public API.

**Files:**

- Serialized orchestrator touch: `crates/memory-substrate/src/api.rs` from Task 2; do not let parallel workers edit this file concurrently.
- Modify: `crates/memory-substrate/src/runtime/mod.rs`
- Create: `crates/memory-substrate/src/runtime/blocking.rs`
- Create: `crates/memory-substrate/src/runtime/reconcile.rs`
- Test: `crates/memory-substrate/tests/api_write_read.rs`
- Test: `crates/memory-substrate/tests/startup_reconciliation.rs`
- Test: `crates/memory-substrate/tests/cancellation.rs`
- Test: `crates/memory-substrate/tests/error_variant_coverage.rs`

**Step 1: Read-only architecture review**

Spawn `backend_arch` in read-only mode before implementation. It reviews module seams and confirms Task 10 can wire behavior without changing Task 3-9 ownership. It does not edit files.

**Step 2: Red API happy-path test**

Test: `write_read_query_and_event_round_trip_through_public_api`.

Run:

```bash
cargo test -p memory-substrate write_read_query_and_event_round_trip_through_public_api -- --nocapture
```

Expected: FAIL.

**Step 3: Wire modules through `Substrate`**

Implement `Substrate` as owner of roots/device/index/events/git handles. Keep internals private.

**Step 4: Implement startup reconciliation phases**

Tests for §13.5.1 phases:

- crash marker forces full reconciliation;
- incomplete merge marker forces reconciliation;
- valid offline human edits are ingested and scheduled for auto-commit;
- invalid edits/conflict markers/unknown non-memory paths are quarantined and writes are refused while operator-required items remain;
- vector reconciliation runs before accepting writes;
- event log recovery runs before accepting writes;
- pending index/event queues replay idempotently;
- file/index hash mismatch enqueues reindex;
- startup completion event includes phase counts.

**Step 5: Add error-variant coverage manifest**

Test: `every_public_error_variant_has_behavioral_coverage`.

Requirements:

- table maps every public error enum variant to at least one named test or fixture;
- test fails if any public error enum gains a variant without coverage metadata;
- variants from §16.6 must all be represented, including committed-but-incomplete outcomes and operator-required repair states.

Run:

```bash
cargo test -p memory-substrate every_public_error_variant_has_behavioral_coverage -- --nocapture
```

Expected before implementation: FAIL.

**Step 6: Cancellation and blocking tests**

Tests:

- cancelling before durable commit leaves no file/index/event;
- cancelling after durable commit records committed outcome or repair marker;
- no public sync method performs blocking git/SQLite/network work.

**Verification plan:**

```bash
cargo test -p memory-substrate api_write_read startup_reconciliation cancellation error_variant_coverage -- --nocapture
cargo fmt --all -- --check
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
```

---

## Task 11: End-to-end fault matrix, multi-device convergence, and performance gates

**Parallel:** yes after Task 10
**Blocked by:** Task 10
**Owned files:** `crates/memory-substrate/tests/e2e_*.rs`, `crates/memory-substrate/tests/crash_*.rs`, `crates/memory-substrate/benches/release_gate.rs`, `fixtures/perf/**`, `scripts/bench-gate.sh`, `scripts/durability-probe-gate.sh`, `bench/results.json`, `bench/durability-results.json`
**Subagent type:** `test_hardener` plus `performance_engineer`, reviewed by `security_auditor`
**Skills:** `clean-code`, `tdd`, `debugging-systematic`, `performance`, `optimize`, `rust-engineer`

**Invariants:** Fault tests use deterministic injection, not timing-only process kills. Performance gates write `bench/results.json` and fail on target misses or >10% regression. Two-clone test must converge byte-identically after semantic merges and repo reconciliation.
**Out of scope:** Implementing new substrate features to satisfy missing tests without routing back to owning tasks.

**Files:**

- Create: `crates/memory-substrate/tests/e2e_write_index_git.rs`
- Create: `crates/memory-substrate/tests/crash_matrix.rs`
- Create: `crates/memory-substrate/tests/multi_device_convergence.rs`
- Create: `fixtures/perf/seed_10k.rs` or deterministic generator under `crates/memory-test-support/src/perf.rs`
- Modify: `scripts/bench-gate.sh`
- Create: `scripts/durability-probe-gate.sh`
- Use: `scripts/two-clone-convergence.sh` from Task 8; route any required changes back through the Task 8 owner.

**Step 1: Red two-clone convergence test**

Run:

```bash
./scripts/two-clone-convergence.sh --full
```

Expected: FAIL until all git/merge/reconcile paths are wired.

**Step 2: Add e2e scenarios**

Tests:

- init/adopt/write/read/index/event/commit happy path;
- human editor watcher ingestion;
- stale-base programmatic conflict;
- multi-device divergent merge;
- duplicate device ID repair;
- sensitive encrypted write with no plaintext leakage;
- reindex equivalence.

**Step 3: Add crash matrix**

Faults:

- before write;
- during write;
- after temp fsync;
- after rename before parent fsync;
- after parent fsync before index;
- after index before event;
- during event append;
- pending repair queue append failure after durable commit.

Every committed-but-incomplete state must converge after startup reconciliation.

**Step 4: Add release performance gate**

Measure:

- 10K-memory cold reindex p95 <= 60s;
- query by ID p95 <= 10ms;
- filtered metadata query p95 <= 50ms;
- FTS chunk query p95 <= 75ms;
- vector chunk query p95 <= 100ms;
- tree validator p95 <= 500ms.

Write hardware/OS/filesystem/SQLite pragmas and results to `bench/results.json`.

**Step 5: Add release durability probe matrix**

Run:

```bash
./scripts/durability-probe-gate.sh --matrix apfs,tmpfs,ext4,einval,best-effort --output bench/durability-results.json
```

Expected:

- `Full` succeeds on supported APFS/ext4/tmpfs fixtures available on the current host profile;
- `Refused` fixture monkey-patching parent-dir fsync to `EINVAL` blocks `Substrate::open`;
- `BestEffort` fixture requires per-write opt-in;
- output JSON records host OS, filesystem, probe result, and exact command.

**Verification plan:**

```bash
cargo test --workspace --release
./scripts/two-clone-convergence.sh --full
./scripts/bench-gate.sh --tier release --output bench/results.json
./scripts/durability-probe-gate.sh --matrix apfs,tmpfs,ext4,einval,best-effort --output bench/durability-results.json
cargo fuzz run merge_driver -- -max_total_time=600
```

---

## Task 12: Documentation, CI, and release gates

**Parallel:** yes for draft docs after Task 10; final evidence update is blocked by Task 11
**Blocked by:** Task 10 for draft docs; Task 11 for final docs/CI evidence
**Owned files:** `docs/api/**`, `docs/dev/**`, `docs/runbooks/**`, `.github/workflows/**`, `README.md`
**Subagent type:** `docs_editor`, reviewed by `review_guard` and `docs_researcher`
**Skills:** `clean-code`, `tdd`, `doc`, `write-human`, `spec-quality-checklist`

**Invariants:** Docs must reflect actual commands and current API names. CI must run the same gates as local scripts. Do not document unsupported Specgate Rust import enforcement as if it exists.
**Out of scope:** Changing implementation to make docs easier.

**Files:**

- Create: `docs/api/stream-a-public-api.md`
- Create: `docs/dev/stream-a-architecture.md`
- Create: `docs/dev/stream-a-test-matrix.md`
- Create: `docs/runbooks/operator-repair.md`
- Create: `docs/runbooks/privacy-leak-response-placeholder.md`
- Modify: `README.md`
- Modify: `.github/workflows/stream-a-ci.yml`
- Create: `.github/workflows/stream-a-fuzz.yml`
- Create: `.github/workflows/stream-a-perf.yml`
- Use: `scripts/check.sh` from Task 1; route command changes back through the Task 1 owner.

**Step 1: Red docs command audit**

Add a script check or test that fails if documented commands are missing:

```bash
rg -n "scripts/check.sh|two-clone-convergence|bench-gate|cargo fuzz" docs README.md
```

Expected first run: FAIL until docs exist.

**Step 2: Write API docs**

Document:

- blocking/async behavior;
- every `WriteOutcome` state;
- every error family;
- metadata-only encrypted records;
- vector consistency limitations;
- startup reconciliation phases.

**Step 3: Write operator runbooks**

Runbooks:

- `DurabilityUnsupported` and forced test/CI open;
- merge driver missing/stale;
- operator-required quarantine at startup;
- event log quarantine;
- pending queue replay failure;
- privacy leak response placeholder for Stream D/G.

**Step 4: CI workflows**

Workflows:

- `stream-a-ci.yml`: fmt, clippy, unit/integration, Oxfmt/Oxlint, Specgate, rust boundary check.
- `stream-a-fuzz.yml`: 10-minute merge-driver fuzz.
- `stream-a-perf.yml`: scheduled/release performance gate on Linux x86_64; local Apple Silicon perf remains manual evidence unless CI runner exists.

**Verification plan:**

```bash
pnpm exec oxfmt --check docs README.md .github scripts
bash scripts/check.sh
```

---

## Task 13: Independent review, remediation, and final acceptance

**Parallel:** no
**Blocked by:** Tasks 1-12
**Owned files:** `docs/reviews/**`, no implementation files unless remediation tasks are opened
**Subagent type:** `review_guard`, plus `reviewer`, `security_auditor`, `performance_engineer`, `test_hardener`
**Skills:** `clean-code`, `tdd`, `receiving-code-review`, `caveman-review`, `harden`, `performance`

**Invariants:** Review is a release gate. Every blocking finding is fixed or explicitly rejected with evidence. No "green enough" release without §17.7 proof.
**Out of scope:** New feature scope beyond Stream A v1.0.

**Files:**

- Create: `docs/reviews/stream-a-final-review.md`
- Create: `docs/reviews/stream-a-security-review.md`
- Create: `docs/reviews/stream-a-performance-review.md`
- Create: `docs/reviews/stream-a-test-coverage-review.md`

**Step 1: Spawn independent review lanes**

Run in parallel:

- `reviewer`: correctness/maintainability against spec.
- `security_auditor`: secret/privacy/path/git safety.
- `test_hardener`: acceptance signal coverage and flaky risks.
- `performance_engineer`: perf harness and result quality.
- `review_guard`: final merge risk.

**Step 2: Triage findings**

Codex orchestrator creates remediation subtasks with owned files and exact commands. All remediation subagents still receive `clean-code` and `tdd`.

**Step 3: Full acceptance gate**

Run:

```bash
cargo test --workspace --release
cargo fuzz run merge_driver -- -max_total_time=600
./scripts/two-clone-convergence.sh --full
./scripts/bench-gate.sh --tier release --output bench/results.json
./scripts/durability-probe-gate.sh --matrix apfs,tmpfs,ext4,einval,best-effort --output bench/durability-results.json
specgate validate
specgate check --output-mode deterministic
specgate doctor ownership --project-root . --format json
pnpm exec oxfmt --check .
pnpm exec oxlint .
cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints
bash scripts/check.sh
```

Expected: PASS on local machine, plus Linux CI. Attach the following evidence in `docs/reviews/stream-a-performance-review.md` and `docs/reviews/stream-a-final-review.md`:

- macOS arm64 `cargo test --workspace --release` output;
- Linux x86_64 `cargo test --workspace --release` output;
- macOS and Linux perf result JSON;
- durability matrix JSON;
- merge-driver fuzz output.

**Step 4: Final acceptance mapping**

Create a table in `docs/reviews/stream-a-final-review.md` mapping every §17.7 acceptance criterion to exact command output and file evidence.

**Verification plan:**

```bash
test -s docs/reviews/stream-a-final-review.md
rg -n "§17.7|cargo test --workspace --release|merge_driver|two-clone|bench/results.json|Independent review" docs/reviews/stream-a-final-review.md
```

---

## Parallelization plan

Safe parallel lanes after Task 2:

1. Task 3 frontmatter.
2. Task 4 tree/config/IDs.

Safe parallel lanes after Tasks 3-4:

1. Task 5 markdown/events/repair.
2. Task 6 index/vector.
3. Task 9a merge library and Task 9b merge-driver CLI/fuzz.

Safe parallel lanes after Tasks 5-6-9:

1. Task 7 watcher.
2. Task 8 git, once Task 9b exposes merge driver binary behavior.

Task 10 serializes public API integration because it touches shared `api.rs`/runtime seams.

Task 11 and Task 12 can overlap only if docs are marked draft until Task 11 final perf/fault evidence lands.

Before parallel execution, run the owned-files duplicate check:

```bash
rg '\*\*Owned files:\*\*' docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md \
  | sed 's/.*\*\*Owned files:\*\* *//' \
  | tr ',' '\n' \
  | sed 's/`//g' \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | rg -v '^$' \
  | sort \
  | uniq -d
```

Expected: no duplicate exact file ownership in parallel task groups. Directory-level overlaps must be manually reviewed by `code_mapper` because this simple check cannot reason about glob containment.

Then spawn `code_mapper` for a read-only glob-containment review before any parallel workers start. Required output:

- no parallel task owns a child path of another parallel task's owned directory;
- no two parallel tasks write the same fixture namespace;
- any serialized orchestrator-touch files are listed explicitly;
- unsafe overlaps are converted into sequential subtasks before spawning workers.

## Acceptance signal coverage map

| Spec section | Implemented/gated by |
| --- | --- |
| §3 durability tiers | Tasks 5, 10, 11 |
| §5 tree layout/validator | Tasks 4, 8 |
| §6 frontmatter schema | Task 3 |
| §7 IDs/duplicate repair | Task 4 |
| §8 durable Markdown transaction | Task 5 |
| §9 validator | Tasks 3, 4 |
| §10 SQLite/index/vector | Task 6 |
| §11 watcher | Task 7 |
| §12 event log | Task 5 |
| §13 git operations | Task 8 |
| §14 merge driver | Tasks 9a, 9b |
| §15 configuration | Task 4 |
| §16 public API | Tasks 2, 10 |
| §17 tests/fuzz/perf/overall acceptance | Tasks 11, 13 |
| §18 risks | Tasks 5, 6, 8, 9, 11, 13 |
| §20 locked decisions | Tasks 5, 6, 9, 11, 13 |

## Stop conditions and escalation

Stop implementation and ask Trey only if one of these occurs:

1. The v1.0 spec contradicts itself in a way that changes persisted format or public API.
2. A required durability guarantee cannot be achieved on macOS 14+ or Linux 5.10+ without changing the spec.
3. sqlite-vec pre-v1 behavior makes the vector adapter contract impossible without pinning to an alpha/pre-release Trey has not approved.
4. Specgate must enforce Rust import graphs as a hard requirement, but current Specgate cannot; otherwise use Specgate for ownership/config and pair with `scripts/rust-boundary-check.sh`.
5. A security review finds a path where `secret` or plaintext confidential/personal content can hit repo files, SQLite FTS/vector rows, temp files, events, git, or logs.

## First execution command sequence

When Trey says "execute," Codex should start with:

```bash
git status --short --branch
git -C /Users/treygoff/Code/agentlinters pull --ff-only
git -C /Users/treygoff/Code/agentlinters rev-parse --short HEAD
spawn plan_checker for docs/plans/2026-04-25-stream-a-core-substrate-implementation-plan.md
```

Then open Task 1 after plan-check fixes are applied.
