# memoryd `main.rs` refactor — break the 1489-line god file into per-command modules

**Author:** Claude (orchestrator) • **Date:** 2026-05-28 • **Version:** v0.3 (plan-reviewer + codex review patches applied)

## Goal

Break `crates/memoryd/src/main.rs` (currently 1489 lines, with a 500-line `main()` containing a 23-arm `match cli.command` of inline business logic) into per-command modules under `crates/memoryd/src/cli/`. End state: `main.rs` is under 200 lines (arg parsing + dispatch match + signal handlers). Each command's runner lives in a focused, single-responsibility module under `cli/`. Pure helpers (response printing, exit codes, path resolution) live in their own helper modules.

Behavior-preserving. No protocol changes. No test changes (beyond what the workspace test suite naturally exercises through the new module paths). No public-API changes outside the `memoryd::cli` namespace.

This also closes a Stream-B-side plan deviation flagged on 2026-05-28: the 2026-05-27 importer plan called for `crates/memoryd/src/cli/import.rs` and `cli/init.rs` modules; the overnight implementation put both dispatchers in `main.rs`. This refactor lands those modules where the importer plan asked for them, alongside extractions for every other command.

## Non-goals

- No logic changes. No bug fixes. No performance work. Pure structural extraction.
- No new tests beyond what is mechanically required to keep the workspace gate green. The existing test suite (~1228 tests) is the behavior oracle.
- No changes to `cli.rs`'s public arg-parsing types (`Cli`, `Command`, `ServeArgs`, `InitArgs`, etc.) — those move from `cli.rs` to `cli/mod.rs` verbatim.
- No changes to other crates. Owned files are entirely under `crates/memoryd/src/`.
- No `.oxfmtignore` changes. No CI changes. No docs changes beyond the per-task review docs.

## Scope

### In scope

- Move `crates/memoryd/src/cli.rs` → `crates/memoryd/src/cli/mod.rs` (preserves the `memoryd::cli::*` import path).
- Create per-command runner modules under `cli/` (one per `Command` enum variant or per cohesive group of variants).
- Create helper modules under `cli/` for cross-command utilities (response printing, exit codes, path resolution).
- Rewrite `main.rs` so `main()` parses args, dispatches via a thin one-call-per-arm match, and installs signal handlers — and nothing else.
- All gates (workspace clippy, workspace tests, dogfood check) stay green at HEAD after the refactor.

### Out of scope

