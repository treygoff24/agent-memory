use std::collections::HashSet;

use chrono::{TimeZone, Utc};
use memorum_coordination::{ClaimLockInfo, CoordinationInsertion, PeerPresenceEntry, PeerUpdateEntry};
use memoryd::recall::{
    estimated_tokens, render_delta_frame, render_startup_frame_with_coordination,
    render_startup_frame_with_cross_device_updates, CrossDeviceStartupUpdates, DeltaRecallItem, RecallExplanation,
    SessionBinding, StartupCoordinationRender,
};

#[test]
fn test_no_coordination_insertion_emits_unchanged_delta() {
    let rendered = render_delta_frame(&[delta_item()], 400, None);

    assert_eq!(rendered.block, "<memory-delta>\n  <item id=\"mem_item\">normal recall body</item>\n</memory-delta>\n");
    assert!(!rendered.block.contains("coordination="));
    assert!(!rendered.block.contains("<peer-update"));
    assert!(!rendered.block.contains("<peer-presence"));
}

#[test]
fn test_peer_update_inserted_in_delta() {
    let rendered = render_delta_frame(&[delta_item()], 400, Some(&coordination_with_update()));

    assert!(rendered.block.starts_with("<memory-delta coordination=\"stream-i-v0.1\">\n"));
    assert!(rendered
        .block
        .contains("<peer-update from=\"codex\" session=\"abcdefgh\" ts=\"15:23\" relevance=\"0.84\">"));
    assert!(rendered.block.contains("<summary>Migrated users.email to CITEXT.</summary>"));
    assert!(rendered.block.contains("<ref>mem_peer</ref>"));
    assert!(rendered.block.contains("<namespace>project:agent-memory</namespace>"));
}

#[test]
fn test_peer_update_attribute_shape() {
    let rendered = render_delta_frame(&[], 400, Some(&coordination_with_update()));

    assert!(rendered.block.contains("from=\"codex\""));
    assert!(rendered.block.contains("session=\"abcdefgh\""));
    assert!(rendered.block.contains("ts=\"15:23\""));
    assert!(rendered.block.contains("relevance=\"0.84\""));
    assert!(!rendered.block.contains("abcdefghi"));
}

#[test]
fn test_peer_presence_absent_at_level2() {
    let rendered = render_delta_frame(&[], 400, Some(&coordination_with_update()));

    assert!(!rendered.block.contains("<peer-presence"));
}

#[test]
fn test_peer_presence_emitted_at_level3() {
    let mut insertion = coordination_with_update();
    insertion.peer_presence.push(PeerPresenceEntry {
        harness: "claude-code".to_string(),
        session_id: "presence123456".to_string(),
        salient_entities: vec![
            "ent_users_table".to_string(),
            "ent_agent_memory".to_string(),
            "ent_extra_1".to_string(),
            "ent_extra_2".to_string(),
            "ent_extra_3".to_string(),
            "ent_extra_4".to_string(),
        ],
        started_at: Utc.with_ymd_and_hms(2026, 5, 1, 14, 2, 0).unwrap(),
    });

    let rendered = render_delta_frame(&[], 400, Some(&insertion));

    assert_in_order(
        &rendered.block,
        &["<peer-presence>", "<session harness=\"claude-code\"", "</peer-presence>", "<peer-update"],
    );
    assert!(rendered.block.contains("id=\"presen\""));
    assert!(rendered
        .block
        .contains("entities=\"ent_users_table,ent_agent_memory,ent_extra_1,ent_extra_2,ent_extra_3\""));
    assert!(!rendered.block.contains("ent_extra_4"));
    assert!(rendered.block.contains("started=\"14:02\""));
}

#[test]
fn test_summary_privacy_filtered() {
    let mut insertion = coordination_with_update();
    insertion.peer_updates[0].summary = "Email trey@example.com before launch.".to_string();

    let rendered = render_delta_frame(&[], 400, Some(&insertion));

    assert!(rendered.block.contains("<summary>[content not available — privacy classification pending]</summary>"));
    assert!(!rendered.block.contains("trey@example.com"));
}

#[test]
fn test_coordination_attribute_on_delta() {
    let without_entries = render_delta_frame(&[], 400, Some(&CoordinationInsertion::empty()));
    let with_entries = render_delta_frame(&[], 400, Some(&coordination_with_update()));

    assert_eq!(without_entries.block, "<memory-delta empty=\"true\" />\n");
    assert!(with_entries.block.starts_with("<memory-delta coordination=\"stream-i-v0.1\">"));
}

