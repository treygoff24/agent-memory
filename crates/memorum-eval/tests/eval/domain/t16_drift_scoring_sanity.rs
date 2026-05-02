use std::path::Path;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::support::daemon_request;

const STREAM_G_RC_HANDLER_NOT_SHIPPED: &str = "STREAM_G_RC_HANDLER_NOT_SHIPPED";
const MEMORYD_TEST_UTILS_NOT_ENABLED: &str = "MEMORYD_TEST_UTILS_NOT_ENABLED";
const SCORE_TOLERANCE: f64 = 1e-9;
const COMPONENT_TOLERANCE: f64 = 0.02;

#[tokio::test]
async fn t16_reality_check_drift_scores_order_and_explain_components() {
    let scaffold = DaemonScaffold::fresh().await;
    let probe = list_reality_check_items(scaffold.socket_path());
    if stream_g_reality_check_handler_not_shipped(&probe) {
        println!(
            "MEMORUM_EVAL_SKIP:{STREAM_G_RC_HANDLER_NOT_SHIPPED}: RealityCheck(List) returned `{}`; \
             Stream G Task 7 has not wired the runtime handler yet, so Test #16 is semantically skipped.",
            protocol_error_code(&probe).unwrap_or("unknown_error")
        );
        return;
    }
    assert_success(&probe, "RealityCheck(List) probe should either succeed or trigger the Stream G skip guard");

    let memories = DriftFixtureIds {
        fresh_recalled_correlated: write_memory(
            scaffold.socket_path(),
            "T16 Memory A fresh recalled corroborated",
            0.95,
        ),
        stale_unrecalled_sensitive: write_memory(
            scaffold.socket_path(),
            "T16 Memory B stale unrecalled sensitive",
            0.70,
        ),
        midrange_some_recalls: write_memory(scaffold.socket_path(), "T16 Memory C midrange some recalls", 0.85),
    };
    if let Some(skip_reason) = seed_drift_inputs(scaffold.socket_path(), scaffold.tree_dir(), &memories) {
        println!(
            "MEMORUM_EVAL_SKIP:{skip_reason}: memoryd was compiled without the test-utils feature; \
                  TestInjectEvent is unavailable. Rebuild memoryd with --features test-utils."
        );
        return;
    }

    let response = list_reality_check_items(scaffold.socket_path());
    if stream_g_reality_check_handler_not_shipped(&response) {
        println!(
            "MEMORUM_EVAL_SKIP:{STREAM_G_RC_HANDLER_NOT_SHIPPED}: RealityCheck(List) became unavailable after fixture setup; \
             preserving the same dependency skip rather than reporting a false pass."
        );
        return;
    }

    let items = pending_items(&response);
    let memory_a = find_scored_item(items, &memories.fresh_recalled_correlated);
    let memory_b = find_scored_item(items, &memories.stale_unrecalled_sensitive);
    let memory_c = find_scored_item(items, &memories.midrange_some_recalls);

    let score_a = score(memory_a);
    let score_b = score(memory_b);
    let score_c = score(memory_c);
    eval_assert!(
        score_b > score_c && score_c > score_a,
        "expected strict drift ordering B > C > A, got A={score_a}, B={score_b}, C={score_c}\n{response:#?}"
    );
    eval_assert!(score_a <= 0.25, "Memory A should score low drift, got {score_a}: {memory_a:#?}");
    eval_assert!(score_b >= 0.65, "Memory B should score high drift, got {score_b}: {memory_b:#?}");
    eval_assert!(
        (0.25..0.65).contains(&score_c),
        "Memory C should score mid-range drift, got {score_c}: {memory_c:#?}"
    );

    assert_component_shape_and_values(
        memory_a,
        ExpectedComponents {
            days_since_observed_norm: Some(0.0),
            recall_frequency_norm: Some(1.0),
            cross_source_corroboration: Some(1.0),
            confidence_decay: Some(0.0),
            sensitivity_weight: Some(0.0),
        },
    );
    assert_component_shape_and_values(
        memory_b,
        ExpectedComponents {
            days_since_observed_norm: Some(1.0),
            recall_frequency_norm: Some(0.0),
            cross_source_corroboration: Some(0.0),
            confidence_decay: Some(0.25),
            sensitivity_weight: Some(1.0),
        },
    );
    assert_component_shape_and_values(
        memory_c,
        ExpectedComponents {
            days_since_observed_norm: Some(1.0 / 3.0),
            recall_frequency_norm: None,
            cross_source_corroboration: Some(0.0),
            confidence_decay: Some(0.10),
            sensitivity_weight: Some(0.3),
        },
    );
    let c_recall_frequency = component(memory_c, "recall_frequency_norm");
    eval_assert!(
        (0.0..1.0).contains(&c_recall_frequency),
        "Memory C should have a non-saturated recall-frequency component, got {c_recall_frequency}"
    );

    for item in [memory_a, memory_b, memory_c] {
        assert_weighted_sum(item);
    }
    eval_flush_assertion_count();
}

