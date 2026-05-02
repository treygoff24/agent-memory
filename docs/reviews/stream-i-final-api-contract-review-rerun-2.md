# Stream I Final API Contract Review Rerun 2

Status: Approved

## Scope

Narrow rerun of the two Severity 2 blockers from `docs/reviews/stream-i-final-api-contract-review-rerun.md` after the latest fixes:

1. Startup peer-update relevance must not mutate the receiving `SessionContext` from candidate project rows, and must not fall back to unconditional project peer-update insertion when the relevance gate returns no entries.
2. `PeerHeartbeatAck` must have one reconciled shape across the normative spec, API docs, DTO, handler, and tests, including the implemented `active_peers` field.

The working tree was already heavily dirty before this review. I did not edit implementation code; this review artifact is the only intended write.

## Findings

No blocking findings in the reviewed scope.

## Blocker 1 rerun: startup peer-update relevance

Approved. The current startup path evaluates peer writes against the receiving session context without candidate-derived salience mutation or unconditional project fallback.

Evidence:

- Startup builds one receiving `SessionContext` from the validated session binding and the non-peer startup recall selection (`startup_context_from_selection`), then passes that context into startup peer-update evaluation (`crates/memoryd/src/recall/startup.rs:149-151`).
- Same-device startup updates clone that precomputed receiving context, filter by `indexed_at` recency, build `PeerWriteCandidate`s, and call `RelevanceGate::evaluate`; the only return path is `non_empty_insertion`, which drops an empty gate result instead of emitting fallback peer-updates (`crates/memoryd/src/recall/startup.rs:270-287`, `crates/memoryd/src/recall/startup.rs:323-329`).
- Cross-device startup updates use the same receiving-context pattern, with the cross-device threshold/window overrides. If the gate returns no peer updates, the function returns `None`; there is no project fallback branch (`crates/memoryd/src/recall/startup.rs:289-321`).
- The removed risky helpers are not present: `rg` found no `add_project_candidate_entities`, no `project_startup_insertion`, and no `relevance: 1.0` project fallback in `crates/memoryd/src/recall/startup.rs`.
- Project-scope candidates are still scoped normally (`row_is_in_startup_scope`) and converted into candidate paths/namespace without mutating the receiver (`crates/memoryd/src/recall/startup.rs:257-268`, `crates/memoryd/src/recall/startup.rs:378-421`). This means same-device and cross-device project rows must clear the same relevance gate; they no longer make themselves relevant by being appended to `session.salient_entities`.
- The gate score uses candidate entities/paths against `session.salient_entities` and `session.salient_paths`, and only records selected peer-write ids after scoring; it does not add candidate salience into the session (`crates/memorum-coordination/src/gate.rs:56-87`, `crates/memorum-coordination/src/gate.rs:96-128`).
- The spec still requires scoring against the current session context (`docs/specs/stream-i-cross-session-v0.1.md:212-221`), derives salient entities/paths from startup/session inputs rather than peer candidates (`docs/specs/stream-i-cross-session-v0.1.md:292-308`), and limits startup insertion to salient, recent peer activity (`docs/specs/stream-i-cross-session-v0.1.md:392-422`). Current code matches that contract.

Test evidence:

- Same-device project-scope negative coverage is now explicit: `project_peer_update_requires_receiving_session_salience` writes an unrelated project peer memory and asserts no `<peer-update>` and no memory id are surfaced (`crates/memoryd/tests/startup_recall_mcp.rs:218-235`).
- The prior config-level fallback risk is covered by `project_default_mode_overrides_level1_config_fallback`, which verifies a project `default` mode can override global Level 1 while an unrelated project peer write still does not surface (`crates/memoryd/tests/startup_recall_mcp.rs:238-260`).
- Cross-device positive/recency behavior remains covered by `test_cross_device_startup_peer_update` and `test_startup_no_cross_device_outside_window` (`crates/memoryd/tests/startup_recall_mcp.rs:104-135`). I did not find an explicit cross-device unrelated-project negative test, but the reviewed cross-device implementation shares the no-mutation/no-fallback gate path and uses a stricter threshold; this is a non-blocking residual test-depth risk rather than an open contract mismatch.

