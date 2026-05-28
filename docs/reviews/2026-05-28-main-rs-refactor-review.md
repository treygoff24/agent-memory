# memoryd `main.rs` refactor — Phase 3 review consolidation

**Plan:** `docs/plans/2026-05-28-memoryd-main-rs-refactor.md`
**Pre-refactor SHA:** `769fc59` • **Phase 2 head:** `15b3098`
**Reviewers:** R01 (main + serve + init + import), R02 (dream + recall + memory + peer), R03 (web + reality_check + privacy + daemon), R04 (output + exit + paths + source + review + ui). All sonnet, all loaded `clean-code` + `rust-engineer` skills.

## Bottom line

**0 real BLOCKERs.** R02 flagged two "BLOCKERs" — `dream.rs:95–103` (lease release re-evaluates `chrono::Utc::now()` instead of using the captured `now`) and `peer.rs:9–36` (`-> !` print functions followed by unreachable `Ok(())`). Both **verified pre-existing** in the pre-refactor `main.rs` at L745 and L1295–L1422 respectively — moved verbatim. Logged under follow-ups, not fixed in this refactor.

Behavior preservation oracle confirms clean: golden-smoke compare of 9 CLI invocations (`--help`, `import --help`, `init --help`, `web enable --help`, `reality-check run --help`, `source capture --help`, `review approve --help`, `privacy --help`, `privacy-filter --help`) is **byte-identical** before and after.

## Findings — to apply in Phase 4

These are introduced by the refactor or trivially in-scope. I will apply them directly.

| # | File | Description | Source |
|---|---|---|---|
| F1 | `cli/serve.rs:11` | Runner signature uses fully-qualified `crate::cli::ServeArgs` instead of importing the short name. Inconsistent with sibling runners. Add `use crate::cli::ServeArgs;` at the top; use `ServeArgs` in the signature. | R01 |
| F2 | `cli/init.rs:6` | `use crate::import::discovery::{...}` import is inside the function body; other imports are at module level. Move to top. | R01 |
| F3 | `cli/import.rs` | Multiple inline full-path references that could go through the existing `use` cluster: (a) add `use crate::cli::paths::resolve_socket_arg;` and use the short name at L52; (b) extend `use crate::cli::ImportArgs;` to `use crate::cli::{ImportArgs, ImportHarness};` and use short names in the match; (c) extend `use crate::import::project_map::{...}` to include `PromptResult, PromptedDisposition` and drop full-path qualifications from the `impl` body at L74+. | R01 |
| F4 | `cli/recall.rs:75–77` | `recall_socket_path` is a private duplicate of `cli::paths::resolve_socket_with_runtime`. Refactor-introduced divergence: the original had only one resolver; partitioning between `paths.rs` and `recall.rs` created the duplicate. Delete `recall_socket_path`, import `resolve_socket_with_runtime` from `crate::cli::paths`, and call with `(&args.socket, &args.runtime)` for the two `RecallSocketArgs` callsites. | R02 |
| F5 | `cli/web.rs:39` | `print_web_response` parameter uses fully-qualified `crate::protocol::ResponseEnvelope` while `print_web_status` uses the short `ResponseEnvelope`. Add `ResponseEnvelope` to the existing `use crate::protocol::{...}` cluster. | R03 |
| F6 | `cli/reality_check.rs:11–12` | `args.namespace.clone()` called twice across the `if args.json` branch arms. Hoist to a single binding before the `if` and move into the payload constructors. | R03 |
| F7 | `cli/exit.rs` | `doctor_cli_exit_code` (specialized) appears first; `exit_protocol_error` (most general) appears last. Reorder to stepdown: `exit_protocol_error` → `exit_recall_unavailable` → `exit_dream_error` → `recall_exit_code` → `doctor_cli_exit_code`. | R04 |

## Follow-up candidates (pre-existing, deferred — NOT fixed in this refactor)

These are bugs or smells that pre-date the refactor; they moved verbatim. Logged for a future cleanup pass.