#[derive(Debug)]
struct DriftFixtureIds {
    fresh_recalled_correlated: String,
    stale_unrecalled_sensitive: String,
    midrange_some_recalls: String,
}

#[derive(Debug, Clone, Copy)]
struct ExpectedComponents {
    days_since_observed_norm: Option<f64>,
    recall_frequency_norm: Option<f64>,
    cross_source_corroboration: Option<f64>,
    confidence_decay: Option<f64>,
    sensitivity_weight: Option<f64>,
}

fn list_reality_check_items(socket_path: &Path) -> Value {
    daemon_request(socket_path, json!({"reality_check": {"list": {"namespace": null, "limit": 12}}}))
}

fn write_memory(socket_path: &Path, title: &str, confidence: f64) -> String {
    let response = daemon_request(
        socket_path,
        json!({
            "write_memory": {
                "body": format!("{title}. Synthetic non-PII drift scoring fixture."),
                "title": title,
                "tags": ["stream-h", "t16"],
                "meta": {
                    "namespace": "project",
                    "type": "claim",
                    "summary": title,
                    "confidence": confidence,
                    "source_kind": "user",
                    "source_ref": "t16-drift-scoring",
                    "explicit_user_context": true
                }
            }
        }),
    );
    eval_assert_eq!(
        response.pointer("/result/success/governance_write/status").and_then(Value::as_str),
        Some("promoted"),
        "T16 setup write should promote: {response:#?}"
    );
    response
        .pointer("/result/success/governance_write/id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("write response should include memory id: {response:#?}"))
        .to_owned()
}

/// Seed the drift-scoring fixture data.
///
/// Memory properties (observed_at, confidence, sensitivity) that cannot be set
/// through the daemon write API are set via direct rusqlite access. Event-log
/// entries are injected through the daemon's `TestInjectEvent` protocol surface
/// instead of raw SQL, honoring spec §4.2 and eliminating the external `sqlite3`
/// CLI dependency. (H-R1)
///
/// Returns `None` on success or `Some(skip_reason)` if the daemon was compiled
/// without `--features test-utils` and `TestInjectEvent` is unavailable.
fn seed_drift_inputs(socket_path: &Path, tree_dir: &Path, memories: &DriftFixtureIds) -> Option<&'static str> {
    let database_path = tree_dir.join(".memoryd/index.sqlite");
    let corroborating_id = format!("{}_corroborating_source", memories.fresh_recalled_correlated);

    // 1. Update memory properties that cannot be controlled through the write API.
    //    (observed_at, confidence delta, sensitivity tier, source_harness)
    //    Using rusqlite directly rather than spawning the sqlite3 CLI binary.
    update_memory_properties(
        &database_path,
        &memories.fresh_recalled_correlated,
        "now",
        0.95,
        0.95,
        "public",
        "claude-code",
    );
    update_memory_properties(
        &database_path,
        &memories.stale_unrecalled_sensitive,
        "now', '-95 days",
        0.95,
        0.70,
        "personal",
        "codex",
    );
    update_memory_properties(
        &database_path,
        &memories.midrange_some_recalls,
        "now', '-30 days",
        0.95,
        0.85,
        "internal",
        "claude-code",
    );

    // 2. Insert a superseded corroborating source for Memory A to set cross_source_corroboration = 1.
    insert_corroborating_source(&database_path, &memories.fresh_recalled_correlated, &corroborating_id);

    // 3. Inject recall-hit events through the daemon's TestInjectEvent protocol surface.
    //    This replaces the previous raw `events_log` INSERT via sqlite3 CLI. (H-R1)
    //
    //    Note: inject_recall_hits returns Some(skip_reason) when the daemon lacks the
    //    test-utils feature, and None on success. We must NOT use the `?` operator
    //    here: `?` on Option<T> short-circuits on None (empty/missing), not on
    //    Some(x). `Some(skip_reason)?` would silently unwrap and discard the skip
    //    signal, letting execution fall through to the scoring assertions.
    if let Some(skip) = inject_recall_hits(socket_path, &memories.fresh_recalled_correlated, 30) {
        return Some(skip);
    }
    if let Some(skip) = inject_recall_hits(socket_path, &memories.midrange_some_recalls, 5) {
        return Some(skip);
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn update_memory_properties(
    database_path: &Path,
    memory_id: &str,
    observed_at_expr: &str,
    original_confidence: f64,
    confidence: f64,
    sensitivity: &str,
    source_harness: &str,
) {
    let conn = Connection::open(database_path).unwrap_or_else(|err| panic!("open {}: {err}", database_path.display()));
    let sql = format!(
        "UPDATE memories \
         SET observed_at = strftime('%Y-%m-%dT%H:%M:%SZ', '{observed_at_expr}'), \
             original_confidence = {original_confidence}, \
             confidence = {confidence}, \
             sensitivity = '{sensitivity}', \
             source_harness = '{source_harness}', \
             status = 'active', \
             passive_recall = 1, \
             index_body = 1 \
         WHERE id = ?1"
    );
    conn.execute(&sql, rusqlite::params![memory_id])
        .unwrap_or_else(|err| panic!("update memory properties for {memory_id}: {err}"));
}

fn insert_corroborating_source(database_path: &Path, memory_a_id: &str, corroborating_id: &str) {
    let conn = Connection::open(database_path).unwrap_or_else(|err| panic!("open {}: {err}", database_path.display()));
    conn.execute_batch(&format!(
        "INSERT OR REPLACE INTO memories(\
            id, path, schema_version, type, scope, namespace, canonical_namespace_id, \
            summary, confidence, original_confidence, trust_level, sensitivity, status, review_state, \
            requires_user_confirmation, created_at, updated_at, observed_at, valid_from, valid_until, ttl, \
            author, source_kind, source_harness, source_device, \
            body_hash, frontmatter_json, file_hash, file_mtime_ns, indexed_at, metadata_only, \
            passive_recall, index_body, human_review_required, max_scope \
        ) VALUES (\
            '{corroborating_id}', 'projects/default/decisions/t16-corroborating.md', 1, 'claim', 'project', \
            'default', 'default', 'T16 Memory A corroborating source', 0.95, 0.95, 'trusted', \
            'public', 'superseded', NULL, 0, \
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), \
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), NULL, NULL, NULL, \
            'agent', 'agent-primary', 'codex', NULL, \
            't16-corroborating-hash', '{{}}', 't16-corroborating-file-hash', 0, \
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 0, 0, 0, 0, 'project'\
        );
        INSERT OR REPLACE INTO memory_supersession(memory_id, supersedes_id) \
        VALUES ('{memory_a_id}', '{corroborating_id}');"
    ))
    .unwrap_or_else(|err| panic!("insert corroborating source {corroborating_id}: {err}"));
}

/// Inject synthetic RecallHit events via the daemon's test-utils protocol surface.
///
/// Each call sends a `TestInjectEvent` request with a timestamp spread over the
/// last `count` days. The daemon appends the event to its events log and SQLite
/// mirror, honoring all deduplication and seq-number invariants. This replaces
/// the previous raw `events_log` INSERT via the external sqlite3 CLI. (H-R1)
///
/// Returns `None` on success or `Some(MEMORYD_TEST_UTILS_NOT_ENABLED)` when the
/// daemon was built without `--features test-utils`.
fn inject_recall_hits(socket_path: &Path, memory_id: &str, count: usize) -> Option<&'static str> {
    let now = chrono::Utc::now();
    for seq in 0..count {
        let ts = now - chrono::Duration::days(seq as i64);
        let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let response = daemon_request(
            socket_path,
            json!({
                "test_inject_event": {
                    "kind": "recall_hit",
                    "memory_id": memory_id,
                    "ts": ts_str,
                    "harness": null,
                    "session_id": null
                }
            }),
        );
        if let Some(code) = response.pointer("/result/error/code").and_then(Value::as_str) {
            if matches!(code, "invalid_request" | "method_not_allowed") {
                // Daemon was compiled without test-utils. Signal a clean skip rather
                // than panicking so the orchestrator records a proper skipped result.
                return Some(MEMORYD_TEST_UTILS_NOT_ENABLED);
            }
        }
        eval_assert!(
            response.pointer("/result/success").is_some(),
            "TestInjectEvent for {memory_id} seq {seq} should succeed: {response:#?}. \
             Build memoryd with --features test-utils to enable this protocol surface."
        );
    }
    None
}

fn stream_g_reality_check_handler_not_shipped(response: &Value) -> bool {
    let code = protocol_error_code(response).unwrap_or_default();
    let message = response.pointer("/result/error/message").and_then(Value::as_str).unwrap_or_default();
    matches!(code, "not_implemented" | "method_not_allowed" | "method_not_allowed_on_mcp" | "unknown_variant")
        || message.contains("reality check handler lands in Stream G")
        || message.contains("unknown variant")
}

fn protocol_error_code(response: &Value) -> Option<&str> {
    response.pointer("/result/error/code").and_then(Value::as_str)
}

fn assert_success(response: &Value, context: &str) {
    eval_assert!(response.pointer("/result/success").is_some(), "{context}: {response:#?}");
}

fn pending_items(response: &Value) -> &[Value] {
    response
        .pointer("/result/success/reality_check/pending/items")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("RealityCheck(List) should return pending items: {response:#?}"))
}

fn find_scored_item<'a>(items: &'a [Value], memory_id: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item.pointer("/memory_id").and_then(Value::as_str) == Some(memory_id))
        .unwrap_or_else(|| panic!("RealityCheck(List) missing scored item for {memory_id}: {items:#?}"))
}

