# Stream F Final API/Contract Review Rerun

Date: 2026-05-01
Reviewer: Codex API/contract rerun lane
Scope: current workspace changes against `docs/specs/stream-f-dreaming-v0.2.md` and `docs/plans/2026-04-30-stream-f-dreaming.md`, focused on public API/protocol/MCP/CLI contracts and prior blockers.
Mandatory skills applied: clean-code, tdd, rust-engineer.

## Verdict

BLOCK.

The previous S1 around public DreamNow being fixture-only is substantially closed: public CLI/daemon DreamNow now hydrates substrate/recall inputs and writes Pass 2 candidates through a production `SubstrateCandidateWriter`. `memory_observe` also accepts the exact spec-shaped argument subset with defaulted binding metadata, and core lease release/force acquisition semantics are covered.

There are still API/contract blockers: `memoryd dream disable` does not actually block manual/daemon DreamNow, known CLI overrides bypass installed/authenticated eligibility and can surface as `invalid_request` instead of `dream_unavailable`, daemon retryability flags disagree with the stable protocol table, and DreamStatus reports stale released leases as active. The CLI exit-code docs blocker is also not closed; the API doc still has prose, not numeric mappings.

## Findings by severity

### S1 - `memoryd dream disable` does not disable manual or daemon DreamNow

**Contract:** the API doc says the device-local sentinel disables scheduled and manual dreaming (`docs/api/stream-f-dreaming-api.md:107`), and the spec defines stable `dream_disabled` as a non-retryable Stream F error (`docs/specs/stream-f-dreaming-v0.2.md:755-757`).

**Evidence:**

- CLI `DreamCommand::Now` immediately calls `run_manual_dream(args)` and prints the report; there is no synced `dreams.enabled` or `dream-disabled` check at the public CLI boundary (`crates/memoryd/src/main.rs:196-201`).
- `run_manual_dream` loads config, acquires a lease, builds the dream run, selects a harness, and runs the pipeline without checking the local disable sentinel or `config.synced.dreams.enabled` (`crates/memoryd/src/main.rs:319-365`).
- Daemon `RequestPayload::DreamNow` takes the same path: it loads config, parses scope, acquires a lease, builds/selects/runs, and never checks disabled state (`crates/memoryd/src/handlers.rs:142-190`).
- Live probe: after `memoryd dream disable --runtime <tmp>`, `memoryd dream now --repo <tmp>/repo --runtime <tmp>/runtime --scope me --cli echo --json` exited `0` and wrote a success report beginning with `"status": "success"` for Pass 1.

**Impact:** the advertised local kill-switch is ineffective for the highest-risk operation: a manual/admin DreamNow can still send masked prompt text to a harness provider and write dream artifacts after the user has disabled dreaming. This is a public contract and privacy/safety control failure.

**Required fix:** check both synced `dreams.enabled` and the runtime-local `dream-disabled` sentinel before lease acquisition in CLI and daemon DreamNow. Return/stderr `dream_disabled` with the documented non-retryable semantics and add CLI/protocol tests proving no lease, prompt, journal, questions, or candidates are written while disabled.

### S2 - Known `--cli` / `cli_override` values bypass harness eligibility and surface the wrong error class

**Contract:** `cli_override` bypasses per-scope priority for one run, but harness selection still requires an installed/authenticated adapter; no eligible CLI should report `dream_unavailable` (`docs/specs/stream-f-dreaming-v0.2.md:236-239`, `docs/specs/stream-f-dreaming-v0.2.md:455-476`, `docs/specs/stream-f-dreaming-v0.2.md:750-751`).

**Evidence:**

- Priority-based selection checks `adapter.is_installed()` and `adapter.is_authenticated().await == Ok(true)` (`crates/memoryd/src/dream/registry.rs:43-50`).
- Override selection does not perform those checks. Any known adapter is returned directly (`crates/memoryd/src/dream/orchestration.rs:88-98`).
- If that adapter is unavailable, `CodexCli::complete` returns `HarnessCliError::NotInstalled` only after Pass 1 starts (`crates/memoryd/src/dream/harness.rs:317-325`). Pass 1 wraps that as `DreamError::invalid_request("pass 1 harness failed: ...")` (`crates/memoryd/src/dream/pass1.rs:29-32`).
- The daemon only maps messages prefixed with `invalid_request: dream_unavailable: ` to `dream_unavailable`; other dream errors become `invalid_request` (`crates/memoryd/src/handlers.rs:208-214`, `crates/memoryd/src/handlers.rs:2137-2139`).