| File | Description | Source |
|---|---|---|
| `cli/serve.rs:14` | `.map_err(anyhow::Error::msg)` erases the original error chain via `Display`. | R01 |
| `cli/init.rs:13` | `unwrap_or_else(|| PathBuf::from("./memorum"))` falls back to a relative path when `MEMORUM_REPO` is unset and `dirs::home_dir()` returns `None`. | R01 |
| `cli/init.rs:74` | `dialoguer::Confirm::new()...interact().unwrap_or(false)` silently treats tty errors as "no" with no comment explaining the choice. | R01 |
| `cli/dream.rs:95–103` | (R02 "BLOCKER" downgraded to follow-up) `release_manual_lease` is invoked with `now: chrono::Utc::now()` re-evaluated at error time, not the captured `now` from L65. Verified pre-existing at `main.rs:745` in `769fc59`. May cause lease-mismatch on disk depending on how the git layer compares timestamps. | R02 |
| `cli/dream.rs:80–92` | Double `.await` on an async block (`async { execute_dream_run(...).await }.await`) — adds wrapping allocation for no scoping benefit. | R02 |
| `cli/dream.rs:183–213` | `.map_err(|err| anyhow::anyhow!(err.to_string()))` erases the source chain three times in `execute_dream_run`. | R02 |
| `cli/peer.rs:9–36` | (R02 "BLOCKER" downgraded to follow-up) `print_peer_status` and `print_peer_activity` return `-> !` (process::exit) and are called from a `match` arm followed by unreachable `Ok(())`. Control-flow hazard if either is ever relaxed to `-> ()`. Verified pre-existing pattern in `main.rs:1295–L1422`. | R02 |
| `cli/peer.rs:39–96` | `print_peer_status` and `print_peer_activity` have structurally identical Err/Error/`other` arms — dedup opportunity. | R02 |
| `cli/memory.rs:90–95` | `parse_meta` returns `serde_json::Value::Null` for `None` — if the protocol distinguishes absent from explicit null, this is a silent semantic difference. Pre-refactor consistent. | R02 |
| `cli/web.rs:14` | `resolve_socket_arg(&args.socket)` called twice in the `Enable` branch — once for the connect arg, again inside `RequestPayload::WebEnable.socket_path`. Pre-existing. | R03 |
| `cli/web.rs:45` | Hardcoded `"http://localhost:7137"` fallback URL — magic port should be sourced from a constant. | R03 |
| `cli/web.rs:76`, `cli/reality_check.rs:59` | `.expect("web status serializes")` / `.expect("reality check serializes")` — non-actionable messages; should name the type. | R03 |
| `cli/reality_check.rs:98` | `Snoozed`/`Skipped` arriving on the `run` subcommand path silently exits 0 with a JSON blob instead of a human-readable message. | R03 |
| `cli/reality_check.rs:150–156` | `reality_check_error_exit_code` maps `"invalid_request"` and `"no_items"` to identical exit code 1. | R03 |
| `cli/privacy.rs:27–31` | `PrivacyCommand::Scan` rejects `(Some, Some)` with the same error as `(None, None)` — both hit the wildcard. | R03 |
| `cli/privacy.rs:37–38` | `to_string_lossy().as_ref()` round-trip — could pass `Cow<str>` directly. | R03 |
| `cli/daemon.rs:51` | `auto_start_daemon(repo: &PathBuf, runtime: &PathBuf, socket: &PathBuf)` — should be `&Path` per clippy `ptr_arg`. Workspace clippy currently passes (lint not firing on this call shape, possibly due to feature/profile interactions), so not blocking. | R03 |
| `cli/daemon.rs:67–76` | `auto_start_daemon` busy-polls on 100ms intervals for 10s; if the socket file is created but the daemon then crashes pre-accept, `probe_live_socket` may return `Live` and the caller proceeds. Pre-existing race window. | R03 |
| `cli/exit.rs:28-29` | `"invalid_request" => 1` and `"dream_disabled" => 1` could merge into one arm; catch-all also returns 1 so the `"dream_disabled"` arm is reachable only because it's matched before the catch-all. | R04 |
| `cli/output.rs:48–49` | `let _ = crate::first_write::emit_first_write_banner(...)` silently discards errors — documented intent, but a `tracing::warn!` on the discarded error would be cleaner. | R04 |
| `cli/ui.rs:5` | `run` is a thin one-liner wrapping `run_tui` — could collapse to one function. | R04 |
| `cli/review.rs` | Three structurally identical match arms — dedup candidate via a private `dispatch(socket, tag, payload)` helper. | R04 |
| `cli/paths.rs:4` | `socket.clone()` on `&Option<PathBuf>` — `as_deref().map(PathBuf::from).unwrap_or_else(...)` would avoid the clone. | R04 |
| `cli/output.rs:7` | `print_response(response: ResponseEnvelope)` could be `&ResponseEnvelope` to avoid move at call sites that still need the envelope. | R04 |
| `cli/source.rs:29–34` | Runtime `(url, file, mode)` validation could be encoded as a typed enum at clap parse time. | R04 |

## Phase 2.6 eyeball-diff (orchestrator)

For arms with no end-to-end binary test coverage — Source, Review subcommands, Privacy/PrivacyFilter subcommands, Web subcommands, RealityCheck subcommands, Import, Init, Ui-subprocess — I did a `git diff 769fc59..HEAD` cross-check during the R02–R04 fan-out. Workers performed line-by-line comparisons in their reports; R03 explicitly confirmed the key-rotation event ordering (`rotate_local_file()` → `record_device_keys_rotated_event()` → `println!()`) is correct.

No behavior divergence found beyond F4 (`recall_socket_path` duplicate, refactor-introduced) and F6 (`namespace.clone()` once vs. twice — cosmetic).

## Pre-existing failures unrelated to this refactor

These were present on `main` at `769fc59` and persist on the refactor branch:

1. **`dream_harness_cli.rs` Mutex-poison cascade** — 16/20 tests fail on `main`, 15/20 on the refactor branch (the difference is flake noise). One real failure (`NotFound` marker file in tempdir) poisons a `subprocess test lock` Mutex used to serialize subprocess tests; all subsequent tests fail with `PoisonError`. Out of refactor scope.
2. **`scripts/docs-command-validity.sh` "uncaveated `memoryd init`" rule** fires on `docs/getting-started.md:12,17`. The init wizard shipped in commit `769fc59` (importer + init waves 0–5); the docs-validity rule was not updated. Out of refactor scope.

## What's good

- Single-responsibility cuts cleanly: each `cli/<command>.rs` owns one command (or one cohesive family — `daemon.rs` owns Mcp+Status+Doctor, `memory.rs` owns the six memory CRUD verbs, `privacy.rs` owns Privacy+PrivacyFilter+Device).
- The `main.rs` dispatch is uniform: every arm is one line of the shape `Command::X(a) => cli::<m>::run*(a).await?,`. No inline logic leakage.
- Phase 0's `#[allow(dead_code)] pub(crate) mod` scaffolding + Phase 2's `pub mod` flip + dead-code-strip worked as designed. No mid-extraction lint workarounds linger.
- Worker contract held: every worker reported back which Wave-1 helpers they consumed and which sibling runners they avoided. No cross-Wave-2 references in the worker outputs.
- Visibility-model fix (codex review v0.3) caught the critical `pub(crate)` vs. `pub` blocker that would have failed Phase 2 — runners are `pub`, sibling-only helpers are `pub(crate)`, module-internal helpers are private.