#[test]
fn test_capped_peer_updates_added_to_pending_attention() {
    let mut insertion = coordination_with_update();
    insertion.capped_peer_updates = 2;

    let rendered = render_delta_frame(&[], 400, Some(&insertion));

    assert!(rendered
        .block
        .contains("<item kind=\"coordination_overflow\" count=\"2\">2 coordination update(s) omitted by cap.</item>"));
}

#[test]
fn test_claim_locked_attribute() {
    let mut insertion = coordination_with_update();
    insertion.peer_updates[0].claim_locked = Some(ClaimLockInfo {
        memory_id: "mem_peer".to_string(),
        holder_harness: "claude-code".to_string(),
        holder_session_id: "sess_def567".to_string(),
        expires_at: Utc.with_ymd_and_hms(2026, 5, 1, 15, 30, 0).unwrap(),
    });

    let rendered = render_delta_frame(&[], 400, Some(&insertion));

    assert!(rendered.block.contains("claim_locked=\"claude-code:sess_def567\""));
}

#[test]
fn test_budget_accounting_peer_update_bytes() {
    let coordination = coordination_with_update();
    let peer_only = render_delta_frame(&[], 400, Some(&coordination));
    let item_only = render_delta_frame(&[delta_item()], 400, None);
    let combined_budget = peer_only.budget_used_tokens + item_only.budget_used_tokens - 1;

    let rendered = render_delta_frame(&[delta_item()], combined_budget, Some(&coordination));

    assert!(rendered.block.contains("<peer-update"));
    assert!(!rendered.block.contains("<item id=\"mem_item\""));
    assert_eq!(rendered.included_item_ids, Vec::<String>::new());
    assert!(rendered.budget_used_tokens <= combined_budget);
}

#[test]
fn startup_renders_peer_updates_but_never_peer_presence() {
    let mut insertion = coordination_with_update();
    insertion.peer_presence.push(PeerPresenceEntry {
        harness: "codex".to_string(),
        session_id: "present".to_string(),
        salient_entities: vec!["ent_a".to_string()],
        started_at: Utc.with_ymd_and_hms(2026, 5, 1, 14, 2, 0).unwrap(),
    });

    let rendered = render_startup_frame_with_coordination(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        Some(&insertion),
    );

    assert!(rendered.starts_with("<memory-recall version=\"stream-e-v0.6\" harness=\"codex\" session=\"sess_current\" coordination=\"stream-i-v0.1\">"));
    assert_in_order(&rendered, &["<entity-recall", "<peer-update", "</entity-recall>"]);
    assert!(!rendered.contains("<peer-presence"));
}

#[test]
fn startup_without_coordination_preserves_existing_root_shape() {
    let rendered =
        render_startup_frame_with_coordination(&session_binding(), &RecallExplanation::empty(3600), &[], None);

    assert!(
        rendered.starts_with("<memory-recall version=\"stream-e-v0.6\" harness=\"codex\" session=\"sess_current\">")
    );
    assert!(!rendered.contains("coordination="));
}

#[test]
fn startup_renders_cross_device_updates_separately_with_device_other() {
    let same_device = coordination_with_update();
    let cross_device = CrossDeviceStartupUpdates {
        from_sync_date: "2026-05-01".to_string(),
        peer_updates: vec![PeerUpdateEntry {
            harness: "codex".to_string(),
            session_id: "otherdevice123456".to_string(),
            timestamp: Utc.with_ymd_and_hms(2026, 5, 1, 9, 45, 0).unwrap(),
            relevance: 0.781,
            summary: "Renamed AuthService to OAuthProvider.".to_string(),
            reference: "mem_cross".to_string(),
            namespace: "project:agent-memory".to_string(),
            claim_locked: None,
            device: None,
        }],
    };
    let rendered = render_startup_frame_with_cross_device_updates(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        StartupCoordinationRender {
            same_device: Some(&same_device),
            cross_device: Some(&cross_device),
            salient_entities: None,
        },
    );

    assert!(rendered.starts_with("<memory-recall version=\"stream-e-v0.6\" harness=\"codex\" session=\"sess_current\" coordination=\"stream-i-v0.1\">"));
    assert_in_order(&rendered, &["<entity-recall", "<peer-update", "<cross-device-updates", "</entity-recall>"]);
    assert!(rendered.contains("<cross-device-updates from-sync=\"2026-05-01\">"));
    assert!(rendered.contains(
        "<peer-update from=\"codex\" session=\"otherdev\" ts=\"09:45\" relevance=\"0.78\" device=\"other\">"
    ));
    assert!(rendered.contains("<ref>mem_cross</ref>"));
    assert!(!rendered.contains("<peer-presence"));
}