- Renaming any public type or function in the `memoryd::cli` namespace.
- Changing the dispatch model (still `match cli.command { ... }` in `main`; not switching to a trait-dispatch or a registry pattern — that's a v2 conversation).
- Reorganizing modules outside `crates/memoryd/src/cli/` (e.g. `dream/`, `handlers/`, `protocol.rs` — all stay put).
- Improving error messages, log lines, or CLI UX. Strings move verbatim with their owning code.

## Target module layout

```
crates/memoryd/src/
  main.rs              (< 200 LOC: imports, main(), match dispatch, signal/shutdown wiring)
  cli/
    mod.rs             (the existing cli.rs content — Cli, Command, all args/subcommand enums)
    serve.rs           (Serve command — daemon bootstrap, privacy enforcement install, signal wiring)
    daemon.rs          (Mcp + Status + Doctor — thin daemon-client calls + auto_start_daemon helper, used only by Mcp arm)
    memory.rs          (Search + Get + Write + WriteNote + Forget + Supersede — memory CRUD + parse_meta private helper)
    source.rs          (Source subcommands + source_capture_payload helper)
    review.rs          (Review subcommands)
    recall.rs          (Recall subcommands + recall_socket_path private helper)
    dream.rs           (Dream subcommands + run_manual_dream + run_scheduled_dream + execute_dream_run + run_dream_cleanup + dream helpers + DreamRunInvocation struct)
    peer.rs            (Peer subcommands + run_peer_release_lock + print_peer_status + print_peer_activity + confirmed_on_stdin private helper)
    ui.rs              (Ui command + run_tui)
    web.rs             (Web subcommands + WebOperation enum + print_web_response + print_web_status + web_protocol_exit_code)
    reality_check.rs   (RealityCheck subcommands + print_reality_check_run + print_reality_check_summary + print_reality_check_skip + print_reality_check_snooze + reality_check_error_exit_code)
    privacy.rs         (Privacy + PrivacyFilter + Device subcommands + record_device_keys_rotated_event)
    import.rs          (Import command + run_import_command + DefaultSkipPrompts struct + PromptBackend impl)
    init.rs            (Init command + run_init_command)
    output.rs          (print_response + governance_write_response_promoted_id + write_note_response_id + maybe_emit_first_write_banner)
    exit.rs            (exit_protocol_error + exit_recall_unavailable + exit_dream_error + recall_exit_code + doctor_cli_exit_code)
    paths.rs           (resolve_socket_arg + resolve_socket_with_runtime)
```

Seventeen new files under `cli/` (3 helpers + 14 per-command), plus `mod.rs` which is the moved-and-extended `cli.rs`. `main.rs` itself shrinks from 1489 → target ≤ 200 LOC.

## Architecture & invariants

### Visibility model

**Critical Rust correctness point:** `crates/memoryd/src/` is one Cargo package with two crates: a library (`lib.rs`, name `memoryd`) and a binary (`main.rs`, also named `memoryd`). The binary depends on the library. **`pub(crate)` items in the library are NOT visible to the binary** — they would fail to resolve at the `cli::serve::run` call site in `main.rs`.

The visibility rules workers follow:

| Item | Visibility | Reason |
|---|---|---|
| Per-command runner functions (`cli::<cmd>::run`, `run_<subcmd>`) that `main.rs` will dispatch to | `pub` | Called from the binary crate; `pub(crate)` would fail to resolve |
| Module-internal helpers used only within their own `cli/<cmd>.rs` | private (no qualifier) | Default |
| Helpers in `cli/output.rs`, `cli/exit.rs`, `cli/paths.rs` that are called from sibling cli/ modules but NOT from `main.rs` | `pub(crate)` | Visible to all library modules; invisible outside the library |
| Helpers used directly by `main.rs` (e.g. `exit_protocol_error` if `main` calls it) | `pub` | Same rule as runners |

Workers determine "is this called from main.rs?" by grepping the existing main.rs (before extraction) for callers of the function being extracted.

### Naming convention

Each per-command module exposes one or more runner functions named `run`, `run_<subcommand>`, or a small number of `print_*` / `exit_*` helpers. The match arm in `main()` becomes:

```rust
Command::Serve(args) => cli::serve::run(args).await?,
Command::Search(args) => cli::memory::run_search(args).await?,
Command::Dream(args) => match args.command {
    DreamCommand::Now(a) => cli::dream::run_now(a).await?,
    DreamCommand::Status(a) => cli::dream::run_status(a).await?,
    // ...
},
```

(Exact signatures depend on whether the original was `async`, what error type it returned, and whether it `exit!()`ed mid-flight. Workers preserve these verbatim.)

### Invariants (must not violate)

1. **Behavior preservation**: every CLI command produces identical stdout/stderr/exit-code/side-effects before and after the refactor for every existing test input. Verified by the workspace test gate (~1228 tests) passing post-refactor with no `expect()` changes.
2. **Visibility model per §"Visibility model" above**: runner functions called from `main.rs` are `pub`. Cross-cli-module helpers are `pub(crate)`. Module-local helpers are private.
3. **No new dependencies**: refactor uses only crates already in `Cargo.toml`. No `tracing` calls added or removed. No `anyhow::Context` added or removed. (Workers may rearrange imports as needed but not add/remove dependencies.)
4. **Idiomatic Rust**: each new module passes `cargo clippy -p memoryd --all-targets -- -D warnings` with no new `#[allow(...)]` annotations beyond the temporary `#[allow(dead_code)]` used during Phase 1 (removed in Phase 2).
5. **No `unwrap()` in production code paths**: if the original `main.rs` had `unwrap()` calls, they move verbatim — this is not the refactor's job to fix. Flag them in the review doc as follow-up candidates if found.
6. **No file deletion until orchestrator-controlled Phase 2**: workers add new files; the original `cli.rs` (now `cli/mod.rs` after pre-flight) and `main.rs` are reduced only by the orchestrator in Phase 2.
7. **Worker files are leaf modules during Phase 1**: each worker writes a module that compiles standalone with `#[allow(dead_code)]` at the module level. No `use` of new modules from `main.rs` or from sibling new modules during Phase 1 — those wires are connected by the orchestrator in Phase 2.

### Why no worktrees

Standard Codex-pattern worktrees would force ~15 sequential merges with identical `main.rs` conflicts in every merge. This refactor's parallelism gain comes from the fact that **workers each write their own new file and never touch `main.rs` or `cli/mod.rs`**. Phase 1 workers share the working tree without collision risk because they touch disjoint files. The orchestrator does the `main.rs` collapse in one focused pass in Phase 2.

This is a deviation from CLAUDE.md "Repository state strategy" which mandates worktrees for parallel work. Justified because: the parallelism here is across read-then-create, not across read-edit-of-shared-files; each worker's gate is a per-file `cargo check` which serializes naturally on Cargo's lockfile without correctness risk.

## Phase 0 — Orchestrator pre-flight (sequential, ~15 min)

### 0.0 — Branch and SHA capture

Refactor lands on a branch, not directly on `main`. This makes Phase 0 reversible and matches the project's "main is fast-forward only" repository state strategy.

```bash
git switch -c refactor/main-rs-split
PRE_REFACTOR_SHA=$(git rev-parse HEAD)
echo "PRE_REFACTOR_SHA=$PRE_REFACTOR_SHA" > /tmp/refactor-sha.txt
```

Capture the SHA before any change. Phase 5's codex prompt uses it. Rollback at any phase: `git switch main && git branch -D refactor/main-rs-split`.

Each step below has a gate. Phase 0 is sequential.

### 0.1 — Convert `cli.rs` to `cli/mod.rs`

1. `mkdir crates/memoryd/src/cli/`
2. `git mv crates/memoryd/src/cli.rs crates/memoryd/src/cli/mod.rs`
3. Gate: `cargo check -p memoryd` (must pass — `cli` becomes a directory module, all existing imports resolve unchanged because `memoryd::cli::*` is still the same path).
4. `cargo clippy -p memoryd --all-targets -- -D warnings` (must pass — no behavioral or visibility change).

### 0.2 — Add module declarations + empty stubs

Stub files contain a single placeholder comment line. Workers DELETE the placeholder as their first edit before writing real content (saves a `cargo fmt` noise pass).


In `crates/memoryd/src/cli/mod.rs`, append (under existing content):

```rust
// Per-command runners — populated by the 2026-05-28 main.rs refactor.
#[allow(dead_code)]
pub(crate) mod serve;
#[allow(dead_code)]
pub(crate) mod daemon;
#[allow(dead_code)]
pub(crate) mod memory;
#[allow(dead_code)]
pub(crate) mod source;
#[allow(dead_code)]
pub(crate) mod review;
#[allow(dead_code)]
pub(crate) mod recall;
#[allow(dead_code)]
pub(crate) mod dream;
#[allow(dead_code)]
pub(crate) mod peer;
#[allow(dead_code)]
pub(crate) mod ui;
#[allow(dead_code)]
pub(crate) mod web;
#[allow(dead_code)]
pub(crate) mod reality_check;
#[allow(dead_code)]
pub(crate) mod privacy;
#[allow(dead_code)]
pub(crate) mod import;
#[allow(dead_code)]
pub(crate) mod init;
#[allow(dead_code)]
pub(crate) mod output;
#[allow(dead_code)]
pub(crate) mod exit;
#[allow(dead_code)]
pub(crate) mod paths;
```

Create stub files at each path:

```rust
// crates/memoryd/src/cli/<name>.rs
// Populated by the 2026-05-28 main.rs refactor (T<N>).
```

Gate: `cargo check -p memoryd && cargo clippy -p memoryd --all-targets -- -D warnings` (the per-module `#[allow(dead_code)]` keeps empty stubs from warning; orchestrator removes it in Phase 2.3 once the modules are wired into the dispatch).

Commit message: `refactor(memoryd): scaffold cli/ submodules for main.rs split`

## Phase 1 — Parallel extractions (18 worker tasks, fanned out in waves)

Each worker is a native Claude Code subagent (`claude` agent type, model selected per task) with the `clean-code` and `rust-engineer` skills invoked at the top of its prompt. Workers operate on the shared working tree. Each worker's owned file is exactly one of the stub files created in Phase 0; no worker may touch any other file.

### Worker contract (every worker)

**Before writing anything**, the worker MUST:
1. Read `crates/memoryd/src/main.rs` at the line ranges specified in its task spec.
2. Read `crates/memoryd/src/cli/mod.rs` to understand the args types.
3. Invoke the `clean-code` skill.
4. Invoke the `rust-engineer` skill.

**Then** the worker:
5. Writes the new module file in full — imports, types if needed, function bodies copied verbatim from `main.rs`. Function visibility: `pub(crate)` for the runner(s) that `main.rs` will call; private for module-internal helpers.
6. Adds `tracing`, `anyhow`, `clap` etc. imports as needed for the moved code.
7. Does **not** delete, comment, or modify anything in `main.rs` or `cli/mod.rs`.
8. Does **not** add `#[allow(...)]` attributes beyond what's necessary for the code to compile under the existing crate-level lint config; the module-level `#[allow(dead_code)]` is already declared in `mod.rs` and is enough for Phase 1.

**Per-worker gate** (run by worker before reporting done):
```bash
cargo check -p memoryd
cargo clippy -p memoryd --lib --all-features -- -D warnings
```
(Library check, not binary — `main.rs` still references the originals so the binary clippy lint won't be informative until Phase 2. The library check verifies the new module compiles. Library check is sufficient: extracted modules live under `cli/` in the library crate, and any unresolved symbol — function, struct, or trait impl — will fail the lib check because main.rs is a binary and its private symbols are invisible to lib modules.)

**Pre-report grep verification** (worker runs before reporting done):
1. For every function/struct/impl named in the task spec's extract list: `grep -c "^pub(crate) fn <name>\|^fn <name>\|^pub(crate) async fn <name>\|^async fn <name>\|^struct <name>\|^impl .* for <name>" crates/memoryd/src/cli/<owned_file>.rs` — every name returns ≥ 1.
2. Workers must NOT add `use super::<sibling>::*` references to other Phase 1 modules unless they were created in Wave 1 (output, exit, paths). Cross-references to other Wave 2 sibling modules are forbidden in Phase 1 — orchestrator wires those in Phase 2 if needed. If a worker finds their extracted code needs a sibling Wave 2 module's helper, they bring a local copy of the helper as a private fn and flag it in their report so orchestrator can dedupe in Phase 2.
3. No `use crate::cli::<sibling_command>::*` references to other Wave 2 modules. (Wave 1 helpers — `output`, `exit`, `paths` — ARE allowed to be referenced from Wave 2 because Wave 1 is fully shipped before Wave 2 starts.)

**Worker MUST report**:
- Path of file written
- Functions/items extracted (names + signatures)
- Any cross-module references introduced (path + symbol)
- Any concerns or surprises (e.g. "this function called a private helper in main.rs that needs to either move with me or be promoted to a shared module")
- The exact `cargo check -p memoryd` output (last 3 lines).

### Wave 1 — Helper modules (3 workers, parallel, sonnet model)

These extract pure helper functions with no inter-helper dependencies. Run all three in one Agent batch.

**T01 — `cli/output.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/output.rs`
- **Source ranges in main.rs:** L1127–L1158, L1246–L1259 (approximate — worker re-checks line numbers at start)
- **Extract:** `print_response`, `governance_write_response_promoted_id`, `write_note_response_id`, `maybe_emit_first_write_banner`.
- **Visibility:** all four `pub(crate)` (called only by sibling cli/ modules, not by main.rs).
- **Does NOT extract:** `parse_meta` — input parsing for memory writes, lives in T06 `cli/memory.rs` as a private helper. `confirmed_on_stdin` — peer-prompt input, lives in T11 `cli/peer.rs` as a private helper.
- **Why `maybe_emit_first_write_banner` lives here, not in `import.rs`:** the function is called from `Command::Write` (L164) and `Command::WriteNote` (L145) — both in T06 (`memory.rs`) territory. Parking it in `import.rs` would make `import` an artificial dependency of `memory`.

**T02 — `cli/exit.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/exit.rs`
- **Source ranges in main.rs:** L1260–L1265, L1429–L1454
- **Extract:** `exit_protocol_error`, `exit_recall_unavailable`, `exit_dream_error`, `recall_exit_code`, `doctor_cli_exit_code`.
- **Visibility:** all five `pub(crate)`.
- **Does NOT extract:** `confirmed_on_stdin` (peer-prompt input, lives in T11 `cli/peer.rs` as a private helper).

**T03 — `cli/paths.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/paths.rs`
- **Source ranges in main.rs:** L642–L648
- **Extract:** `resolve_socket_arg`, `resolve_socket_with_runtime`.
- **Visibility:** both `pub(crate)`.
- **Does NOT extract:** `recall_socket_path` (lives in T09 `cli/recall.rs` as a private helper — used only by recall arms).

### Inter-wave gate (orchestrator)

After Wave 1 completes and before Wave 2 fans out, orchestrator runs:
```bash
cargo check -p memoryd
cargo clippy -p memoryd --lib --all-features -- -D warnings
cargo test -p memoryd --tests --no-run
```
The `--no-run` test build ensures all integration tests still compile against the new (still-`#[allow(dead_code)]`) module surface — catches any case where a Wave 1 worker accidentally broke a public path that a test relies on.

### Wave 2 — Per-command runners (14 workers, parallel, model selected per complexity)

All run in one Agent batch. Worker models:
- **opus** for tasks touching multi-function clusters, async-coordination state machines, or crypto/signal handling: T04 (serve), T10 (dream), T17 (init).
- **sonnet** for the rest.

**T04 — `cli/serve.rs` (opus)**
- **Owned file:** `crates/memoryd/src/cli/serve.rs`
- **Source ranges in main.rs:** L36–L77 (the `Command::Serve(args) => { ... }` arm body)
- **Extract:** the inline serve logic as `pub(crate) async fn run(args: ServeArgs) -> anyhow::Result<()>`.
- **Care needed:** privacy enforcement install, signal handler wiring (`install_termination_handler` lives at L1468 and stays with serve — move it to this module too, or to a sibling module if used elsewhere; worker reports which). Heavy use of `tracing`, `Substrate::init`/`Substrate::open`, `server::run`.
- **Special note:** if `install_termination_handler` is called from anywhere other than the Serve arm, do NOT move it; leave it in main.rs (orchestrator deals with it in Phase 2).

**T05 — `cli/daemon.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/daemon.rs`
- **Source ranges in main.rs:** L78–L112 (Mcp, Status, Doctor arms) + L674–L702 (`auto_start_daemon`)
- **Extract:** `pub(crate) async fn run_mcp(args)`, `run_status(args)`, `run_doctor(args)`, and `auto_start_daemon` as a private helper (used only by Mcp arm — verified by grep, plan-reviewer follow-up).

**T06 — `cli/memory.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/memory.rs`
- **Source ranges in main.rs:** L113–L200 (Search, Get, WriteNote, Write, Supersede, Forget arms) + L1460–L1466 (`parse_meta`)
- **Extract:** `run_search`, `run_get`, `run_write`, `run_write_note`, `run_supersede`, `run_forget`, and `parse_meta` as a private helper (called by write/write_note/supersede).
- **Visibility:** runners `pub`, `parse_meta` private.

**T07 — `cli/source.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/source.rs`
- **Source ranges in main.rs:** L167–L175 (Source arm) + L650–L672 (`source_capture_payload`)
- **Extract:** `run_capture` (or whatever the source subcommand needs) + `source_capture_payload` as a private helper.

**T08 — `cli/review.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/review.rs`
- **Source ranges in main.rs:** L201–L232
- **Extract:** one function per `ReviewCommand` subcommand variant.

**T09 — `cli/recall.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/recall.rs`
- **Source ranges in main.rs:** L233–L268, L1267–L1293, L1456–L1458
- **Extract:** runner(s) for `RecallCommand` subcommands, `print_recall_startup`, `print_recall_delta`, and `recall_socket_path` (private helper — used only by recall).
- **Visibility:** runner(s) `pub`, internal helpers private.

**T10 — `cli/dream.rs` (opus)**
- **Owned file:** `crates/memoryd/src/cli/dream.rs`
- **Source ranges in main.rs:** L269–L312, L704–L762, L979–L1126 (DreamCommand arm, `run_manual_dream`, `run_scheduled_dream`, `execute_dream_run`, `dream_run_error_to_lease_error`, `run_dream_cleanup`, `parse_cleanup_now`, `dream_report_failed`)
- **Extract:** all of the above as a cohesive module. Runner(s) `pub(crate)`; everything else private. **Also extract the `DreamRunInvocation` struct** defined inside the L979–L1126 range (the struct is module-private, passed between `run_scheduled_dream` and `execute_dream_run` — it moves with them).
- **Care needed:** biggest single extraction. Async coordination, lease error mapping, datetime parsing. Care with `chrono::{DateTime, Utc}` and `memoryd::dream::lease::LeaseError` imports. **Workers extracting structs as well as functions: re-grep your owned source range for `struct ` and `enum ` patterns before reporting done — make sure all type definitions in your range are present in your new module.**

**T11 — `cli/peer.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/peer.rs`
- **Source ranges in main.rs:** L313–L339, L1295–L1422, L1423–L1428 (`confirmed_on_stdin`)
- **Extract:** runner for `PeerCommand` subcommands, `run_peer_release_lock`, `print_peer_status`, `print_peer_activity`, and `confirmed_on_stdin` as a private helper.
- **Visibility:** runners `pub`, `confirmed_on_stdin` private.

**T12 — `cli/ui.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/ui.rs`
- **Source ranges in main.rs:** L340–L342, L764–L782
- **Extract:** `run` calling into `run_tui` + `run_tui` itself.
- **Stops at L782** (just before `WebOperation` enum at L784) so the enum lands cleanly in T13's range.

**T13 — `cli/web.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/web.rs`
- **Source ranges in main.rs:** L343–L368, L784–L869
- **Extract:** the `WebOperation` enum at L784–L788 (module-private, used only by web internals), runner(s) for `WebCommand` subcommands, `print_web_response`, `print_web_status`, `web_protocol_exit_code`.
- **Why `WebOperation` lives here, not in `ui.rs`:** the enum is consumed only by `print_web_response(..., WebOperation)` in this module. Codex review caught that the original T12 range L764-L789 swept it up incorrectly.

**T14 — `cli/reality_check.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/reality_check.rs`
- **Source ranges in main.rs:** L369–L412, L870–L977
- **Extract:** runner(s) for `RealityCheckCommand` subcommands + `print_reality_check_run` + `print_reality_check_summary` + `print_reality_check_skip` + `print_reality_check_snooze` + `reality_check_error_exit_code`.

**T15 — `cli/privacy.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/privacy.rs`
- **Source ranges in main.rs:** L413–L523, L625–L640
- **Extract:** runner(s) for `PrivacyCommand`, `PrivacyFilterCommand`, `DeviceCommand` subcommands + `record_device_keys_rotated_event`.
- **Care needed:** key rotation flow — care with `KeyRotation`, `FileKeyProvider`, atomic event recording.

**T16 — `cli/import.rs` (sonnet)**
- **Owned file:** `crates/memoryd/src/cli/import.rs`
- **Source ranges in main.rs:** L524–L526 (`Command::Import` arm) + L1160–L1245 (`run_import_command`) + L1231–L1244 (`DefaultSkipPrompts` private struct + its `PromptBackend` impl)
- **Extract:** `pub(crate) async fn run(args: ImportArgs) -> anyhow::Result<()>`, the `DefaultSkipPrompts` struct, and its `impl memoryd::import::project_map::PromptBackend for DefaultSkipPrompts` block. Both the struct and the impl are import-only consumers.
- **Does NOT extract:** `maybe_emit_first_write_banner` (lives in T01 `output.rs` because memory writes consume it).

**T17 — `cli/init.rs` (opus)**
- **Owned file:** `crates/memoryd/src/cli/init.rs`
- **Source ranges in main.rs:** L527–L533 (`Command::Init` arm) + L534–L623 (`run_init_command`)
- **Extract:** `pub(crate) async fn run(args: InitArgs) -> anyhow::Result<()>`.
- **Does NOT extract:** `auto_start_daemon` (lives in T05 `daemon.rs` — only Mcp arm calls it, verified by grep).
- **Care needed:** the wizard touches the substrate, calls `dialoguer`, and may shell out to the install script. Preserve all `tracing` calls, all stdout/stderr strings, all exit-code paths verbatim.

### Worker Agent invocation pattern (orchestrator-side)

Workers fanned out in two Agent batches: Wave 1 (3 calls in one message), then Wave 2 (15 calls in one message after Wave 1 completes). Each Agent invocation:

```
Agent({
  description: "<3-5 word task name>",
  subagent_type: "claude",
  model: "<sonnet|opus>",
  prompt: "<full briefing>"
})
```

The prompt includes:
- The worker contract (above, verbatim)
- The owned file path
- The source line ranges in main.rs
- The list of functions to extract
- The exact per-worker gate command
- Pre-flight: invoke `clean-code` and `rust-engineer` skills
- Explicit "do NOT touch main.rs or cli/mod.rs" warning
- "Report back the extracted item names and the last 3 lines of `cargo check` output"

### What happens after Wave 2

State after Phase 1: 18 new files exist under `cli/` (filled in by workers). `main.rs` is unchanged (still 1489 lines). `cli/mod.rs` has the original args types + `#[allow(dead_code)] pub(crate) mod ...` declarations. Workspace builds (because the new modules are `#[allow(dead_code)]` leaf modules, and main.rs still defines the originals).

The state is intentionally "duplicated but compiles." This is the handoff point to Phase 2.

## Phase 2 — Orchestrator `main.rs` collapse (sequential, ~45 min)

I (orchestrator) do this directly. No subagents — the work needs me to hold the full picture of what every new module exposes and how the match arms should call them.

### 2.1 — Read every new module to map the API

Sequential `Read` of all 18 new `cli/*.rs` files. Build a mental (or scratchpad) table: `{Command variant → call site}`.

### 2.2 — Rewrite main.rs in batches

To keep bisection cheap if the new dispatch breaks, the match collapse is batched: 4 arms per batch, `cargo check -p memoryd` between each batch. If a batch fails, only that batch's 4 arms are suspect.

Batch ordering (no dependency between batches; ordering is by failure-blast-radius — most-touched commands first):

- **Batch 1:** `Serve`, `Mcp`, `Status`, `Doctor` (daemon lifecycle — broadest blast radius if broken)
- **Batch 2:** `Search`, `Get`, `Write`, `WriteNote` (memory reads + writes — exercised by most tests)
- **Batch 3:** `Supersede`, `Forget`, `Source`, `Review`
- **Batch 4:** `Recall`, `Dream`, `Peer`
- **Batch 5:** `Ui`, `Web`, `RealityCheck`
- **Batch 6:** `Privacy`, `PrivacyFilter`, `Device`, `Import`, `Init` (5 in this batch — these are mutually orthogonal so a single-batch collapse is fine)

For each batch:
1. Replace the listed arms' bodies with the one-call shape: `Command::X(a) => cli::<module>::run(a).await?,` (or the variant-matched shape for subcommanded commands).
2. Delete the matching helper function bodies from below the match.
3. Run `cargo check -p memoryd` — must pass.
4. Run `cargo clippy -p memoryd --all-targets -- -D warnings` — must pass.

After all 6 batches:
1. Remove all `use` statements at the top of `main.rs` that supported now-extracted code (e.g. `dialoguer`, `KeyRotation`, `Substrate`, `InitOptions`, etc.).
2. Keep only: imports for `clap` + `tokio` + `memoryd::cli` + `memoryd::server`, `main()` itself, and `install_termination_handler` if it's still called from `main` (verify; if it moved into `cli/serve.rs`, delete the original).

### 2.3 — Remove `#[allow(dead_code)]` from `cli/mod.rs`

Strip the `#[allow(dead_code)]` line above each `pub(crate) mod ...;` declaration. The modules are now wired into the dispatch and are no longer dead.

### 2.4 — Workspace gate (the truth)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --no-fail-fast
bash scripts/check-dogfood.sh
```

All four must pass. If `cargo test` regresses on any test, the refactor is broken and Phase 2 is reverted (`git checkout main -- crates/memoryd/src/main.rs crates/memoryd/src/cli/mod.rs`); investigate which extraction silently changed behavior; re-do Phase 2 with the fix.

### 2.5 — Verify size targets

- `wc -l crates/memoryd/src/main.rs` — must be ≤ 200.
- `wc -l crates/memoryd/src/cli/*.rs` — each module ≤ 400 LOC. `dream.rs` may be at the upper end; anything over 400 means the worker packed too much in and grouping gets revisited.

### 2.6 — Coverage matrix + eyeball-diff for under-tested arms

The workspace test suite (1307 tests at HEAD per the 2026-05-28 pre-refactor gate) covers most CLI arms via the daemon-scaffold integration tests, but several arms have **no binary-level coverage of stdout/stderr/exit paths**. Codex review (Phase pre-implementation) corrected an earlier under-tested list — `Device::RotateKeys` does have a real binary-eval path; `Web`, `RealityCheck`, `Import`, and `Init` do not.

The orchestrator-side eyeball-diff list (Phase 2.6):

| Arm / command | Why on the list |
|---|---|
| `Command::Source` (all subcommands) | no end-to-end CLI test |
| `Command::Review` (approve/reject) | no end-to-end CLI test |
| `Command::Privacy` (all subcommands) | no end-to-end CLI test |
| `Command::PrivacyFilter` (all subcommands) | no end-to-end CLI test |
| `Command::Web` (all subcommands) | no binary-level test of stdout/stderr/exit paths |
| `Command::RealityCheck` (all subcommands) | no binary-level test |
| `Command::Import` | `import_end_to_end.rs` tests the pipeline, NOT `run_import_command`'s stdout/stderr/exit |
| `Command::Init` | no binary-level test of wizard prompts/output |
| `Command::Ui` subprocess exit behavior | exec'd subprocess, no test of args passed |

For each entry, orchestrator:
1. Runs `git diff $PRE_REFACTOR_SHA..HEAD -- crates/memoryd/src/cli/<module>.rs` and reads the diff side-by-side with the corresponding pre-refactor `main.rs` range
2. Verifies: identical control flow, identical log strings, identical exit codes, identical println/eprintln output, identical error messages, identical signal handler ordering

Findings recorded in `docs/reviews/2026-05-28-main-rs-refactor-review.md` under a new "Phase 2.6 eyeball-diff" section.

### 2.7 — Before/after golden smoke harness

For the arms with weak binary-level coverage, capture a small set of "golden" command invocations and their stdout/stderr/exit-code outputs BEFORE the refactor lands (i.e. before Phase 0.1), then re-run AFTER Phase 2 and compare bit-exact.

Captured as an ephemeral script at `/tmp/refactor-golden-smoke.sh`:

```bash
# pre-flight: capture before Phase 0
git switch main
cargo build -p memoryd --bin memoryd
for cmd in "memoryd --help" "memoryd import --help" "memoryd init --help" \
           "memoryd web enable --help" "memoryd reality-check run --help" \
           "memoryd source capture --help" "memoryd review approve --help" \
           "memoryd privacy --help" "memoryd privacy-filter --help"; do
  echo "=== $cmd ==="
  eval "./target/debug/$cmd" 2>&1
  echo "exit=$?"
done > /tmp/refactor-golden-before.txt

# post-Phase-2: re-run on the refactor branch
git switch refactor/main-rs-split
cargo build -p memoryd --bin memoryd
# (same loop, output → /tmp/refactor-golden-after.txt)
diff /tmp/refactor-golden-before.txt /tmp/refactor-golden-after.txt   # MUST be empty
```

Any non-empty diff is a refactor regression and blocks Phase 3. The script + diff output are uncommitted ephemera; results are summarized in the review doc.

Commit message: `refactor(memoryd): split main.rs into cli/ per-command modules`

## Phase 3 — Code review fan-out (parallel sonnet subagents)

4 parallel sonnet reviewers, each reading ~4 new `cli/*.rs` files plus the new `main.rs`. Each loads `clean-code` and `rust-engineer` skills. Reviewers report findings; I consolidate.

### Reviewer assignments

- **R01 (sonnet):** main.rs + serve.rs + init.rs + import.rs
- **R02 (sonnet):** dream.rs + recall.rs + memory.rs + peer.rs
- **R03 (sonnet):** web.rs + reality_check.rs + privacy.rs + daemon.rs
- **R04 (sonnet):** output.rs + exit.rs + paths.rs + source.rs + review.rs + ui.rs

### Reviewer prompt (template)

Each reviewer receives:
- The file paths to read
- Instructions to load `clean-code` and `rust-engineer` skills first
- The behavior-preservation invariant
- Explicit asks:
  - Does any extracted function violate single-responsibility or stepdown-rule?
  - Are there `unwrap()` / `expect()` calls without messages that should be `?` + `anyhow::Context`?
  - Are there leftover defensive try/catch (`anyhow::Context` chains that wrap an already-typed error needlessly)?
  - Are imports tight (no unused, no overly broad `use foo::*`)?
  - Are runner function signatures consistent across modules (`async fn run(args: Args) -> anyhow::Result<()>`)?
  - Any obvious deduplication opportunity across the new modules?
  - Any place where a module's name doesn't match what's actually inside it?
- "Report findings as a Markdown list. For each finding: file:line, severity (blocker/risk/nit), description, suggested fix. No prose intro."

### Orchestrator consolidation

I read all four reports, dedupe overlapping findings, and write `docs/reviews/2026-05-28-main-rs-refactor-review.md` with sections **Blockers**, **Risks**, **Nits**.

## Phase 4 — Orchestrator fixes review findings

I apply every Blocker and every Risk finding directly. Nits get applied if they fit the refactor's scope (formatting, naming, dead imports); behavior changes are deferred to follow-up plans.

After fixes, re-run the workspace gate:
```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --no-fail-fast
```

Commit message: `refactor(memoryd): address Phase 3 review findings on cli/ split`

## Phase 5 — Delegate codex review

Per CLAUDE.md "Second opinions & cross-harness delegation", use `delegate codex safe` for an independent read.

### Codex prompt (template, written to a temp file and passed via `--prompt-file`)

The prompt file includes (in order):

1. **One-line summary**: behavior-preserving extraction of `main.rs` into per-command modules.
2. **Pre-refactor SHA** (orchestrator captures and substitutes): the SHA of `main` just before Phase 0.1 ran. Stored in the prompt as `PRE_REFACTOR_SHA=<sha>`.
3. **Diff command** for codex to run first: `git -C /Users/treygoff/Code/agent-memory diff $PRE_REFACTOR_SHA..HEAD -- crates/memoryd/src/main.rs crates/memoryd/src/cli/`
4. **Plan invariants verbatim** (copy from §"Invariants (must not violate)" of this plan file): the seven numbered behavior-preservation guarantees workers were contracted to.
5. **Owned-files map**: a list of each new module file and which Command(s) it owns. Helps codex orient on which module to look at for a given concern.
6. **Review axes**:
   - **Behavior preservation**: identify any case where an extracted function's behavior subtly differs from the original (different exit code path, swallowed error, changed log line ordering, dropped `?` chain, etc.).
   - **Missed extractions**: identify functions still in `main.rs` that should have moved to one of the new modules.
   - **Module boundaries**: identify cases where the wrong things were grouped together (e.g. a helper used only by one command living in a shared helpers module).
   - **Idiomatic Rust**: clippy-flavored findings the workspace clippy may have missed (e.g. `Box<dyn Error>` instead of `anyhow::Error`, missed `?`-chains, redundant `clone()`s introduced during the move).
   - **Cross-module duplication**: a worker brought a local copy of a helper instead of using a shared Wave 1 helper; flag for dedup.
7. **Severity classification**:
   - `BLOCKER`: behavior changed, build breaks, or test fails.
   - `RISK`: subtle correctness concern that might escape tests.
   - `NIT`: style/idiom finding.
   - `PRE-EXISTING`: bug or smell that existed before the refactor (moved verbatim; out of scope to fix here, logged for follow-up).
8. **Output format**: Markdown list, `file:line | severity | description | suggested fix`. No prose intro.

### Invocation

```bash
delegate codex safe --prompt-file /tmp/codex-refactor-review-prompt.md
delegate runs --recent --harness codex --limit 1   # find the run alias
delegate run-output <alias> > /tmp/codex-refactor-review.md   # rendered output, not raw JSONL
```

Orchestrator reads `/tmp/codex-refactor-review.md` directly (rendered output is sufficient; the `--raw` JSONL transcript is only useful for debugging the delegate harness itself).

## Phase 6 — Orchestrator fixes codex findings + final gate

I apply every Codex `BLOCKER` and `RISK` finding. `PRE-EXISTING` findings get logged in `docs/reviews/2026-05-28-main-rs-refactor-review.md` under a new "Follow-up candidates (pre-existing, deferred)" section but not fixed in this refactor.

### Final gate

```bash
bash scripts/check-dogfood.sh
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets --all-features -- -D warnings
cd crates/memoryd-web/frontend && pnpm typecheck && pnpm lint && pnpm test
cd ../../..
wc -l crates/memoryd/src/main.rs              # ≤ 200
wc -l crates/memoryd/src/cli/*.rs             # each ≤ 400 (dream may be larger; flag if so)
```

All gates must pass. Commit message: `refactor(memoryd): apply Codex review fixes on cli/ split`

## Acceptance signals (plan-level)

This refactor is complete when:

1. `crates/memoryd/src/main.rs` is ≤ 200 LOC and contains only: imports, `main()` (parsing + dispatch match + signal-handler wiring), and the `install_termination_handler` if it's not part of `cli::serve`.
2. Every command's runner logic lives in `crates/memoryd/src/cli/<command>.rs`. The dispatch match in `main()` is exactly one line per arm (or a nested `match` for subcommands).
3. `cargo test --workspace --no-fail-fast` passes with the same test count as pre-refactor (~1228).
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes clean.
5. `bash scripts/check-dogfood.sh` passes.
6. Frontend gate (typecheck + lint + vitest) passes.
7. `docs/reviews/2026-05-28-main-rs-refactor-review.md` exists with all Phase 3 + Phase 5 findings consolidated.
8. Codex review found no `BLOCKER` findings unaddressed.

## Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Worker calls a private helper still in main.rs and the helper isn't on any extract list | Medium | Worker contract requires reporting any unrecognized callee. Inter-wave gate (`cargo check -p memoryd && cargo test --no-run`) catches dangling references after each wave. |
| `install_termination_handler` (L1468) is called from multiple arms | Low | T04 (serve) worker checks call sites; if serve-only it moves into `cli/serve.rs`, else stays in main.rs. |
| Match arm dispatch shape mismatches because some arms `?` the result and some `exit_with(...)` (`-> !`) | Medium | Workers report the runner signature (Result vs. `!`). Orchestrator handles both shapes in the batched rewrite — some arms become `=> cli::x::run(a).await?,`, some become `=> cli::x::run(a).await` without `?` if the runner diverges. |
| Workers add tracing/log line "improvements" mid-extraction | Medium | Worker contract: "no behavioral changes including log line content/level/order — strings move verbatim." Phase 3 reviewers asked to spot-check strings against the pre-refactor source. |
| Workers load `clean-code` / `rust-engineer` and start refactoring mid-extraction | Medium | Worker contract: skills are for judgment on logical boundaries, not for rewriting. Extract first, do not edit. |
| Spot-checked under-tested arms (Source/Review/Privacy/Device key rotation) still ship a silent extraction bug | Low | Phase 2.6 orchestrator-side eyeball-diff for those specific arms, recorded in the review doc. |
| `Cargo.toml` needs a change (e.g. a feature gate) | Very Low | No `Cargo.toml` changes; the binary still points at `src/main.rs`. |

## Per-task gate definitions (reference)

| Phase | Gate command |
|---|---|
| Phase 0 | `cargo check -p memoryd && cargo clippy -p memoryd --all-targets -- -D warnings` |
| Phase 1 (per worker) | `cargo check -p memoryd && cargo clippy -p memoryd --lib --all-features -- -D warnings` |
| Phase 2 | `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --no-fail-fast && bash scripts/check-dogfood.sh` |
| Phase 4 | Same as Phase 2 |
| Phase 6 (final) | Phase 2 gate + `cd crates/memoryd-web/frontend && pnpm typecheck && pnpm lint && pnpm test` |

## Plan revision history

- 2026-05-28 v0.1: Initial draft.
- 2026-05-28 v0.2: Plan-reviewer patches. (1) Re-assigned `auto_start_daemon` from T17 init to T05 daemon (verified by grep — only Mcp arm calls it). (2) Added `DefaultSkipPrompts` struct + impl to T16 import extract list (was implicit, now explicit). (3) Moved `maybe_emit_first_write_banner` from T16 to T01 (output.rs) — called from memory writes, not import. (4) Added pre-report grep verification + cross-module-reference rules to worker contract. (5) Added inter-wave gate (`cargo check + clippy + test --no-run`) between Wave 1 and Wave 2. (6) Phase 2 match collapse broken into 6 batches of 4 arms each with `cargo check` between. (7) Added Phase 2.6 eyeball-diff spot-check for under-tested arms (Source/Review/Privacy/Device key rotation). (8) Added `DreamRunInvocation` struct explicitly to T10 extract list. (9) Beefed up codex prompt: pre-refactor SHA, diff command, invariants verbatim, owned-files map. (10) Dropped `--raw` JSONL parsing from delegate invocation; use rendered output. (11) Risk table compressed from 11 rows to 7. (12) LOC budget unified at ≤ 400 per cli/ module. (13) Stub-placeholder deletion explicit in worker contract.
- 2026-05-28 v0.3: Codex review patches (`delegate codex safe`). (1) **BLOCKER fixed:** visibility model rewritten. `pub(crate)` items in the library are NOT visible to the binary; runners called from main.rs must be `pub`, cross-cli-module helpers `pub(crate)`, module-internal helpers private. New "Visibility model" subsection in Architecture. (2) **BLOCKER fixed:** `WebOperation` enum (L784-L788) reassigned from T12 ui (which previously claimed L764-L789) to T13 web (where it's actually used). T12 ui range truncated to L764-L782. (3) Worker count math: Wave 2 is 14 workers (not 15); total worker tasks 17 (not 18); model assignments corrected (opus = T04, T10, T17, not T16). (4) Helper-ownership refinements: `parse_meta` moves from `output.rs` to `memory.rs` (input parsing, not output); `confirmed_on_stdin` moves from `exit.rs` to `peer.rs` (peer-prompt input, not exit handling); `recall_socket_path` moves from `paths.rs` to `recall.rs` (used only by recall). (5) Phase 2.6 coverage matrix corrected: Web/RealityCheck/Import/Init added; Device::RotateKeys removed (it IS tested in `memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs`). (6) Phase 2.7 added: before/after golden smoke harness — capture stdout/stderr/exit for help-text and dry-run commands on the under-tested arms pre-refactor; diff post-refactor. (7) Phase 0.0 added: branch creation + SHA capture as the first step. Rollback now `git switch main && git branch -D refactor/main-rs-split` at any point. Refactor lands on `refactor/main-rs-split`, not directly on `main`.