**Impact:** `memoryd dream now --cli codex` or `RequestPayload::DreamNow { cli_override: Some("codex") }` can acquire a lease and then fail as `invalid_request`/CLI exit 1 when Codex is missing or unauthenticated, instead of failing selection as `dream_unavailable`/CLI exit 2. This violates the stable error contract and makes wrappers handle environmental absence as a caller bug.

**Required fix:** for override values, run the same installed/authenticated eligibility probe used by priority selection before returning the adapter. Unknown names should remain `invalid_request`; known-but-disabled/missing/unauthenticated adapters should return `dream_unavailable` before Pass 1.

### S2 - Daemon protocol `retryable` flags contradict the Stream F error table

**Contract:** Stream F's stable error table marks `dream_unavailable`, `lease_held`, and `lease_unavailable` retryable, with manual/scheduled behavior differences documented separately (`docs/specs/stream-f-dreaming-v0.2.md:750-753`).

**Evidence:**

- `HandlerError::dream_unavailable` hardcodes `retryable: false` (`crates/memoryd/src/handlers.rs:2117-2119`).
- `HandlerError::from_lease` hardcodes `retryable: false` for every lease error, including `lease_held` and `lease_unavailable` (`crates/memoryd/src/handlers.rs:2141-2142`).
- Current tests assert the wrong daemon contract: `lease_held` is expected non-retryable in `handler_contract.rs` (`crates/memoryd/tests/handler_contract.rs:202-207`), and disabled/known-unavailable harness responses are expected non-retryable (`crates/memoryd/tests/handler_contract.rs:301-305`).

**Impact:** daemon clients cannot trust `ProtocolError.retryable` for Stream F. This is not just docs drift; the serialized public protocol field is wrong relative to the v0.2 contract.

**Required fix:** encode retryability per Stream F table. At minimum: `dream_unavailable`, `lease_held`, and `lease_unavailable` should set `retryable: true`; `invalid_request`, `privacy_error`, `dream_pass_failed`, and `dream_disabled` should remain false. Update handler contract tests to match the spec.

### S2 - DreamStatus reports released/superseded leases as active

**Contract:** lease release/force semantics require stale leases to stop winning after release or force takeover (`docs/specs/stream-f-dreaming-v0.2.md:568-570`, `docs/specs/stream-f-dreaming-v0.2.md:759`). `DreamStatusReport.active_leases` is a public status surface (`docs/specs/stream-f-dreaming-v0.2.md:291`).

**Evidence:**

- The acquisition path now uses latest-record-wins semantics: `active_lease` reverses records and checks only the latest matching scope (`crates/memoryd/src/dream/lease.rs:285-288`). Release records expire at `request.now` (`crates/memoryd/src/dream/lease.rs:300-307`).
- DreamStatus does not use that same semantics. It pushes every unexpired record into `active_leases`, regardless of later release or forced takeover records for the same scope (`crates/memoryd/src/dream/status.rs:101-119`).
- Live probe: a lease file containing an unexpired foreign lease followed by a release record for the same scope produced `active_leases = [{device: "dev_foreign", scope: "me", ...}]` from `memoryd dream status --json`.

**Impact:** operators and wrappers can see a released/overridden lease as still active in the public status API even though acquisition correctly ignores it. This undermines the lease-release/force fix and can cause false operational alarms or incorrect scheduling decisions.

**Required fix:** share the latest-record lease-state reducer between acquisition and status, or add an equivalent status reducer that returns only the latest non-expired, non-release record per scope.

### S3 - CLI exit-code documentation remains incomplete

**Contract:** the spec gives concrete exit-code mappings for `memoryd dream now`: `0`, `1 invalid_request`, `2 dream_unavailable`, `3 privacy_error`, `4 dream_pass_failed`, and `5 lease_held|lease_unavailable` (`docs/specs/stream-f-dreaming-v0.2.md:338-345`). Prior review called out missing CLI exit-code docs.

