Verdict: Approved

# Stream I Review Gate D Security/Privacy Review

**Scope:** Read-only security/privacy review after Stream I Tasks 14-19. Reviewed recall XML rendering, startup coordination rendering, Level 1 short-circuiting, claim-lock advisory/RAM-only behavior, peer CLI/admin exposure, MCP forwarding boundaries, and focused tests.

## Findings

No blocking security or privacy findings.

## Evidence checklist

### `<peer-update>` summary privacy

Pass. `<peer-update>` summaries are rendered through the Stream D safe-fragment boundary before XML insertion. `render_peer_update_element` emits `<summary>` from `safe_peer_summary(&entry.summary)`, and `safe_peer_summary` calls `memory_privacy::safe_plaintext_fragment` with the deterministic classifier, replacing both encrypted/secret and review-pending decisions with the privacy placeholder before escaping (`crates/memoryd/src/recall/render.rs:350-376`, `crates/memoryd/src/recall/render.rs:393-401`). The behavior test covers an email canary and asserts it is absent from output (`crates/memoryd/tests/coordination_recall_render.rs:81-90`).

### `<peer-presence>` startup exclusion and delta-only rendering

Pass. Startup rendering converts `StartupCoordinationRender.same_device` through `render_peer_update_elements`, which only maps `peer_updates`; `peer_presence` is not rendered into `<memory-recall>` (`crates/memoryd/src/recall/render.rs:146-180`, `crates/memoryd/src/recall/render.rs:327-329`). Delta rendering is the only renderer that emits `<peer-presence>`, and it places presence before peer updates when `CoordinationInsertion.peer_presence` is non-empty (`crates/memoryd/src/recall/render.rs:192-210`, `crates/memoryd/src/recall/render.rs:306-324`, `crates/memoryd/src/recall/render.rs:378-390`). Tests assert startup never emits presence and delta presence ordering is correct (`crates/memoryd/tests/coordination_recall_render.rs:50-79`, `crates/memoryd/tests/coordination_recall_render.rs:143-163`).

### Level 1 short-circuit

Pass. Startup peer-update construction returns no coordination updates when the effective coordination level is below 2 (`crates/memoryd/src/recall/startup.rs:183-190`, `crates/memoryd/src/recall/startup.rs:209-216`). Supersede claim-lock acquisition also returns `SupersedeClaimLock::inactive()` before calling `ClaimLockRegistry::acquire` when `effective_coordination_level(meta) < 2` (`crates/memoryd/src/handlers.rs:1684-1699`). Tests cover global Level 1 and project `minimal` suppression for claim locks and startup peer updates (`crates/memoryd/tests/claim_lock_supersede.rs:12-50`, `crates/memoryd/tests/startup_recall_mcp.rs:149-166`).

### Conditional `coordination=` attribute

Pass. Startup adds `coordination="stream-i-v0.1"` only when same-device or cross-device peer-update output is non-empty (`crates/memoryd/src/recall/render.rs:152-159`). Delta adds the attribute only when at least one coordination entry actually rendered within budget, not merely because an insertion object exists (`crates/memoryd/src/recall/render.rs:200-210`, `crates/memoryd/src/recall/render.rs:228-235`). Tests cover `None`, empty, and populated cases (`crates/memoryd/tests/coordination_recall_render.rs:9-17`, `crates/memoryd/tests/coordination_recall_render.rs:92-99`, `crates/memoryd/tests/coordination_recall_render.rs:165-174`).

### Peer admin surfaces are not MCP-exposed

Pass. The MCP tool enum and manifest are still the frozen nine agent-facing tools and contain no peer/admin tool (`crates/memoryd/src/mcp.rs:29-40`, `crates/memoryd/src/mcp.rs:246-258`). Even if a peer/admin daemon payload is attempted through the generic MCP forwarding helper, `PeerHeartbeat`, `PeerStatus`, `PeerActivity`, and `PeerReleaseLock` are rejected locally with `method_not_allowed_on_mcp` before socket I/O (`crates/memoryd/src/mcp.rs:223-242`). Tests assert peer command names do not parse as MCP tools and the forwarding choke point rejects peer payloads (`crates/memoryd/tests/peer_cli.rs:101-117`).

### Delta budget accounting

Pass. Delta coordination entries use the same `estimated_tokens` budget estimator as Stream E items and are pushed only when `used_tokens + tokens <= budget_tokens`; normal delta items are admitted only after coordination tokens are counted (`crates/memoryd/src/recall/render.rs:192-218`, `crates/memoryd/src/recall/render.rs:306-324`, `crates/memoryd/src/recall/render.rs:412-420`). The regression test proves a peer-update consumes budget and can exclude a normal item without exceeding the budget (`crates/memoryd/tests/coordination_recall_render.rs:128-141`).

### Cross-device `device="other"` framing

Pass. Cross-device startup rendering wraps entries in `<cross-device-updates from-sync="...">`, clones each update, forces `device = Some("other")`, and then renders the peer update (`crates/memoryd/src/recall/render.rs:331-347`). Startup selection also stamps cross-device updates as `"other"` before returning the wrapper object (`crates/memoryd/src/recall/startup.rs:255-280`). Tests assert the wrapper and `device="other"` attribute (`crates/memoryd/tests/coordination_recall_render.rs:176-208`, `crates/memoryd/tests/startup_recall_mcp.rs:104-120`).

### Claim-lock advisory and RAM-only behavior

Pass. Claim locks live in `ClaimLockRegistry`, a `DashMap` held by `HandlerState`; there is no serialization or canonical-frontmatter write path for the lock registry (`crates/memorum-coordination/src/claim_lock.rs:148-152`, `crates/memoryd/src/handlers.rs:90-123`). Acquisition remains advisory: contention replaces the holder in RAM, records an advisory warning/event, and the supersede continues; success releases the holder, and failure rollback releases/restores RAM state (`crates/memoryd/src/handlers.rs:1700-1725`, `crates/memoryd/src/handlers.rs:1793-1840`). The only disk write in the contention path is the authorized `EventKind::ClaimLockContention` audit event, not claim-lock state (`crates/memoryd/src/handlers.rs:1706-1715`). Tests cover Level 1 no-acquire, contention warning/event emission, and rollback on post-acquire failure (`crates/memoryd/tests/claim_lock_supersede.rs:12-50`, `crates/memoryd/tests/claim_lock_supersede.rs:91-135`, `crates/memoryd/tests/claim_lock_supersede.rs:213-238`).

## Validation run

Passed:

```bash
cargo test -p memoryd --test coordination_recall_render --test peer_cli --test claim_lock_supersede
```

Result: 10 `claim_lock_supersede` tests, 14 `coordination_recall_render` tests, and 8 `peer_cli` tests passed.

Passed:

```bash
cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings
```

Passed additional startup-focused check:

```bash
cargo test -p memoryd --test startup_recall_mcp
```

Result: 12 tests passed, including cross-device startup framing, startup Level 1 suppression, and startup response budget shape.

## Residual risk

- This security review did not prove every future caller of `PeerDeliveryAuditEntry` redacts summaries before admin CLI display; no production call site currently records peer deliveries outside tests, and peer activity is MCP-rejected/admin-only.
- This review treats daemon-local peer/admin protocol calls as an intended local-admin boundary and focuses on the requested MCP forwarding boundary.
- Presence `entities` are XML-escaped and bounded, but they are trusted to be canonical entity ids per spec; if future harnesses send free-form labels there, a separate privacy hardening pass should enforce an `ent_...` shape or safe descriptor projection.

Confidence: high for the requested Gate D security/privacy checklist.
