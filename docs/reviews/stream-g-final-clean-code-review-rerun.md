### Verdict

Approve with minor follow-ups

Approved. No final-gate clean-code blockers found in the rerun.

### Intended outcome

Stream G appears intended to provide human observability over shipped Streams A-F through the TUI, localhost web dashboard, Reality Check workflow, notifications, trust artifacts, and performance/release evidence, while preserving `memoryd` as the single protocol boundary for UI/web reads and mutations. This rerun specifically verifies that the prior final clean-code blockers were fixed without introducing new maintainability or boundary regressions.

### Executive summary

The remediation resolves the prior blockers. `memoryd-tui` now drains queued daemon calls from the production loop and tests that review/Reality Check actions reach daemon protocol payloads; Reality Check TUI rows now carry stable `memory_id` values and dispatch the selected row with the active `session_id`; `memoryd-web` no longer depends on `memory-substrate`; the boundary script now rejects that dependency edge; the canonical Stream G benchmark baseline exists and the assert gate passes against it. I found one low-severity docs consistency follow-up: README still describes Stream G shipped status as pending benchmark baseline promotion even though the baseline now exists and the bench evidence records a non-bootstrap pass. That should be cleaned up before handoff, but it does not block approval.

### Findings

[Low] [Maintainability] README still describes baseline promotion as pending after the baseline was promoted

- Evidence: `README.md:47-50` says final Stream G shipped status is pending explicit promotion of `bench/stream-g-observability-results.darwin-arm64.json` from the reviewed `.proposed` baseline. The canonical baseline file now exists at `bench/stream-g-observability-results.darwin-arm64.json`, and `docs/reviews/stream-g-bench-evidence.md:18-26` says the canonical baseline exists and was created through the explicit release/update command. The same evidence file records a final non-bootstrap assert pass at `docs/reviews/stream-g-bench-evidence.md:54-65` and `docs/reviews/stream-g-bench-evidence.md:94-98`.
- Why it matters: The prior docs blocker was about overclaiming shipped status. This remediation now slightly underclaims one prerequisite, which can confuse future agents about whether the canonical benchmark promotion still needs to happen.
- Reasoning: This is not a code-path risk and does not invalidate the Stream G implementation or validation evidence. It is a handoff/status drift issue between README and the bench artifact.
- Recommendation: Update README status language to distinguish the now-completed canonical benchmark promotion from any remaining final-review/merge/signoff state, e.g. "Stream G is implemented with canonical observability benchmark baseline promoted; final shipped status is pending final review/merge signoff" if that is the intended release state.
- Confidence: High

### Non-blocking simplifications

- `crates/memoryd-tui/src/app.rs` remains a large orchestration module spanning event loop, action staging/dispatch, modal handling, DTO fixtures, and rendering shell helpers. It is acceptable for this final gate, but a later split into `actions`, `event_loop`, `snapshot`, and `fixtures` modules would reduce review load and make future TUI behavior changes safer.
- `crates/memoryd-web/src/routes/mod.rs` still hosts a sizeable deterministic dashboard fixture next to route wiring. This is acceptable for the current dashboard contract tests, but moving fixture construction behind a `fixtures` module or test-support helper would make production route flow easier to audit as more daemon-backed routes replace placeholders.

### Test gaps

- TUI dispatch coverage is now present for the prior blockers: `crates/memoryd-tui/tests/keymap.rs:147-187` covers review action daemon dispatch and retry visibility, and `crates/memoryd-tui/tests/keymap.rs:189-247` covers selected Reality Check memory id/session dispatch.
- Web boundary and mutation coverage is present for the prior blocker: `crates/memoryd-web/tests/api_contract.rs` covers daemon-backed POST paths, `crates/memoryd-web/tests/csrf.rs` covers CSRF and localhost constraints, and `crates/memoryd-web/tests/concurrent_access.rs` covers concurrent review mutation conflict behavior.
- Remaining residual coverage risk is documented in `docs/reviews/stream-g-bench-evidence.md:108-113`: TUI and web benchmark measurements are still synthetic for render/serialization paths. This is acceptable for clean-code approval because the production scoring path is covered by `score_memories_at` tests and bench evidence, but true end-to-end UI/browser performance remains future hardening.

### Questions / uncertainties

- The prompt named `docs/specs/stream-g-observability-v1.0.md`, but the current worktree contains `docs/specs/stream-g-observability-v0.1.md`; the plan explicitly says v0.1 is the active implementation contract at `docs/plans/2026-05-01-stream-g-observability.md:11-22`. I reviewed against that local contract plus the Stream G API/dev docs.
- I did not rerun the full workspace gate because the prompt reported it had already passed and this was a review-only rerun. I did run narrow validation against the remediated boundaries and benchmark assert path.

### Positives

- The TUI remediation closes the real production loop gap: `App::dispatch_queued_daemon_calls` is called from the tick branch in `crates/memoryd-tui/src/app.rs:584-593`, failed calls remain queued for retry, and the tests now assert daemon-observable request payloads instead of only internal queue state.
- The UI/web daemon boundary is materially stronger: `memoryd-web` imports `MemoryId` from `memoryd::protocol` at `crates/memoryd-web/src/routes/mod.rs:4-5`, its `Cargo.toml` has no `memory-substrate` dependency, and `scripts/rust-boundary-check.sh:4-7` now makes that dependency edge fail fast.
- The performance remediation moved the important scoring benchmark onto the production `memoryd::reality_check::score_memories_at` path and records a promoted canonical baseline plus non-bootstrap assert pass.

Commands run during this rerun:

```bash
./scripts/rust-boundary-check.sh
# PASS

cargo test -p memoryd-tui --test keymap
# PASS: 15 tests

cargo test -p memoryd-web --test api_contract --test csrf --test concurrent_access
# PASS: api_contract 15, csrf 8, concurrent_access 1

cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
# PASS: all measurements under budget; scoring_10k_memories p95 201.809 ms <= 500 ms
```