## Blocker 2 rerun: `PeerHeartbeatAck` shape

Approved. The spec/API/docs/DTO/handler/tests now agree that `PeerHeartbeatAck` includes `active_peers`.

Evidence:

- The normative spec's `PeerHeartbeatAck` now includes `active_peers: Vec<ActivePeer>` and documents its Level 3/public-projection behavior directly in the ACK shape (`docs/specs/stream-i-cross-session-v0.1.md:473-486`).
- The API docs expose the same fields: `session_id`, `active_level`, `peer_session_count`, `active_peers`, and `conflicting_claim_locks` (`docs/api/stream-i-cross-session-api.md:32-43`).
- The coordination DTO matches those fields exactly (`crates/memorum-coordination/src/protocol.rs:21-29`), and `memoryd` re-exports that DTO for its daemon protocol response variant (`crates/memoryd/src/protocol.rs:11`, `crates/memoryd/src/protocol.rs:95-99`, `crates/memoryd/src/protocol.rs:223`).
- The heartbeat handler delegates to the coordination heartbeat handler, then populates `conflicting_claim_locks` for Level 3 by intersecting live locks with the heartbeat session's salient entities (`crates/memoryd/src/handlers.rs:569-593`, `crates/memoryd/src/handlers.rs:602-645`).
- The coordination heartbeat path returns `active_peers` only at Level 3 and sets `peer_session_count` from that bounded projection (`crates/memorum-coordination/src/presence.rs:243-277`).
- The `ActivePeer` projection truncates session ids and caps salient entities before serialization (`crates/memorum-coordination/src/presence.rs:341-348`). Tests assert hidden rich fields are absent from `active_peers` JSON and that entity/session-id caps are enforced (`crates/memorum-coordination/tests/presence_unit.rs:261-300`, `crates/memorum-coordination/tests/presence_unit.rs:302-324`).
- `memoryd` integration tests assert both `active_peers` and `conflicting_claim_locks` behavior on the daemon handler path (`crates/memoryd/tests/heartbeat_protocol.rs:57-69`, `crates/memoryd/tests/heartbeat_protocol.rs:107-124`).

## Commands run

- `git status --short`
- `sed -n '1,260p' docs/reviews/stream-i-final-api-contract-review-rerun.md`
- `rg -n "PeerHeartbeatAck|active_peers|heartbeat|startup|peer|project|SessionContext|session_context|receiving|candidate" docs/specs/stream-i-cross-session-v0.1.md docs/api/stream-i-cross-session-api.md crates/memorum-coordination/src/protocol.rs crates/memoryd/src/handlers.rs crates/memoryd/src/recall/startup.rs crates/memoryd/tests/startup_recall_mcp.rs crates/memoryd/tests/heartbeat_protocol.rs`
- `rg -n "add_project_candidate_entities|project_startup_insertion|fallback|relevance: 1\.0|candidate-derived|candidate.*session|salient_entities.*row|project candidate" crates/memoryd/src/recall/startup.rs crates/memoryd/tests/startup_recall_mcp.rs docs/specs/stream-i-cross-session-v0.1.md`
- `cargo test -p memoryd --test startup_recall_mcp` — passed, 14 tests.
- `cargo test -p memoryd --test heartbeat_protocol` — passed, 10 tests.
- `cargo test -p memorum-coordination --test presence_unit` — passed, 23 tests.

## Residual risks / non-blocking notes

- I did not rerun full workspace gates because this was a narrow contract rerun and recent broader focused gates were already reported as passing.
- I did not find a dedicated cross-device unrelated-project negative test. The code path is shared with same-device relevance evaluation and now returns `None` on empty gate output, so I do not consider this a blocker. A future hardening test would be useful: write a project-scope peer row from `source_device = other` with no receiving-session entity salience and assert no `<cross-device-updates>` / no memory id is rendered.
- The repo remains heavily dirty with many pre-existing modified/untracked files; this review should be interpreted against that live working tree state.
