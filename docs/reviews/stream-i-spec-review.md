# Stream I v0.1 spec review

**Reviewer:** plan-reviewer (Claude sonnet-4-6, fresh context)
**Date:** 2026-05-01
**Spec:** docs/specs/stream-i-cross-session-v0.1.md (981 lines)
**Verdict:** BLOCK

Five blockers need resolution before implementation starts. Three are outright specification errors that will produce wrong code. One is an ambiguity between two sections that leaves claim-lock acquisition wiring undefined. One is a design gap for cross-device Level 2 content that creates a silent permanent-drop scenario with no fallback.

---

## Blockers

**1. §4.3 Tier 3 description: the parenthetical "at least one entity in common" is mathematically false.**

The entity_overlap formula is Jaccard similarity: `|intersection| / |union|`. Jaccard = 1.0 requires p.entities and s.salient_entities to be identical non-empty sets. "At least one entity in common" describes Jaccard > 0, which is a completely different condition. The parenthetical directly contradicts the formula.

Concrete counterexample: peer write has entities `{auth_flow}`, session has salient entities `{auth_flow, users_table, email_column}`. These have one entity in common. Jaccard = 1/3. Tier 3 score = 0.5 \* (1/3) = 0.167, well below `tier3_threshold = 0.5`. The phrase "at least one entity in common" predicts this fires; it does not. The correct behavior is: Tier 3 peer-updates fire only when the peer write's entity set and the session's salient entity set are identical non-empty sets (Jaccard = 1.0, score = 0.5, equals tier3_threshold). Interestingly, the rest of the §4.3 paragraph correctly says "entity-exact match" — the error is only in the parenthetical.

The full Tier 1 score arithmetic is worth showing clearly, since focus area #2 flags this as high-stakes:

```
score = 0.5 * E + 0.3 * P + 0.2 * T    (E, P, T in [0,1])

E=1.0, P=0,   T=0:   score = 0.50  — below threshold 0.6, does NOT surface
E=1.0, P=0.5, T=0:   score = 0.65  — surfaces
E=0,   P=1.0, T=1.0: score = 0.50  — below threshold, does NOT surface
E=0.5, P=0.5, T=0.5: score = 0.50  — below threshold, does NOT surface

Maximum score without entity overlap: 0.3 + 0.2 = 0.5 < 0.6 threshold.
Entity overlap is therefore a necessary condition for any peer-update to fire at Tier 1.
```

This threshold/weight combination means no pure path+topic combination can ever clear the gate. An implementor who tests a peer-update that has strong path and topic relevance but no entity overlap will be confused when it never fires. The spec should call this out explicitly in §4.1, not just leave it discoverable by arithmetic.

Fix: in §4.3, replace "(i.e., entity overlap >= 1.0 — at least one entity in common)" with "(i.e., Jaccard = 1.0: peer write's entity set and session's salient entity set are identical non-empty sets)." In §4.1, add a sentence noting that entity overlap is a necessary condition for the gate to fire under the current weight configuration.

**2. §6.1 `started_at: DateTime<Utc>` type is incompatible with its own comment.**

The `PeerHeartbeat` struct in §6.1 declares:

```rust
started_at: DateTime<Utc>,  // session start time (first heartbeat only; others may omit via None)
```

The type `DateTime<Utc>` cannot be `None`. These are irreconcilable. Implementing this as written produces a compilation error when the implementor tries to omit the field on renewal heartbeats, or the "may omit" behavior is silently lost if the implementor changes the type without realizing the protocol serialization needs updating.

Fix: change the type to `Option<DateTime<Utc>>` with `#[serde(skip_serializing_if = "Option::is_none")]`. The protocol cost is minimal (one optional field absent on most heartbeats) and the type then matches the intended semantics.

**3. §8.2 misses the pre-parse whitelist in the shipped `project.rs`.**

The spec says Stream I must update "Stream E's project-binding parser" to accept `concurrent_session_mode`. The shipped file at `/Users/treygoff/Code/agent-memory/crates/memoryd/src/recall/project.rs` line 81 contains:

```rust
if key.is_empty() || !matches!(key, "canonical_id" | "alias") {
    return Err(RecallError::invalid_request(format!("unknown {PROJECT_FILE} field: {key}")));
}
```