**Evidence:**

- The API doc still has prose-only exit behavior and omits numeric exit codes (`docs/api/stream-f-dreaming-api.md:109-115`).
- Implementation has concrete mappings: lease errors use `LeaseError::cli_exit_code()` (`crates/memoryd/src/dream/lease.rs:68-72`) through `exit_dream_error` (`crates/memoryd/src/main.rs:439-441`), DreamNow pass failure exits 4 (`crates/memoryd/src/main.rs:197-201`), and harness-unavailable/unknown override special cases exit 2/1 (`crates/memoryd/src/main.rs:379-385`).
- Current CLI tests only pin the lease-held exit 5 path (`crates/memoryd/tests/dream_cli.rs:164-190`).

**Impact:** wrappers still cannot rely on the API docs for stable automation behavior. This is lower severity than runtime protocol defects, but it is a prior blocker that remains open.

**Required fix:** add a numeric exit-code table to `docs/api/stream-f-dreaming-api.md` and add CLI contract tests for `invalid_request`, `dream_unavailable`, `lease_held`, `lease_unavailable`, `dream_pass_failed`, and `dream_disabled` once disabled enforcement exists.

## Prior-blocker rerun status

| Prior blocker                                            | Rerun result                   | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| -------------------------------------------------------- | ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Production DreamNow uses real substrate/candidate writer | PASS with residual review risk | CLI and daemon DreamNow build via `build_dream_run` and pass `build.writer` to `DreamRunner` (`crates/memoryd/src/main.rs:337-365`, `crates/memoryd/src/handlers.rs:164-189`). `build_dream_run` loads substrate fragments, active memories, previous questions, and returns `SubstrateCandidateWriter` (`crates/memoryd/src/dream/orchestration.rs:50-76`). The writer persists candidate memories through `Substrate::write_memory` with author kind `Dreaming`, `policy_applied` from request, and grounding rehydration flag (`crates/memoryd/src/dream/orchestration.rs:122-164`, `crates/memoryd/src/dream/orchestration.rs:173-231`). Focused test passed: `cargo test -p memoryd --test handler_contract dreaming_protocol_echo_writes_pass_2_candidate_to_canonical_queue -- --nocapture`. |
| Daemon harness selection                                 | PARTIAL / BLOCKED              | Registry and priority selection exist (`crates/memoryd/src/dream/registry.rs:13-17`, `crates/memoryd/src/dream/registry.rs:43-50`), but known overrides bypass eligibility and can surface the wrong error class; see S2 above.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `memory_observe` spec-shaped arguments                   | PASS                           | MCP and protocol observe fields have defaults (`crates/memoryd/src/mcp.rs:115-129`, `crates/memoryd/src/protocol.rs:86-99`), and the manifest requires only `text` and `kind` (`crates/memoryd/src/mcp.rs:382-405`). Focused tests passed: `cargo test -p memoryd --test mcp_manifest memory_observe_request_accepts_spec_shaped_args_without_binding_fields -- --nocapture` and `cargo test -p memoryd --test mcp_forward forward_spec_shaped_memory_observe_sends_defaulted_binding_to_daemon -- --nocapture`.                                                                                                                                                                                                                                                                                    |
| Lease release/force semantics                            | PARTIAL / BLOCKED              | Acquisition semantics are improved: release records exist (`crates/memoryd/src/dream/lease.rs:171-196`) and active lease lookup uses latest-record-wins (`crates/memoryd/src/dream/lease.rs:285-288`). Focused tests passed for release and forced takeover. DreamStatus still reports stale released leases as active; see S2 above.                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| CLI exit-code docs                                       | BLOCKED                        | API doc still lacks numeric mappings; see S3 above.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

## Commands run

