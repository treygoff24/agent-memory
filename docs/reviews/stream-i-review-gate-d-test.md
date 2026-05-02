Verdict: Changes requested

# Stream I Review Gate D Test Coverage Review

**Date:** 2026-05-02  
**Scope:** Review-only test coverage pass after Stream I Tasks 14-19. Inspected the Stream I spec/plan, coordination crate tests, memoryd recall/heartbeat/claim-lock/peer CLI tests, and the relevant production surfaces. Did not edit production code.

## Blocking finding

### S1 — Per-turn delta coordination is not covered end-to-end and appears unwired

The Gate D acceptance matrix needs Level 2/3 per-turn coordination coverage: Level 2 should be able to surface `<peer-update>` in `<memory-delta>`, and Level 3 should add `<peer-presence>` before peer updates. The current tests only prove the low-level renderer can emit those XML fragments when a handcrafted `CoordinationInsertion` is supplied: `test_peer_presence_absent_at_level2` and `test_peer_presence_emitted_at_level3` call `render_delta_frame(..., Some(&insertion))` directly (`crates/memoryd/tests/coordination_recall_render.rs:43-79`).

The daemon delta path does not pass coordination context at all. `handlers::delta_response` receives `state` but calls `build_delta_response(substrate, request)` with no coordination input (`crates/memoryd/src/handlers.rs:1057-1063`), and `build_delta_response` always renders with `render_delta_frame(&items, budget_tokens, None)` (`crates/memoryd/src/recall/delta.rs:7-20`). As a result, the current test suite can pass while the actual `RequestPayload::Delta` / `memoryd recall delta-block` path never emits Stream I peer updates or Level 3 peer presence.

**Why this blocks Gate D:** this leaves core acceptance coverage missing for recall assembler peer XML in the operational per-turn path and for Level 3 enforcement. It also means the test named `test_peer_presence_emitted_at_level3` is only truthful for the renderer helper, not for the product behavior implied by Level 3.

**Required remediation:** add a daemon-level failing test first, then wire the minimal fix. The regression should exercise the public daemon/handler path, not just `render_delta_frame`:

- Level 2/default: seed a relevant peer write or coordination candidate, call `RequestPayload::Delta`, and assert `coordination="stream-i-v0.1"` plus `<peer-update>`.
- Level 3/collaborative: send/seed peer heartbeat presence, call `RequestPayload::Delta`, and assert `<peer-presence>` precedes `<peer-update>`.
- Level 1/minimal and Tier 3: same public path should assert no `coordination=`, no `<peer-update>`, no `<peer-presence>`.

Keep the renderer unit tests; they are useful, but they are not sufficient as acceptance coverage for Tasks 14 and 17.

## Coverage notes by requested focus

### Recall assembler peer XML

Covered at renderer-unit level:

- No coordination preserves the old delta shape and omits coordination XML (`crates/memoryd/tests/coordination_recall_render.rs:9-17`).
- Peer update insertion, attributes, summary/ref/namespace, and `coordination="stream-i-v0.1"` are asserted (`crates/memoryd/tests/coordination_recall_render.rs:19-41`, `crates/memoryd/tests/coordination_recall_render.rs:92-99`).
- Level 2 absence / Level 3 presence ordering is asserted only by direct renderer injection (`crates/memoryd/tests/coordination_recall_render.rs:43-79`).
- Claim-lock attribute rendering and coordination overflow pending-attention rendering are covered (`crates/memoryd/tests/coordination_recall_render.rs:101-125`).
- Budget accounting has a useful negative path: a peer update can consume budget and force normal delta items to drop while staying under budget (`crates/memoryd/tests/coordination_recall_render.rs:128-140`).

Gap: no public daemon delta acceptance test, per blocker above.

### Recency window using `indexed_at`

Covered with deterministic fixtures in the coordination gate: the test creates one row stale by `indexed_at` and one row authored two hours ago but fresh by `indexed_at`, then asserts only the fresh-by-index row surfaces (`crates/memorum-coordination/tests/gate_unit.rs:138-159`). The implementation compares `candidate.row.indexed_at` against the explicit `now` argument (`crates/memorum-coordination/src/gate.rs:28-45`).

Startup cross-device recency is also covered through SQLite fixture mutation: `set_indexed_at` directly updates the indexed timestamp (`crates/memoryd/tests/startup_recall_mcp.rs:415-420`), and `test_startup_no_cross_device_outside_window` asserts a two-day-stale indexed row is omitted (`crates/memoryd/tests/startup_recall_mcp.rs:123-133`). This uses wall-clock `Utc::now()` with a large margin, so it is practically stable, though less pure than the gate-unit fixture.

