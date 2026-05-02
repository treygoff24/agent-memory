Verdict: Changes requested

### Intended outcome

Stream G is intended to ship human observability over the already-shipped substrate/daemon/governance/privacy/recall/dream streams: TUI, localhost web dashboard, Reality Check, notifications, trust artifact rendering, and docs/bench evidence. The architecture contract is clear that UI crates must route through `memoryd` daemon protocol and must not access substrate internals directly; Stream G should be additive and preserve the daemon as the single mutation/read boundary.

### Executive summary

The core daemon-side Reality Check, scoring, notification, trust-artifact, and web route tests I ran are green, and the `memoryd::reality_check` / `memoryd::notifications` module split is generally maintainable. However, I found two final-gate blockers in the UI boundary. First, `memoryd-tui` stages review/Reality Check actions into an in-memory queue that the production event loop never drains, so TUI mutating actions do not actually reach the daemon. Second, `memoryd-web` still depends directly on `memory-substrate` and imports `memory_substrate::MemoryId`, violating the documented UI-crate boundary even though the same type is already re-exported through `memoryd::protocol`. There is also a docs/bench consistency issue: README declares Stream G shipped while the Stream G bench evidence still says the canonical bench baseline is absent and only a `.proposed` file exists.

### Findings

[High] [Correctness] TUI mutating actions are queued but never sent to the daemon

- Evidence: `crates/memoryd-tui/src/app.rs:144-147` defines `DaemonCall::{Review, RealityCheck, ForceRefresh}` and `crates/memoryd-tui/src/app.rs:174` stores `queued_daemon_calls`; review actions are pushed after the undo window at `crates/memoryd-tui/src/app.rs:300-308`, but the production TUI loop at `crates/memoryd-tui/src/app.rs:545-572` only draws, reads terminal events, checks quit, and polls status/trust artifacts. There is no production drain/dispatch path for `queued_daemon_calls`; `rg -n "queued_daemon_calls|DaemonCall" crates/memoryd-tui/src crates/memoryd-tui/tests` found only the queue definitions, push sites, and tests asserting queued values.
- Why it matters: The API docs promise TUI review and Reality Check actions route through the daemon. As implemented, a user can press approve/reject/confirm/forget in the TUI and see local UI state change or queued test state, but no governance decision, Reality Check response, event log entry, tombstone, or state-file mutation happens.
- Reasoning: This is a boundary/design smell with a functional failure mode: command creation is separated from daemon I/O, but the second half of the abstraction was never implemented. The current tests lock in the intermediate queue instead of verifying observable daemon behavior, so they pass while the business outcome is broken.
- Recommendation: Add a single daemon-dispatch boundary in `memoryd-tui` that drains queued calls in the event loop and maps them to `memoryd::client::request` payloads. Prefer moving the mapping into `DaemonClient` methods such as `review_action`, `reality_check_respond`, and `force_refresh/status`, then test against a fake or loopback daemon protocol surface that actions actually produce the expected request and clear/fail visibly.
- Confidence: High

[High] [Correctness] TUI Reality Check actions do not carry a valid target memory id

- Evidence: `crates/memoryd-tui/src/app.rs:492-501` queues `DaemonCall::RealityCheck` using `snapshot.reality_check.items.first().map(|item| item.title.clone())` as `memory_id`; `crates/memoryd-tui/src/app.rs:1201-1205` and `crates/memoryd-tui/src/app.rs:1237-1240` show `RealityCheckPanelData` / `RealityCheckRow` do not contain a memory id at all. The panel keeps a cursor at `crates/memoryd-tui/src/panels/reality_check.rs:9-13`, but active-run action keys at `crates/memoryd-tui/src/panels/reality_check.rs:62-68` do not use it, and rendering hard-codes active progress text at `crates/memoryd-tui/src/panels/reality_check.rs:76-80`.
- Why it matters: Even after the queue-drain bug is fixed, confirm/correct/forget/not-relevant from the TUI would target the first item's title, not a `MemoryId`, and would ignore the user's selected item. This either fails daemon validation or mutates the wrong item if a title-like token accidentally parses elsewhere.
- Reasoning: The DTO intentionally rendered for the panel dropped the stable identifier needed at the mutation boundary. That is the wrong separation: UI display can hide or redact content, but action state must preserve opaque ids from the daemon response.
- Recommendation: Add `memory_id: String` or the protocol `MemoryId` to `RealityCheckRow`, populate it from `RealityCheckItem.memory_id`, use the active cursor to select the current row, and add tests that pressing each action queues/sends the selected row's id rather than the title or first item.
- Confidence: High

[Medium] [API Contract] `memoryd-web` still depends directly on the substrate crate

- Evidence: `crates/memoryd-web/Cargo.toml:21-22` depends on both `memoryd` and `memory-substrate`; `crates/memoryd-web/src/routes/mod.rs:3-6` imports `memory_substrate::MemoryId` directly. The Stream G architecture says both UI crates depend on the daemon protocol/client surface, not substrate internals.
- Why it matters: This weakens the enforced module boundary. Today it is only using a type, but it creates an allowed dependency edge from the web UI to substrate internals, making future direct reads/writes or index coupling easier and harder to catch in review.
- Reasoning: `memoryd::protocol` already re-exports `MemoryId`, so the web crate does not need `memory-substrate` to build fixture DTOs. Keeping the dependency makes the architecture doc and Cargo graph disagree.
- Recommendation: Replace the direct import with `memoryd::protocol::MemoryId`, remove the `memory-substrate` dependency from `crates/memoryd-web/Cargo.toml`, and keep/extend `./scripts/rust-boundary-check.sh` so UI crates cannot add direct substrate dependencies without an explicit exception.
- Confidence: High

