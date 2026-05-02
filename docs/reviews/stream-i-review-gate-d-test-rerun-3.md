Verdict: Approved

# Stream I Gate D test rerun 3

Review-only rerun after source identity/startup attribution/path-overlap fixes. I did not edit production code; this artifact is the only intended write from this pass.

## Focused command

```bash
cargo test -p memoryd --test coordination_integration --test coordination_recall_render --test startup_recall_mcp --test peer_cli --test claim_lock_supersede
```

Result: passed.

Observed suite totals:

- `claim_lock_supersede`: 10 passed, 0 failed
- `coordination_integration`: 8 passed, 0 failed
- `coordination_recall_render`: 14 passed, 0 failed
- `peer_cli`: 8 passed, 0 failed
- `startup_recall_mcp`: 13 passed, 0 failed

Total focused coverage executed: 53 passed, 0 failed.

## Required coverage verification

- Production delta Level 1/2/3 insertion is covered in `crates/memoryd/tests/coordination_integration.rs`:
  - Level 1 suppression: `level1_daemon_delta_has_no_coordination_insertion` at lines 18-34 asserts no `coordination=`, `<peer-update`, or `<peer-presence`.
  - Level 2 peer-update insertion: `level2_daemon_delta_includes_relevant_peer_update_from_index` at lines 37-52 asserts Stream I coordination root attribute, peer update, ref, claim-lock attribution, and summary-only/no raw body behavior.
  - Level 3 presence-before-update insertion: `level3_daemon_delta_includes_peer_presence_before_peer_update` at lines 55-71 asserts `<peer-presence>` before `<peer-update>` and excludes the current session.

- Actual writer attribution in delta XML/audit is covered by `peer_update_uses_actual_writer_attribution_in_delta_and_audit` at `coordination_integration.rs` lines 143-170. It writes a peer memory as `claude-code` / `sess_peer_writer`, asserts the rendered delta uses that writer rather than local/default identity, then asserts the peer activity audit stores `from_harness = claude-code` and `from_session_id = sess_peer_writer`.

- Cooldown for same receiving session, not other receiving session is covered by `peer_update_cooldown_is_per_receiving_session_in_daemon_ram` at `coordination_integration.rs` lines 172-192. It asserts the first delivery appears, the second delivery to the same receiver is suppressed, and delivery to `sess_other_receiver` still appears.

- Entity-overlap, path-overlap presence filtering, and no-overlap suppression are covered in `coordination_integration.rs`:
  - Entity overlap: `level3_presence_requires_salient_entity_or_path_overlap` at lines 73-97 includes the overlapping `claude-code` session and suppresses unrelated `cursor` / `unrel999`.
  - Path overlap without entity overlap: `level3_presence_renders_for_salient_path_overlap_without_entity_overlap` at lines 99-123 includes the path-overlap session and suppresses the no-path-overlap `cursor` / `nopth999` session.

- Startup writer attribution does not leak `source_device` is covered by `test_startup_peer_update_uses_writer_attribution_not_source_device` at `crates/memoryd/tests/startup_recall_mcp.rs` lines 149-174. It writes a startup peer update with `source_device = dev_startup` but writer identity `claude-code` / `sess_peer_writer`, then asserts the opening peer-update uses writer attribution and does not render the device as session identity.

- Retained peer CLI negative paths are covered in `crates/memoryd/tests/peer_cli.rs`:
  - CLI command surface parses status/activity/release-lock at lines 18-25.
  - Release-lock no-lock negative path returns `NoLockFound` at lines 78-86.
  - Peer commands remain excluded from MCP manifest and local MCP forwarding rejects them with `method_not_allowed_on_mcp` at lines 101-112 and following assertions in the same test.

- Retained claim-lock negative paths are covered in `crates/memoryd/tests/claim_lock_supersede.rs`:
  - Governance refusal does not get replaced by claim-lock warning at lines 137-166.
  - Secret-looking session identity rejects before contention logging at lines 168-189.
  - Oversized harness identity rejects at lines 191-211.
  - Post-acquire supersede failure restores previous claim-lock holder at lines 213-238.
  - Contention event emission remains covered at lines 113-135.

## Worktree note

The repository already had a large dirty worktree before this review pass, including modified production files and untracked Stream I test/review files. I did not attempt to normalize or revert unrelated state.

## Remaining risk

No blocking test risk found in the requested focused gate. This pass did not run broader workspace gates (`cargo fmt`, `cargo clippy`, or full `cargo test`) because the task requested the focused rerun command only.