This pre-parse whitelist runs before serde deserialization. Adding `concurrent_session_mode` to the `ProjectFile` struct is necessary but not sufficient. Without updating this whitelist, every project file that sets `concurrent_session_mode: collaborative` fails with an opaque "unknown .memory-project.yaml field" error, silently falls back to git-remote project binding, and loses the Level 3 opt-in with no visible indication. The agent's session continues to operate, just at the wrong coordination level.

Fix: the spec must explicitly name `/Users/treygoff/Code/agent-memory/crates/memoryd/src/recall/project.rs` line 81 and the `matches!` arm as required changes in §8.2, alongside the struct and serde updates.

**4. §3.3 and §7.1 conflict on which sessions acquire claim locks.**

Section 3.3 lists claim-lock acquisition under "Level 3 — Presence + intent (opt-in)": "Sessions in Level 3: ... 3. Acquire claim locks on memory_supersede workflow entries." Section 7.1 describes the `handle_supersede` path as unconditionally calling `claim_lock_registry.acquire()` with no level check shown or mentioned.

These cannot both be right. An implementor reading §7.1 in isolation wires claim-lock acquisition for all sessions. An implementor reading §3.3 first adds a level check. The practical consequence of the unconditional interpretation: Level 2 sessions silently acquire claim locks they didn't expect per their coordination contract, receive the `claim_lock` envelope field in their `memory_supersede` response, and generate `claim_locked` attributes in peer-updates for other Level 2 sessions — all of which is behavior §3.2 does not list as Level 2 features.

The advisory-only design (§7.1 rationale) actually makes unconditional claim locks for Level 2 sessions relatively harmless (they expire by TTL, §7.3 item 2), but the spec must choose rather than contradict itself.

Fix: §7.1 must include an explicit level check statement. Either "claim lock acquisition runs only when the session's project coordination level is 3" (if §3.3 is authoritative), or "claim lock acquisition runs at all levels" with §3.3 updated to list claim locks as a Level 2+ feature.

**5. Cross-device Level 2 content has no fallback for ordinary late-arriving syncs.**