[Medium] [Tests] Current TUI tests verify implementation staging, not daemon-observable behavior

- Evidence: `crates/memoryd-tui/tests/keymap.rs:91-98` asserts that the undo window produces a `DaemonCall::Review` in `queued_daemon_calls`; no source file outside `crates/memoryd-tui/src/app.rs` consumes that queue, and no TUI test asserts a daemon request is sent. The focused TUI command passed: `cargo test -p memoryd-tui --test keymap --test panel_render --test socket_unreachable --test resize`.
- Why it matters: These tests give false confidence for the exact user-facing action path Stream G needs. A regression where TUI actions never leave local state is not just untested; it is the current behavior.
- Reasoning: This is a classic test smell: the tests assert an internal staging artifact rather than the public outcome at the daemon protocol boundary.
- Recommendation: Keep small keymap tests for local navigation, but add behavior tests around a fake daemon client or Unix-socket test server that prove review and Reality Check actions issue the intended protocol payload and surface errors without silently dropping the action.
- Confidence: High

[Medium] [Reliability] Stream G is documented as shipped while its bench baseline is still bootstrap-only

- Evidence: README says "Streams A-G are shipped" at `README.md:3` and describes Stream G as shipped at `README.md:44-47`. The bench evidence says the canonical baseline `bench/stream-g-observability-results.darwin-arm64.json` is not present at `docs/reviews/stream-g-bench-evidence.md:14-20`, and lists the assert gate as still in first-run bootstrap mode at `docs/reviews/stream-g-bench-evidence.md:73-75`. The worktree currently contains only `bench/stream-g-observability-results.darwin-arm64.json.proposed` for Stream G.
- Why it matters: A final gate should not declare Stream G shipped while release/perf evidence is still explicitly awaiting human promotion. This is an operability/documentation mismatch: future agents or CI may treat Stream G as complete even though the routine assert baseline has not been established.
- Reasoning: The Task 17 plan allows first-run `.proposed` output, but the final gate needs either the canonical baseline promoted or docs that clearly state Stream G is not final-shipped until that external/manual step is complete.
- Recommendation: Either promote the reviewed Stream G bench baseline with the explicit `--write-output bench/stream-g-observability-results.darwin-arm64.json` path and rerun the assert command, or change README/final status language to "implemented pending final bench baseline promotion" until that happens.
- Confidence: High

### Non-blocking simplifications

- `crates/memoryd-tui/src/app.rs` is doing too many things for clean-code maintainability: terminal lifecycle, event loop, action staging, panel routing, rendering helpers, DTO definitions, and sample fixture construction all live in one 1,247-line file. After the blocking action-dispatch fix, split along natural seams: `event_loop`, `actions`, `layout/render_shell`, `snapshot`, and test fixtures.
- `crates/memoryd-web/src/routes/mod.rs` mixes route module wiring with a large fixture builder. Moving fixture construction under `tests/fixtures` or a `#[cfg(test)]` helper would make the production route module easier to audit for real daemon-bound data flow.
- `memoryd::reality_check::scoring` is reasonably decomposed, but it currently reopens the index from `substrate.roots().runtime.join("index.sqlite")`. That belongs in daemon/substrate-owned code, not UI, and is acceptable here; a small substrate helper for event/supersession aggregate reads would reduce knowledge of index paths in `memoryd` over time.

### Test gaps

- Missing TUI integration/contract test proving review queue actions reach `RequestPayload::ReviewApprove` / `ReviewReject` or the intended daemon review payload.
- Missing TUI integration/contract test proving Reality Check actions use the selected `RealityCheckItem.memory_id` and emit `RequestPayload::RealityCheck(Respond { ... })` with the correct session id and action.
- Missing negative-path TUI test proving daemon failures are surfaced to the user instead of leaving stale queued actions silently accumulated.
- Missing Cargo-boundary assertion that `memoryd-web` cannot depend on `memory-substrate` directly; `./scripts/rust-boundary-check.sh` passed but did not catch the current Cargo edge.
- Final bench assert coverage is incomplete until `bench/stream-g-observability-results.darwin-arm64.json` exists and `cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json` validates against it rather than taking the bootstrap path.

### Questions / uncertainties

- I did not run the full workspace gate or docs generation; this was a focused clean-code/Rust maintainability final review as requested.
- I did not inspect every Stream G/H/I dirty worktree file; the review focused on the requested Stream G boundaries, UI crates, Reality Check, notifications, and docs/bench evidence.
- The web dashboard has daemon-backed routes for several API surfaces, but some routes still intentionally use fixtures/deferred responses. I treated those as acceptable only where the API docs explicitly defer the surface or the route has daemon-backed behavior under `WebState::daemon`.

### Positives

- `memoryd::reality_check` has a good module split (`scoring`, `scheduling`, `session`, `types`) and the scoring functions are small, deterministic, and well covered.
- Notification dispatch uses injectable traits for Slack/email/sleep, redacts sensitive error/debug surfaces, and keeps external payload summaries content-free.
- The web server enforces localhost binding and CSRF for POST routes, and the focused web route/CSRF/concurrency tests passed.

### Commands run

```bash
./scripts/rust-boundary-check.sh
# PASS

cargo test -p memoryd-tui --test keymap --test panel_render --test socket_unreachable --test resize
# PASS: keymap 12, panel_render 11, resize 2, socket_unreachable 3

cargo test -p memoryd-web --test csrf --test api_contract --test concurrent_access
# PASS: api_contract 15, concurrent_access 1, csrf 8

cargo test -p memoryd --test scoring --test scheduling --test responses --test dispatcher --test trust_artifact --test slash_commands --test cli_contract
# PASS: cli_contract 18, dispatcher 12, responses 17, scheduling 8, scoring 20, slash_commands 4, trust_artifact 8
```