fn coordination_with_update() -> CoordinationInsertion {
    CoordinationInsertion {
        peer_updates: vec![PeerUpdateEntry {
            harness: "codex".to_string(),
            session_id: "abcdefghijklmnop".to_string(),
            timestamp: Utc.with_ymd_and_hms(2026, 5, 1, 15, 23, 0).unwrap(),
            relevance: 0.836,
            summary: "Migrated users.email to CITEXT.".to_string(),
            reference: "mem_peer".to_string(),
            namespace: "project:agent-memory".to_string(),
            claim_locked: None,
            device: None,
        }],
        peer_presence: Vec::new(),
        capped_peer_updates: 0,
        capped_peer_presence: 0,
    }
}

fn delta_item() -> DeltaRecallItem {
    DeltaRecallItem { id: "mem_item".to_string(), text: "normal recall body".to_string() }
}

fn session_binding() -> SessionBinding {
    SessionBinding {
        session_id: "sess_current".to_string(),
        harness: "codex".to_string(),
        harness_version: None,
        cwd: "/repo".to_string(),
        project: None,
        namespaces_in_scope: vec!["project:agent-memory".to_string()],
    }
}

fn assert_in_order(value: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let position = value[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing expected fragment {needle:?} after byte {cursor}"));
        cursor += position + needle.len();
    }
}

#[test]
fn estimated_token_fixture_sanity() {
    assert_eq!(estimated_tokens("abcde"), 2);
}

/// When no salient entities are provided, the attribute is emitted empty.
#[test]
fn entity_recall_entities_attr_empty_when_no_salients() {
    let rendered = render_startup_frame_with_cross_device_updates(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        StartupCoordinationRender { same_device: None, cross_device: None, salient_entities: None },
    );

    assert!(rendered.contains("<entity-recall entities=\"\">"), "expected empty entities attr, got:\n{rendered}");
}

/// When salient entities are present, the attribute is populated with a
/// comma-separated, lexicographically sorted, XML-escaped list.
#[test]
fn entity_recall_entities_attr_populated_from_salients() {
    // Use "bravo" before "alpha" in the HashSet to verify sorting.
    let salients: HashSet<String> = ["bravo".to_string(), "alpha".to_string()].into();

    let rendered = render_startup_frame_with_cross_device_updates(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        StartupCoordinationRender { same_device: None, cross_device: None, salient_entities: Some(&salients) },
    );

    // Sorted: alpha comes before bravo.
    assert!(
        rendered.contains("<entity-recall entities=\"alpha,bravo\">"),
        "expected sorted entities attr, got:\n{rendered}"
    );
}

/// Entities with XML-special characters in their ids are properly escaped in
/// the attribute value.
#[test]
fn entity_recall_entities_attr_xml_escaped() {
    let salients: HashSet<String> = ["ent&special".to_string(), "ent\"quoted".to_string()].into();

    let rendered = render_startup_frame_with_cross_device_updates(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        StartupCoordinationRender { same_device: None, cross_device: None, salient_entities: Some(&salients) },
    );

    // Both ids must be escaped; raw & or " must not appear inside the attribute value.
    assert!(!rendered.contains("ent&special"), "raw & must be escaped");
    assert!(!rendered.contains("ent\"quoted"), "raw \" must be escaped");
    assert!(rendered.contains("ent&amp;special"), "& must be escaped to &amp;");
    assert!(rendered.contains("ent&quot;quoted"), "\" must be escaped to &quot;");
}

/// Two calls with the same entity set must produce identical attribute values —
/// confirms that `HashSet` iteration order is not leaking into the output.
#[test]
fn entity_recall_entities_attr_is_deterministic() {
    let salients: HashSet<String> =
        ["ent_c".to_string(), "ent_a".to_string(), "ent_b".to_string(), "ent_d".to_string()].into();

    let first = render_startup_frame_with_cross_device_updates(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        StartupCoordinationRender { same_device: None, cross_device: None, salient_entities: Some(&salients) },
    );
    let second = render_startup_frame_with_cross_device_updates(
        &session_binding(),
        &RecallExplanation::empty(3600),
        &[],
        StartupCoordinationRender { same_device: None, cross_device: None, salient_entities: Some(&salients) },
    );

    assert_eq!(first, second, "entity-recall attribute must be deterministic across calls");
    assert!(
        first.contains("<entity-recall entities=\"ent_a,ent_b,ent_c,ent_d\">"),
        "entities must be sorted lexicographically"
    );
}
