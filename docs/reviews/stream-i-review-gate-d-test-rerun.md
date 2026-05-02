Verdict: Approved

# Stream I Gate D Test Rerun

## Scope

Review-only rerun after the Gate D fix. I inspected the new daemon-level coordination coverage, the production delta coordination wiring, and the existing renderer/peer CLI/claim-lock negative-path coverage. I did not edit production code.

## Findings

No blocking findings.

## Acceptance evidence

### Level 1 delta has no coordination

Covered through the actual daemon request path, not just renderer injection. `level1_daemon_delta_has_no_coordination_insertion` constructs a level-1 `HandlerState`, writes a peer memory, calls the daemon delta fixture, and asserts no coordination attribute, no `<peer-update>`, and no `<peer-presence>` (`crates/memoryd/tests/coordination_integration.rs:17-34`). The fixture routes through `handle_request_with_state` with `RequestPayload::Delta`, so this exercises the daemon handler surface (`crates/memoryd/tests/coordination_integration.rs:120-140`). Production coordination also returns no insertion when the effective level is below 2 (`crates/memoryd/src/recall/delta.rs:94-103`).

### Level 2 actual daemon delta includes relevant peer updates from real substrate/index candidate setup

Covered. `level2_daemon_delta_includes_relevant_peer_update_from_index` initializes a real substrate, writes a peer memory through `write_memory`, calls daemon delta, and asserts the Stream I coordination attribute, `<peer-update>`, the peer memory `<ref>`, and claim-lock metadata (`crates/memoryd/tests/coordination_integration.rs:36-52`, `crates/memoryd/tests/coordination_integration.rs:143-157`, `crates/memoryd/tests/coordination_integration.rs:191-258`). The production path builds peer candidates by querying the recall index for active/pinned/candidate passive-recall rows and filtering same-device peer writes (`crates/memoryd/src/recall/delta.rs:130-155`, `crates/memoryd/src/recall/delta.rs:165-167`, `crates/memoryd/src/recall/delta.rs:221-233`).

### Level 3 actual daemon delta includes peer presence before updates

Covered. `level3_daemon_delta_includes_peer_presence_before_peer_update` writes a real peer memory, inserts current and peer presence records into the production presence registry, calls daemon delta, and asserts `<peer-presence>` precedes `<peer-update>` while excluding the current session (`crates/memoryd/tests/coordination_integration.rs:54-71`, `crates/memoryd/tests/coordination_integration.rs:261-275`). Production code attaches active peer presence only at level 3 or higher (`crates/memoryd/src/recall/delta.rs:112-116`) and excludes the caller via `own_session_id` when querying active peers (`crates/memoryd/src/recall/delta.rs:267-293`).

### Production delivery populates peer activity audit

Covered. `production_delta_delivery_populates_peer_activity_audit` calls the daemon delta path, confirms a peer update rendered, then calls the daemon peer-activity path and asserts one recorded delivery with the expected memory id, destination session, and summary (`crates/memoryd/tests/coordination_integration.rs:73-89`, `crates/memoryd/tests/coordination_integration.rs:159-179`). Production delta computes deliveries only for rendered peer updates and records them through the optional delivery recorder (`crates/memoryd/src/recall/delta.rs:73-80`, `crates/memoryd/src/recall/delta.rs:304-333`). The daemon handler passes `Some(state)` as the recorder (`crates/memoryd/src/handlers.rs:1073-1085`), `HandlerState` implements the recorder by writing `PeerDeliveryAuditEntry` records (`crates/memoryd/src/handlers.rs:185-198`), and `peer_activity_response` reads those audit entries with filtering, sorting, and limit handling (`crates/memoryd/src/handlers.rs:511-536`).

### Existing renderer/peer CLI/claim-lock tests still cover negative paths

Still covered and passing:

- Renderer no-coordination shape omits `coordination=`, `<peer-update>`, and `<peer-presence>` (`crates/memoryd/tests/coordination_recall_render.rs:9-17`).
- Renderer privacy negative path masks an email-bearing peer summary (`crates/memoryd/tests/coordination_recall_render.rs:81-90`).
- Startup renderer negative path proves startup can render peer updates but never peer presence (`crates/memoryd/tests/coordination_recall_render.rs:143-163`).
- Peer CLI no-lock release returns `NoLockFound`, and peer commands are rejected from MCP exposure (`crates/memoryd/tests/peer_cli.rs:78-117`).
- Claim-lock negative paths cover level-1 no acquisition, project-minimal suppression, governance refusal not being masked, secret/oversized identity rejection, and previous-holder restoration after post-acquire failure (`crates/memoryd/tests/claim_lock_supersede.rs:12-50`, `crates/memoryd/tests/claim_lock_supersede.rs:137-238`).

## Checks run

```text
cargo test -p memoryd --test coordination_integration --test coordination_recall_render --test peer_cli --test claim_lock_supersede
```

Result: passed.

- `claim_lock_supersede`: 10 passed.
- `coordination_integration`: 4 passed.
- `coordination_recall_render`: 14 passed.
- `peer_cli`: 8 passed.

## Residual risk

This was a focused Gate D test-coverage rerun, not a full Stream I security/performance review. The worktree was already broadly dirty with many Stream G/H/I changes and untracked files; this review intentionally touched only this artifact.