Section 5.3 applies the 30-minute recency window against `updated_at` (the peer's write time), not the local sync-arrival time. The spec correctly notes that promoted writes older than 30 minutes fall back to normal Stream E entity-recall. But Stream E's safety invariants (§2) explicitly exclude candidates, quarantined memories, and substrate notes from factual recall content — these can only surface via the peer-update path.

Level 2's specific value over Level 1 is exactly these content types: in-flight proposals, substrate notes, and `memory_observe` fragments. A cross-device sync that completes 35 minutes after the peer wrote candidates and notes means those items are permanently undeliverable: older than the 30-minute window, no Stream E fallback. The extended window in §5.3 only fires for "the first session after a multi-day absence," not for an ordinary same-day sync gap of 35-90 minutes.

This is either an accepted limitation that needs explicit documentation — "Level 2 cross-device delivery of non-promoted writes is effectively unavailable when sync-to-write delay exceeds 30 minutes" — or it requires a separate recency window for cross-device Level 2 content that doesn't require multi-day absence to trigger. Either resolution is acceptable; the current state where neither outcome is documented is not.

---

## Risks

**1. Entity overlap as an undocumented necessary condition.**

As shown in the Blocker 1 arithmetic, no combination of path and topic similarity can clear the 0.6 threshold without entity overlap (max non-entity score = 0.5). Entity overlap is therefore a necessary condition for any peer-update to fire, but §4.1 never states this. An operator debugging a missed peer-update on a highly topic-relevant write will need to work out the arithmetic themselves. A prose statement in §4.1 would save that effort and prevent incorrect threshold-tuning attempts.

**2. PresenceRegistry has no hard upper bound on entries.**

`PresenceRegistry` uses `DashMap<String, PresenceRecord>` with no cap on entries. With stale threshold at 5 minutes and cleanup every 60 seconds, up to 5 minutes of new sessions accumulate between sweeps. Under adversarial conditions (a harness reconnecting rapidly, a test suite without cleanup, a buggy client spamming heartbeats), the registry grows without bound between cleanup cycles. Each entry holds up to 32 entity ids and 32 namespace paths. The spec should add a `max_registered_sessions` config parameter (suggested default: 256) with a documented eviction policy on cap breach, and the cleanup task should enforce this cap during each sweep.

**3. Stream H framing test seam is verbally agreed but unresolved at the file level.**

Stream H §10.1 says "this spec defers the framing test ownership decision to Stream I." Stream I §10 resolves it: assertion logic in `crates/memorum-coordination/src/framing_tests.rs`, invocation via Stream H's `harness_runner.rs`. But Stream H's test catalog ends at `t18_encrypted_tier_key_rotation.rs` with no `t19_peer_update_framing.rs` in `crates/memorum-eval/src/tests/eval/domain/`. The assertion code has no connection to `harness_runner.rs` without a glue file that neither spec names. The Stream I implementation plan must add a concrete file to that directory, and Stream H §10.1 should be closed rather than left open since Stream I has answered it.

**4. Framing test cost is not acknowledged.**

6 harness-invocation cases times 3 runs each equals 18 real LLM API calls per framing suite run. Stream H treats tests #13 and #15 as real-harness-only, gated behind credential env vars and skipped in normal PR runs. The spec should state the same classification for framing tests. Without this, the CI author may add them to the per-PR gate, creating ongoing inference cost and credential-dependent test flakiness.

**5. Benchmark baseline initialization path is unspecified.**

Section 13.4 adds a `peer_relevance_gate` section to `bench/baseline.darwin-arm64.json`. The human-authored-commit invariant means this section must exist before automated runs can validate against it. Stream A's bootstrap path emits a `.proposed` file on first run when a section is absent. Stream I should state explicitly that its bench follows the same convention, rather than leaving the first-run behavior undefined (where the automated runner might fail or silently skip the gate).

**6. Candidate evaluation count has no hard ceiling.**

Section 13.1 bounds the delta-block latency contribution at `N_candidates * 5ms`, noting "typically 0-10 writes." The bench fixture uses 100 candidates; at 5ms each (embedding excluded), that is 500ms before Stream E's own work begins. Stream E's delta-block p95 budget is 120ms. After a multi-day absence on a project with an active peer, there could be 50-200 peer writes within the extended cross-device window, easily exceeding 1000ms of gate evaluation. The spec needs either a hard cap on candidates evaluated per delta-block call or an explicit acknowledgment that the 120ms budget is advisory and will be exceeded under high-candidate conditions.

---

## Nits

**1. §12 should be renamed.**

Items 1, 2, 5, and 6 in "Open questions" are confirmed design decisions with explicit rationale, not open questions. Item 1 (embedding freshness fallback to 0.0) is a deliberate tradeoff. Item 2 (no entity decay) is explicitly deferred post-dogfood. Item 5 (no cross-device claim locks) has a named v2 path. Only item 4 (Tier 2 heartbeat) is genuinely open. Renaming the section to "Known tradeoffs and post-v1 follow-ups" avoids triggering unnecessary redesign discussion during implementation review.

**2. `EventKind::ClaimLockContention` in §7.4 is an undisclosed Stream A surface addition.**

Section 7.4 says the daemon "logs a contention event to the event log with `EventKind::ClaimLockContention { memory_id, holder, contender }`." The event log is the Stream A JSONL. Adding a new `EventKind` variant is a Stream A surface change. Section 1.1 says "Stream A — no changes required." Either this is daemon-internal state only (not written to JSONL) and §7.4 should say "logs to daemon-internal audit," or it is a Stream A surface addition that must be disclosed in §1.1 following the precedent of Stream E's `MemoryQuery` extension and Stream D's `EventKind::EncryptedContentRevealed` addition.

**3. `memoryd peer activity` output should signal its ephemerality.**

The command sample output in §9.2 shows a peer-update delivery log but includes no indication that this trail resets on daemon restart. An operator debugging a missed peer-update after a crash will see an empty list with no explanation. A header line such as "In-memory audit since daemon start at HH:MM; resets on restart" would make the ephemerality explicit. The same applies to `memoryd peer status` (active sessions and claim locks also reset on restart).

---

## Cross-spec consistency findings

**1. Intra-`<entity-recall>` element ordering at startup is unspecified.**

Stream E's API doc specifies section order for `<memory-recall>` but does not document element order within `<entity-recall>`. Stream I §5.3 inserts `<peer-update>` elements as siblings of `<memory>` elements inside `<entity-recall>`. The relative ordering (peer-updates before or after memory elements?) should be stated explicitly in §5.3 to make the insertion deterministic and testable with `test_cross_device_startup_peer_update` in §11.2.

**2. `peer_update_total` counter has no defined location in `StatusResponse`.**

Stream G Panel 1 (per `stream-g-observability-v0.1.md` §3.2) displays "peer-updates:8" sourced from `StatusResponse`. Stream E's recall counter schema in `docs/api/stream-e-passive-recall-api.md` defines `startup_total` and `delta_total`. Stream I adds peer-update deliveries but doesn't specify where `peer_update_total` lives in `StatusResponse`. Stream G assumes it exists; neither Stream I's spec nor Stream E's API doc declares it. Stream I §1.1 or §2.2 should add `peer_update_total: u64` to the `StatusResponse.recall` counter schema.

**3. Stream H §10.1 is answered by Stream I but neither spec closes the loop.**

Stream H §10.1 leaves the framing test ownership as an explicit open question, with a note that Stream I should resolve it. Stream I §10 does resolve it. But Stream H §10.1 still reads as open, and the Stream H plan author won't know to add `t19_peer_update_framing.rs` to `crates/memorum-eval/src/tests/eval/domain/`. The Stream I spec should include a callout: "Stream H must add `crates/memorum-eval/src/tests/eval/domain/t19_peer_update_framing.rs` to wire the `memorum_coordination::framing_tests::assert_framing` function to Stream H's `harness_runner.rs` execution path."

---

## Things I checked and found correct

The `memory_subscribe` removal compliance is clean. Level 3 heartbeats are single-shot request-response (PeerHeartbeat to PeerHeartbeatAck). Peer-updates are consumed via the existing pre-turn hook call to `memoryd recall delta-block`. Presence data is assembled per-call during `handle_delta_block`. There is no SSE, no WebSocket, no long-polling, no new connection lifecycle anywhere in the spec. The nine-tool freeze holds: `memoryd peer {status,activity,release-lock}` are all admin CLI, explicitly rejected from MCP forwarding. The v1 anti-features are cleanly absent.

The claim-lock contention resolution (warn-but-allow at §7.4) is the right call, with sound justification. Hard refusal on a stale lock would require a daemon restart to unblock, producing a worse user experience than an advisory warning. The TTL-based expiry and stale-session sweeper together handle the crash-mid-supersede scenario without requiring explicit release. The rationale in §7.1 explains the choice clearly and commits to it without hedging.

The cool-down and per-turn cap semantics are correctly specified. Capped entries — those that pass the threshold but lose the per-turn cap selection — do not enter `surfaced_peer_writes`, so they remain candidates for subsequent turns until they leave the recency window. The spec doesn't state this property explicitly, but the cool-down registry's semantics (tracking surfaced ids only, not capped ids) make it true. This gives fair distributed delivery across multiple turns: if five writes all pass the gate in turn 1, two surface and three are capped but not cooled-down, so they remain eligible in turn 2. The unit test `test_per_turn_cap` in §11.1 should verify not only the cap count but also that capped ids do not appear in `surfaced_peer_writes`.

The framing test pass criteria in §10.3 are testable without LLM judgment for the hard criterion. Criterion 1 (attribution correct) uses a static misattribution phrase list with case-insensitive matching. The hard/soft hierarchy — criterion 1 must hold at all temperatures, criteria 2 and 3 are soft at temperature 1.0 only — is the right approach for a behavioral property that must be reliable under normal use but can degrade gracefully at maximum stochastic sampling.

The `<peer-update>` XML schema carries exactly the right framing attributes. The `from`, `session`, `ts`, and optional `device` attributes together communicate: which harness wrote this, which specific session, at what time, and whether it came from a different machine. Every attribute is load-bearing. The spec treats the framing as a hard correctness requirement, not a cosmetic preference, which is correct — an agent that misattributes a peer-update as user input and acts on it unconditionally is exhibiting a behavioral defect, not just a display issue.
