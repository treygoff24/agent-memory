use chrono::{DateTime, Duration, TimeZone, Utc};
use memorum_coordination::gate::{path_fraction, score, CandidateEmbedding, PeerWriteCandidate};
use memorum_coordination::{
    CoordinationConfig, CoordinationInsertion, PeerUpdateEntry, QueryEmbedding, RelevanceGate, SessionContext,
};
use memory_substrate::{
    EmbeddingTriple, Entity, MemoryId, MemoryStatus, RecallIndexRow, RepoPath, Scope, Sensitivity, SourceKind,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn test_score_entity_overlap_only() {
    let session = session_with_entities(["ent_a", "ent_b"]);
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000001", ["ent_a", "ent_b"], []);

    assert_eq!(score(&candidate, &session), 0.5);
}

#[test]
fn test_score_path_overlap_only() {
    let mut session = tier1_session();
    session.salient_paths = set(["project:proj/decision.md"]);
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000002", [], ["project:proj/decision.md"]);

    assert_eq!(score(&candidate, &session), 0.3);
}

#[test]
fn test_score_all_components() {
    let mut session = session_with_entities(["ent_a", "ent_b"]);
    session.salient_paths = set(["project:proj/a.md", "project:proj/c.md"]);
    session.recent_query_embedding = Some(query_embedding(default_triple(), vec![1.0, 0.0]));
    let mut candidate =
        candidate("mem_20260501_a1b2c3d4e5f60718_000003", ["ent_a"], ["project:proj/a.md", "project:proj/b.md"]);
    candidate.embedding = Some(candidate_embedding(default_triple(), vec![1.0, 0.0]));

    let expected = 0.5 * 0.5 + 0.3 * 0.5 + 0.2 * 1.0;
    assert!((score(&candidate, &session) - expected).abs() <= f64::EPSILON);
}

#[test]
fn test_threshold_boundary() {
    let mut session = session_with_entities(["ent_a", "ent_b", "ent_c", "ent_d", "ent_e"]);
    session.salient_paths = set(["project:proj/matched.md"]);
    session.recent_query_embedding = Some(query_embedding(default_triple(), vec![0.9995, 0.031_618_82]));
    let at_threshold =
        candidate("mem_20260501_a1b2c3d4e5f60718_000004", ["ent_a", "ent_b", "ent_c"], ["project:proj/matched.md"]);
    let mut below_threshold = candidate("mem_20260501_a1b2c3d4e5f60718_000005", ["ent_a"], ["project:proj/matched.md"]);
    below_threshold.embedding = Some(candidate_embedding(default_triple(), vec![1.0, 0.0]));

    assert_eq!(score(&at_threshold, &session), 0.6);
    assert!((score(&below_threshold, &session) - 0.5999).abs() < 0.000_001);

    let insertion = gate().evaluate(&mut session, &[at_threshold, below_threshold], fixture_now());

    assert_eq!(peer_update_ids(&insertion.peer_updates), ["mem_20260501_a1b2c3d4e5f60718_000004"]);
    assert_eq!(insertion.capped_peer_updates, 0);
}

#[test]
fn test_per_turn_cap() {
    let mut session = session_with_entities(["ent_a"]);
    session.salient_paths = set(["project:proj/shared.md"]);
    let now = fixture_now();
    let candidates = vec![
        candidate_at("mem_20260501_a1b2c3d4e5f60718_000010", ["ent_a"], ["project:proj/shared.md"], times(now, now)),
        candidate_at(
            "mem_20260501_a1b2c3d4e5f60718_000011",
            ["ent_a"],
            ["project:proj/shared.md"],
            times(now + Duration::seconds(10), now),
        ),
        candidate_at("mem_20260501_a1b2c3d4e5f60718_000009", ["ent_a"], ["project:proj/shared.md"], times(now, now)),
        candidate_at("mem_20260501_a1b2c3d4e5f60718_000012", ["ent_a"], ["project:proj/shared.md"], times(now, now)),
        candidate_at("mem_20260501_a1b2c3d4e5f60718_000013", ["ent_a"], ["project:proj/shared.md"], times(now, now)),
    ];

    let insertion = gate().evaluate(&mut session, &candidates, now);

    assert_eq!(
        peer_update_ids(&insertion.peer_updates),
        ["mem_20260501_a1b2c3d4e5f60718_000011", "mem_20260501_a1b2c3d4e5f60718_000009"]
    );
    assert_eq!(insertion.capped_peer_updates, 3);
}

#[test]
fn test_cool_down() {
    let mut session = session_with_entities(["ent_a"]);
    session.salient_paths = set(["project:proj/shared.md"]);
    session.surfaced_peer_writes.insert("mem_20260501_a1b2c3d4e5f60718_000014".to_string());
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000014", ["ent_a"], ["project:proj/shared.md"]);

    let insertion = gate().evaluate(&mut session, &[candidate], fixture_now());

    assert!(insertion.peer_updates.is_empty());
    assert_eq!(insertion.capped_peer_updates, 0);
}

#[test]
fn test_evaluate_records_cooldown_for_selected_peer_write() {
    let mut session = session_with_entities(["ent_a"]);
    session.salient_paths = set(["project:proj/shared.md"]);
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000024", ["ent_a"], ["project:proj/shared.md"]);

    let first_insertion = gate().evaluate(&mut session, std::slice::from_ref(&candidate), fixture_now());
    let second_insertion = gate().evaluate(&mut session, &[candidate], fixture_now());

    assert_eq!(peer_update_ids(&first_insertion.peer_updates), ["mem_20260501_a1b2c3d4e5f60718_000024"]);
    assert!(second_insertion.peer_updates.is_empty());
    assert!(session.has_surfaced_peer_write("mem_20260501_a1b2c3d4e5f60718_000024"));
}

#[test]
fn test_peer_update_reference_is_memory_id_not_namespace_path() {
    let mut session = session_with_entities(["ent_a"]);
    session.salient_paths = set(["project:proj/shared.md"]);
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000025", ["ent_a"], ["project:proj/shared.md"]);

    let insertion = gate().evaluate(&mut session, &[candidate], fixture_now());
    let peer_update = insertion.peer_updates.first().expect("candidate should pass relevance gate");

    assert_eq!(peer_update.reference, "mem_20260501_a1b2c3d4e5f60718_000025");
    assert_eq!(peer_update.namespace, "project:proj");
    assert_ne!(peer_update.reference, "project:proj/mem_20260501_a1b2c3d4e5f60718_000025.md");
}

#[test]
fn test_same_device_peer_update_leaves_device_unset() {
    let mut session = session_with_entities(["ent_a"]);
    session.salient_paths = set(["project:proj/shared.md"]);
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000026", ["ent_a"], ["project:proj/shared.md"]);

    let insertion = gate().evaluate(&mut session, &[candidate], fixture_now());

    assert_eq!(insertion.peer_updates.first().and_then(|entry| entry.device.as_deref()), None);
}

#[test]
fn test_recency_window_uses_recall_indexed_at_not_updated_at() {
    let mut session = session_with_entities(["ent_a"]);
    session.salient_paths = set(["project:proj/shared.md"]);
    let now = fixture_now();
    let stale_by_indexed_at = candidate_at(
        "mem_20260501_a1b2c3d4e5f60718_000015",
        ["ent_a"],
        ["project:proj/shared.md"],
        times(now, now - Duration::minutes(31)),
    );
    let fresh_by_indexed_at = candidate_at(
        "mem_20260501_a1b2c3d4e5f60718_000016",
        ["ent_a"],
        ["project:proj/shared.md"],
        times(now - Duration::hours(2), now - Duration::minutes(29)),
    );

    let insertion = gate().evaluate(&mut session, &[stale_by_indexed_at, fresh_by_indexed_at], now);

    assert_eq!(peer_update_ids(&insertion.peer_updates), ["mem_20260501_a1b2c3d4e5f60718_000016"]);
}

#[test]
fn test_tier3_returns_empty_no_scoring() {
    let mut tier1_session = session_with_entities(["ent_a"]);
    tier1_session.salient_paths = set(["project:proj/shared.md"]);
    let mut tier3_session = tier1_session.clone();
    tier3_session.harness = "cursor".to_string();
    let candidates = high_scoring_candidates();
    let now = fixture_now();

    let tier1_insertion = gate().evaluate(&mut tier1_session, &candidates, now);
    let tier3_insertion = gate().evaluate(&mut tier3_session, &candidates, now);

    assert_eq!(tier1_insertion.peer_updates.len(), CoordinationConfig::default().relevance_gate.per_turn_cap);
    assert_eq!(tier3_insertion, CoordinationInsertion::empty());
    assert!(tier3_session.surfaced_peer_writes.is_empty());
}

#[test]
fn test_tier3_returns_before_scorer_spy_is_called() {
    let mut tier3_session = session_with_entities(["ent_a"]);
    tier3_session.harness = "cursor".to_string();
    tier3_session.salient_paths = set(["project:proj/shared.md"]);
    let candidates = high_scoring_candidates();
    let calls = AtomicUsize::new(0);
    let mut scorer = |candidate: &PeerWriteCandidate,
                      session: &SessionContext,
                      embedding: Option<&memorum_coordination::QueryEmbedding>| {
        calls.fetch_add(1, Ordering::SeqCst);
        memorum_coordination::gate::score(candidate, session) + embedding.map(|_| 0.0).unwrap_or_default()
    };

    let insertion = gate().evaluate_with_scorer(&mut tier3_session, &candidates, fixture_now(), &mut scorer);

    assert_eq!(insertion, CoordinationInsertion::empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn test_entity_overlap_required_property() {
    let mut session = tier1_session();
    session.salient_paths = set(["project:proj/shared.md"]);
    session.recent_query_embedding = Some(query_embedding(default_triple(), vec![1.0, 0.0]));
    let mut candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000018", [], ["project:proj/shared.md"]);
    candidate.embedding = Some(candidate_embedding(default_triple(), vec![1.0, 0.0]));

    assert_eq!(score(&candidate, &session), 0.5);
    assert!(gate().evaluate(&mut session, &[candidate], fixture_now()).peer_updates.is_empty());
}

#[test]
fn test_empty_entity_sets() {
    let session = tier1_session();
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000019", [], []);

    assert_eq!(score(&candidate, &session), 0.0);
}

#[test]
fn test_embedding_triple_mismatch() {
    let mut session = session_with_entities(["ent_a"]);
    session.recent_query_embedding = Some(query_embedding(default_triple(), vec![1.0, 0.0]));
    let mut candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000020", ["ent_a"], []);
    candidate.embedding = Some(candidate_embedding(
        EmbeddingTriple { provider: "local".to_string(), model_ref: "different".to_string(), dimension: 2 },
        vec![1.0, 0.0],
    ));

    assert_eq!(score(&candidate, &session), 0.5);
}

#[test]
fn test_embedding_cache_hit_uses_cached_value() {
    let mut session = session_with_entities(["ent_a"]);
    session.set_recent_query_message_hash("hash_current");
    session.cache_query_embedding("hash_current", query_embedding(default_triple(), vec![1.0, 0.0]));
    let mut candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000021", ["ent_a"], []);
    candidate.embedding = Some(candidate_embedding(default_triple(), vec![1.0, 0.0]));

    assert_eq!(score(&candidate, &session), 0.7);
    assert_eq!(gate().evaluate(&mut session, &[candidate], fixture_now()).peer_updates.len(), 1);
}

#[test]
fn test_embedding_cache_miss_yields_zero_topic() {
    let mut session = session_with_entities(["ent_a"]);
    session.set_recent_query_message_hash("hash_backlogged");
    let mut candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000022", ["ent_a"], []);
    candidate.embedding = Some(candidate_embedding(default_triple(), vec![1.0, 0.0]));

    assert_eq!(score(&candidate, &session), 0.5);
    assert!(gate().evaluate(&mut session, &[candidate], fixture_now()).peer_updates.is_empty());
}

#[test]
fn test_embedding_triple_mismatch_yields_zero() {
    let mut session = session_with_entities(["ent_a"]);
    session.set_recent_query_message_hash("hash_current");
    session.cache_query_embedding("hash_current", query_embedding(default_triple(), vec![1.0, 0.0]));
    let mut candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000023", ["ent_a"], []);
    candidate.embedding = Some(candidate_embedding(
        EmbeddingTriple { provider: "local".to_string(), model_ref: "rotated".to_string(), dimension: 2 },
        vec![1.0, 0.0],
    ));

    assert_eq!(score(&candidate, &session), 0.5);
    assert!(gate().evaluate(&mut session, &[candidate], fixture_now()).peer_updates.is_empty());
}

// path_fraction prefix-matching tests

/// Session directory covers a file inside it (session prefix → candidate file).
#[test]
fn path_fraction_directory_prefix_covers_file_inside() {
    let session_paths = set(["/src/auth"]);
    let candidate_paths = ["/src/auth/service.rs".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 1.0);
}

/// Non-boundary prefix must NOT match: `/src/auth` must not cover
/// `/src/authentication.rs` even though the string starts with `/src/auth`.
#[test]
fn path_fraction_non_boundary_prefix_does_not_match() {
    let session_paths = set(["/src/auth"]);
    let candidate_paths = ["/src/authentication.rs".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 0.0);
}

#[test]
fn path_fraction_exact_match_still_works() {
    let session_paths = set(["/src/auth/service.rs"]);
    let candidate_paths = ["/src/auth/service.rs".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 1.0);
}

/// Reverse direction: session is focused on a specific file, candidate path is
/// the parent directory. Bidirectional prefix matching: the candidate directory
/// is an ancestor of (i.e. a prefix of) the session file path, so they
/// intersect.
#[test]
fn path_fraction_reverse_direction_candidate_parent_of_session_file() {
    let session_paths = set(["/src/auth/service.rs"]);
    let candidate_paths = ["/src/auth".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 1.0);
}

/// Only the covered fraction counts when not all paths match.
#[test]
fn path_fraction_partial_coverage() {
    let session_paths = set(["/src/auth"]);
    let candidate_paths = ["/src/auth/service.rs".to_string(), "/src/payments/handler.rs".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 0.5);
}

/// Empty candidate paths returns 0.0 (unchanged baseline).
#[test]
fn path_fraction_empty_candidates_returns_zero() {
    let session_paths = set(["/src/auth"]);
    assert_eq!(path_fraction(&[], &session_paths), 0.0);
}

fn session_with_entities<const N: usize>(entities: [&str; N]) -> SessionContext {
    let mut session = tier1_session();
    session.salient_entities = entities.into_iter().map(String::from).collect();
    session
}

fn tier1_session() -> SessionContext {
    SessionContext { session_id: "sess_current".to_string(), harness: "codex".to_string(), ..SessionContext::default() }
}

fn gate() -> RelevanceGate {
    RelevanceGate::new(CoordinationConfig::default())
}

fn candidate<const E: usize, const P: usize>(id: &str, entities: [&str; E], paths: [&str; P]) -> PeerWriteCandidate {
    let now = fixture_now();
    candidate_at(id, entities, paths, times(now, now))
}

fn high_scoring_candidates() -> Vec<PeerWriteCandidate> {
    (30..40)
        .map(|suffix| {
            candidate(&format!("mem_20260501_a1b2c3d4e5f60718_{suffix:06}"), ["ent_a"], ["project:proj/shared.md"])
        })
        .collect()
}

fn candidate_at<const E: usize, const P: usize>(
    id: &str,
    entities: [&str; E],
    paths: [&str; P],
    times: CandidateTimes,
) -> PeerWriteCandidate {
    PeerWriteCandidate {
        memory_id: MemoryId::new(id),
        row: row(id, entities, &format!("project:proj/{id}.md"), times),
        paths: paths.into_iter().map(String::from).collect(),
        harness: "claude-code".to_string(),
        session_id: "peer_session".to_string(),
        namespace: "project:proj".to_string(),
        embedding: None,
    }
}

#[derive(Clone, Copy)]
struct CandidateTimes {
    updated_at: DateTime<Utc>,
    indexed_at: DateTime<Utc>,
}

fn times(updated_at: DateTime<Utc>, indexed_at: DateTime<Utc>) -> CandidateTimes {
    CandidateTimes { updated_at, indexed_at }
}

fn row<const N: usize>(id: &str, entity_ids: [&str; N], path: &str, times: CandidateTimes) -> RecallIndexRow {
    RecallIndexRow {
        id: MemoryId::new(id),
        path: RepoPath::from_unchecked(path),
        summary: format!("summary for {id}"),
        status: MemoryStatus::Active,
        scope: Scope::Project,
        canonical_namespace_id: Some("proj".to_string()),
        updated_at: times.updated_at,
        indexed_at: times.indexed_at,
        confidence: 1.0,
        source_kind: SourceKind::AgentPrimary,
        source_device: Some("device_peer".to_string()),
        source_harness: None,
        source_session_id: None,
        author_harness: None,
        author_session_id: None,
        sensitivity: Sensitivity::Internal,
        passive_recall: true,
        index_body: true,
        requires_user_confirmation: false,
        review_state: None,
        human_review_required: false,
        max_scope: Scope::Project,
        merge_diagnostics_json: None,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: entity_ids
            .into_iter()
            .map(|id| Entity { id: id.to_string(), label: id.to_string(), aliases: Vec::new() })
            .collect(),
    }
}

fn fixture_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap()
}

fn set<const N: usize>(values: [&str; N]) -> std::collections::HashSet<String> {
    values.into_iter().map(String::from).collect()
}

fn default_triple() -> EmbeddingTriple {
    EmbeddingTriple { provider: "local".to_string(), model_ref: "test-embed".to_string(), dimension: 2 }
}

fn query_embedding(triple: EmbeddingTriple, vector: Vec<f32>) -> QueryEmbedding {
    QueryEmbedding { triple, vector }
}

fn candidate_embedding(triple: EmbeddingTriple, vector: Vec<f32>) -> CandidateEmbedding {
    CandidateEmbedding { triple, vector }
}

fn peer_update_ids(entries: &[PeerUpdateEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.reference.clone()).collect()
}

/// Simulates the two-pass startup recall flow (same-device → cross-device).
///
/// If a memory id is surfaced in pass 1 (same-device), it must be suppressed
/// in pass 2 (cross-device) even though the two passes operate over different
/// candidate slices.  The fix: seed pass 2's `SessionContext` clone with the
/// ids surfaced by pass 1 before evaluating.
///
/// This test exercises the gate-level mechanism directly without requiring a
/// Substrate (the startup.rs fix wires this for production; the gate itself is
/// the right unit to test for the cool-down contract).
#[test]
fn cross_device_pass_suppresses_ids_already_surfaced_in_same_device_pass() {
    let now = fixture_now();
    let mut base_session = session_with_entities(["ent_shared"]);
    base_session.salient_paths = set(["project:proj/shared.md"]);

    // Pass 1 — same-device candidates.  One passes the gate.
    let same_device_candidate =
        candidate("mem_20260501_a1b2c3d4e5f60718_000500", ["ent_shared"], ["project:proj/shared.md"]);

    let pass1_insertion = gate().evaluate(&mut base_session, &[same_device_candidate], now);
    assert_eq!(
        peer_update_ids(&pass1_insertion.peer_updates),
        ["mem_20260501_a1b2c3d4e5f60718_000500"],
        "pass 1: same-device candidate must be surfaced"
    );

    // Simulate what startup.rs does: extract surfaced ids from pass 1's result
    // and seed a fresh session clone for pass 2.
    let mut pass2_session = base_session.clone();
    // NOTE: base_session already has the id recorded (gate mutates it), but we
    // explicitly seed pass2_session to mirror the startup.rs fix where the
    // caller must thread the surfaced set explicitly.
    for id in pass1_insertion.peer_updates.iter().map(|u| u.reference.clone()) {
        pass2_session.record_surfaced_peer_write(id);
    }

    // Pass 2 — cross-device candidates include the same memory id plus a novel one.
    let same_id_cross_device =
        candidate("mem_20260501_a1b2c3d4e5f60718_000500", ["ent_shared"], ["project:proj/shared.md"]);
    let novel_cross_device =
        candidate("mem_20260501_a1b2c3d4e5f60718_000501", ["ent_shared"], ["project:proj/shared.md"]);

    let pass2_insertion = gate().evaluate(&mut pass2_session, &[same_id_cross_device, novel_cross_device], now);

    // The id surfaced in pass 1 must be suppressed; only the novel id appears.
    assert_eq!(
        peer_update_ids(&pass2_insertion.peer_updates),
        ["mem_20260501_a1b2c3d4e5f60718_000501"],
        "pass 2: id already surfaced in pass 1 must be suppressed by cool-down"
    );
}

/// Without cross-pass seeding, the same id appears in both passes — confirming
/// that the shared-cool-down fix is load-bearing (not accidentally a no-op).
#[test]
fn without_cooldown_seeding_same_id_appears_in_both_passes() {
    let now = fixture_now();

    let mut pass1_session = session_with_entities(["ent_shared"]);
    pass1_session.salient_paths = set(["project:proj/shared.md"]);
    let mut pass2_session = pass1_session.clone(); // NOT seeded with pass 1 surfaced ids

    let same_id = "mem_20260501_a1b2c3d4e5f60718_000502";
    let pass1_candidate = candidate(same_id, ["ent_shared"], ["project:proj/shared.md"]);
    let pass2_candidate = candidate(same_id, ["ent_shared"], ["project:proj/shared.md"]);

    let pass1_insertion = gate().evaluate(&mut pass1_session, &[pass1_candidate], now);
    let pass2_insertion = gate().evaluate(&mut pass2_session, &[pass2_candidate], now);

    // Without seeding: both passes independently surface the same id.
    assert_eq!(peer_update_ids(&pass1_insertion.peer_updates), [same_id]);
    assert_eq!(peer_update_ids(&pass2_insertion.peer_updates), [same_id], "unseeded pass 2 duplicates pass 1 output");
}