```text
sed -n '1,220p' /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md
sed -n '1,180p' /Users/treygoff/.agents/skill-library/clean-code/SKILL.md
sed -n '1,180p' /Users/treygoff/.agents/skill-library/tdd/SKILL.md
rg -n "Stream F|dreaming|memory_observe|DreamNow|lease release|CLI exit" /Users/treygoff/.ai-profiles/runtime/codex/personal/memories/MEMORY.md
git status --short --branch
git diff --stat
git diff --name-only
sed -n '1,260p' docs/specs/stream-f-dreaming-v0.2.md
sed -n '1,320p' docs/plans/2026-04-30-stream-f-dreaming.md
sed -n '1,240p' docs/reviews/stream-f-final-api-contract-review.md
rg -n "NoopCandidateWriter|CandidateWriter|SubstrateCandidateWriter|Governance|hydrate|DreamRunOptions|select|HarnessCliRegistry|cli_override|DreamNow|release|force|LeaseAction|active_lease|default_observe|memory_observe|exit code|Exit code|lease_held|lease_unavailable|dream_unavailable|dream_pass_failed|dream_disabled|invalid_request" crates/memoryd/src crates/memoryd/tests docs/api/stream-f-dreaming-api.md docs/specs/stream-f-dreaming-v0.2.md
nl -ba crates/memoryd/src/main.rs | sed -n '120,470p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '1,260p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '450,620p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '2110,2175p'
nl -ba crates/memoryd/src/dream/orchestration.rs | sed -n '1,520p'
nl -ba crates/memoryd/src/dream/run.rs | sed -n '1,360p'
nl -ba crates/memoryd/src/dream/pass1.rs | sed -n '1,180p'
nl -ba crates/memoryd/src/dream/pass2.rs | sed -n '1,320p'
nl -ba crates/memoryd/src/dream/lease.rs | sed -n '1,360p'
nl -ba crates/memoryd/src/dream/registry.rs | sed -n '1,320p'
nl -ba crates/memoryd/src/dream/harness.rs | sed -n '1,380p'
nl -ba crates/memoryd/src/dream/status.rs | sed -n '1,220p'
nl -ba crates/memoryd/src/mcp.rs | sed -n '1,420p'
nl -ba crates/memoryd/src/protocol.rs | sed -n '1,280p'
nl -ba docs/api/stream-f-dreaming-api.md | sed -n '1,220p'
nl -ba crates/memoryd/tests/handler_contract.rs | sed -n '1,520p'
nl -ba crates/memoryd/tests/mcp_manifest.rs | sed -n '1,240p'
nl -ba crates/memoryd/tests/mcp_forward.rs | sed -n '1,260p'
nl -ba crates/memoryd/tests/dream_cli.rs | sed -n '1,360p'
nl -ba crates/memoryd/tests/dream_lease_election.rs | sed -n '1,320p'
cargo test -p memoryd --test handler_contract dreaming_protocol_echo_writes_pass_2_candidate_to_canonical_queue -- --nocapture
cargo test -p memoryd --test mcp_manifest memory_observe_request_accepts_spec_shaped_args_without_binding_fields -- --nocapture
cargo test -p memoryd --test mcp_forward forward_spec_shaped_memory_observe_sends_defaulted_binding_to_daemon -- --nocapture
cargo test -p memoryd --test dream_lease_election explicit_release_leaves_no_active_lease_to_block_later_acquire -- --nocapture
cargo test -p memoryd --test dream_lease_election forced_takeover_makes_forced_holder_active_and_ignores_stale_prior_holder -- --nocapture
cargo test -p memoryd --test dream_cli dream_manual_lease_failure_exit_code_5_remains_covered -- --nocapture
manual probe: after `memoryd dream disable`, `memoryd dream now --cli echo --json` exited 0 and wrote a success report
manual probe: `memoryd dream status --json` reported an unexpired stale lease even when a later same-scope release record existed
```

## Residual risks

- I did not run full workspace fmt/clippy/test/bench gates; this was a targeted API/contract rerun.
- I did not verify live `claude` or `codex` auth behavior because the contract bug is visible in the override selection path before any host-specific auth state matters.
- The candidate writer now persists candidate memories, but it writes directly through Stream A substrate APIs rather than a governance-engine candidate API. That appears to satisfy the current candidate queue shape and tests, but a Stream C owner should still confirm this is the desired ownership boundary before final ship.