fn score(item: &Value) -> f64 {
    item.pointer("/score").and_then(Value::as_f64).unwrap_or_else(|| panic!("item missing score: {item:#?}"))
}

fn assert_component_shape_and_values(item: &Value, expected: ExpectedComponents) {
    let component_scores =
        item.pointer("/component_scores").unwrap_or_else(|| panic!("missing component_scores: {item:#?}"));
    eval_assert!(component_scores.is_object(), "component_scores must be a named object, got {component_scores:#?}");
    assert_component(component_scores, "days_since_observed_norm", expected.days_since_observed_norm);
    assert_component(component_scores, "recall_frequency_norm", expected.recall_frequency_norm);
    assert_component(component_scores, "cross_source_corroboration", expected.cross_source_corroboration);
    assert_component(component_scores, "confidence_decay", expected.confidence_decay);
    assert_component(component_scores, "sensitivity_weight", expected.sensitivity_weight);
}

fn assert_component(component_scores: &Value, field: &str, expected: Option<f64>) {
    let actual = component_scores
        .get(field)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("component_scores missing numeric field `{field}`: {component_scores:#?}"));
    if let Some(expected) = expected {
        eval_assert!(
            (actual - expected).abs() <= COMPONENT_TOLERANCE,
            "component `{field}` expected approximately {expected}, got {actual}"
        );
    }
}

fn component(item: &Value, field: &str) -> f64 {
    item.pointer(&format!("/component_scores/{field}"))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("item missing numeric component `{field}`: {item:#?}"))
}

fn assert_weighted_sum(item: &Value) {
    let reconstructed = 0.35 * component(item, "days_since_observed_norm")
        + 0.20 * (1.0 - component(item, "recall_frequency_norm"))
        + 0.20 * (1.0 - component(item, "cross_source_corroboration"))
        + 0.15 * component(item, "confidence_decay")
        + 0.10 * component(item, "sensitivity_weight");
    let reported = score(item);
    eval_assert!(
        (reconstructed - reported).abs() <= SCORE_TOLERANCE,
        "reported score should equal weighted component sum: reported={reported}, reconstructed={reconstructed}, item={item:#?}"
    );
}