### Cross-device startup peer updates

Covered at startup handler level:

- Cross-device writes render `<cross-device-updates>`, `device="other"`, and the expected `<ref>` inside `<entity-recall>` (`crates/memoryd/tests/startup_recall_mcp.rs:104-120`).
- Same-device startup peer updates avoid the `device=` attribute and do not create the cross-device wrapper (`crates/memoryd/tests/startup_recall_mcp.rs:136-146`).
- Cross-device rendering also has renderer-unit coverage for wrapper and `device="other"` shape (`crates/memoryd/tests/coordination_recall_render.rs:176-207`).

### Level 1/2/3 enforcement

Partially covered:

- Level 1/minimal supersede skips Stream I claim-lock acquisition (`crates/memoryd/tests/claim_lock_supersede.rs:12-28`).
- Project `minimal` mode overrides Level 2 config and leaves an existing lock untouched with no contention event (`crates/memoryd/tests/claim_lock_supersede.rs:30-50`).
- Project `default` mode overrides Level 1 fallback and acquires/contends (`crates/memoryd/tests/claim_lock_supersede.rs:52-71`).
- Level 2 successful supersede releases its lock (`crates/memoryd/tests/claim_lock_supersede.rs:73-89`).
- Level 3 heartbeat records presence and renews held claim locks; Level 1/2 heartbeats ack without presence mutation (`crates/memoryd/tests/heartbeat_protocol.rs:67-101`).
- Startup project-mode enforcement is covered for minimal/default/unknown values (`crates/memoryd/tests/startup_recall_mcp.rs:149-212`).

Blocking gap: Level 2/3 per-turn delta enforcement is not covered through the actual daemon delta path and appears unwired (`crates/memoryd/src/handlers.rs:1057-1063`, `crates/memoryd/src/recall/delta.rs:7-20`).

### Tier 3 short-circuit

Covered at behavior level:

- Tier 3 sessions return an empty insertion while Tier 1 receives capped updates (`crates/memorum-coordination/tests/gate_unit.rs:161-175`).
- Session derivation also asserts the relevance gate is skipped for Tier 3-derived binding context (`crates/memorum-coordination/tests/session_derivation.rs:36-53`).
- The implementation checks `session.is_tier3()` before recency filtering, embedding lookup, scoring, sorting, or cooldown mutation (`crates/memorum-coordination/src/gate.rs:28-45`).

Minor hardening recommendation: the plan called for a spy/counter proving zero scoring calls. The current behavior assertions are acceptable, but a cheap scoring-spy regression would make this impossible to accidentally weaken during refactor.

### Peer CLI status/activity/release-lock

Covered at parser, handler, render, and MCP-manifest levels:

- Clap parser accepts `peer status`, `peer activity`, JSON format, session/since filters, and `release-lock --yes` (`crates/memoryd/tests/peer_cli.rs:17-25`).
- Status rendering covers coordination level, active sessions, and claim locks (`crates/memoryd/tests/peer_cli.rs:27-61`).
- Activity rendering covers recorded deliveries (`crates/memoryd/tests/peer_cli.rs:63-76`).
- Release-lock covers no-lock-found and forced-success handler behavior (`crates/memoryd/tests/peer_cli.rs:78-99`).
- Peer admin tool names are absent from MCP parsing/manifest, and `RequestPayload::PeerStatus` is rejected locally with `method_not_allowed_on_mcp` (`crates/memoryd/tests/peer_cli.rs:101-117`).

Coverage gap worth closing before final release: exit-code behavior for the actual CLI process is not asserted. The tests call the handler directly for release-lock no-lock-found (`crates/memoryd/tests/peer_cli.rs:78-85`) rather than spawning the binary and proving the required exit code. The MCP rejection test also samples `PeerStatus`; `mcp.rs` rejects heartbeat/status/activity/release-lock in one match arm (`crates/memoryd/src/mcp.rs:223-242`), but tests should cover the full peer-admin set to prevent future match-arm drift.

### Negative paths

Covered:

- Level 1 no claim acquisition (`crates/memoryd/tests/claim_lock_supersede.rs:12-28`).
- Governance rejection does not get masked by claim-lock warning (`crates/memoryd/tests/claim_lock_supersede.rs:137-166`).
- Secret-like claim-lock session identity is rejected before contention event write; oversized harness identity is rejected (`crates/memoryd/tests/claim_lock_supersede.rs:168-211`).
- Safe plaintext masking of peer summaries replaces an email-containing summary and prevents the raw email from appearing (`crates/memoryd/tests/coordination_recall_render.rs:81-90`).
- No peer-presence at startup is covered by renderer-level startup test and startup minimal-mode assertions (`crates/memoryd/tests/coordination_recall_render.rs:143-163`, `crates/memoryd/tests/startup_recall_mcp.rs:161-165`).
- Release-lock no-lock-found handler behavior is covered (`crates/memoryd/tests/peer_cli.rs:78-85`).
- Stale cleanup removes stale presence and releases held claim locks under paused Tokio time (`crates/memoryd/tests/stale_session_cleanup.rs:10-33`).

Needs hardening:

- Budget overflow coverage proves normal items drop behind peer XML, but does not explicitly assert the case where a coordination entry itself exceeds the budget and is omitted without overflowing.
- MCP rejection should enumerate all peer-admin payloads, not just `PeerStatus` plus the heartbeat test in `mcp_manifest` (`crates/memoryd/tests/mcp_manifest.rs:89-111`, `crates/memoryd/tests/peer_cli.rs:101-117`).
- CLI release-lock no-lock-found should assert process exit code `1`, not only protocol status.

### Test determinism and clock fixtures

Good:

- `memorum-coordination` recency and tier tests use fixed `Utc.with_ymd_and_hms(...)` fixtures (`crates/memorum-coordination/tests/gate_unit.rs:332-333`).
- Stale-session cleanup uses `#[tokio::test(start_paused = true)]` and `tokio::time::advance`, which is deterministic for the cleanup interval (`crates/memoryd/tests/stale_session_cleanup.rs:10-32`, `crates/memoryd/tests/stale_session_cleanup.rs:35-55`).
- Peer delivery/status test fixtures use fixed timestamps for rendered output (`crates/memoryd/tests/peer_cli.rs:216-227`).

Acceptable but less ideal:

- Startup cross-device recency tests use `Utc::now() - Duration::days(2)` to force stale indexed time (`crates/memoryd/tests/startup_recall_mcp.rs:123-128`). The margin is large relative to the one-day default window, so it should not be flaky, but a fixture clock seam would be cleaner.

### Test naming and behavior-vs-implementation detail

Mostly good. The tests generally read as behavior specs and assert public outputs or registry state. The main exception is `test_peer_presence_emitted_at_level3`: the name implies Level 3 product behavior, but the test only constructs a `CoordinationInsertion` manually and calls the renderer (`crates/memoryd/tests/coordination_recall_render.rs:50-79`). Rename it to make the scope explicit, or keep it and add the missing daemon-level Level 3 delta test.

## Validations run

```bash
cargo test -p memorum-coordination --test gate_unit --test session_derivation --test presence_unit --test claim_lock_unit
```

Result: passed — 17 `claim_lock_unit`, 17 `gate_unit`, 23 `presence_unit`, and 9 `session_derivation` tests.

```bash
cargo test -p memoryd --test project_binding_concurrent_mode --test heartbeat_protocol --test stale_session_cleanup --test claim_lock_supersede --test coordination_recall_render --test peer_cli
```

Result: passed — 6 `project_binding_concurrent_mode`, 9 `heartbeat_protocol`, 2 `stale_session_cleanup`, 10 `claim_lock_supersede`, 14 `coordination_recall_render`, and 8 `peer_cli` tests.

```bash
cargo test -p memoryd --test startup_recall_mcp
```

Result: passed — 12 startup/MCP tests, including cross-device startup and project-mode coordination tests.

Attempted but invalid command:

```bash
cargo test -p memoryd --test startup_recall_mcp test_cross_device_startup_peer_update test_startup_no_cross_device_outside_window test_startup_same_device_peer_update_no_device_attr test_level1_no_peer_update_from_project_mode test_level2_default_when_mode_absent project_default_mode_overrides_level1_config_fallback unknown_project_mode_rejects_startup_end_to_end
```

Result: failed before running tests because `cargo test` accepts only one test-name filter before `--`; I replaced it with the full `startup_recall_mcp` test file run above.

## Remaining risk

This was a focused test-coverage review, not a full security/performance review. The worktree is broadly dirty with many pre-existing Stream G/H/I changes and untracked files; this artifact is the only file I intentionally wrote. Until the daemon delta coordination blocker is fixed with a public-path regression test, Gate D should not approve.
