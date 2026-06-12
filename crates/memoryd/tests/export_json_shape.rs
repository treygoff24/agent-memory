//! Wright item `export-json-shape-01`: integration test for the default JSON
//! output shape of `memoryd export`.
//!
//! Builds a 3-memory fixture (plaintext, ciphertext, metadata-only), spawns the
//! binary via `std::process::Command`, and asserts the v0.1 schema per
//! `docs/specs/feature-memoryd-export-v0.1.md §4 / §8.1`.

use std::process::Command;

use memory_privacy::FileKeyProvider;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EncryptedWriteRequest, EventContext, Frontmatter, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, TrustLevel,
    WritePolicy,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exportshape01";
const TEST_PROJECT_CANONICAL_ID: &str = "proj_export_json_shape";
const TEST_PROJECT_ALIAS: &str = "export-json-shape";

fn make_metadata_only_memory(id: &str, updated_at_str: &str) -> Memory {
    let ts = chrono::DateTime::parse_from_rfc3339(updated_at_str).expect("fixed ts").with_timezone(&chrono::Utc);
    let id = MemoryId::new(id);
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: id.clone(),
            memory_type: MemoryType::Claim,
            scope: Scope::Agent,
            summary: "metadata-only export fixture".to_string(),
            confidence: 0.7,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Confidential,
            status: MemoryStatus::Active,
            created_at: ts,
            updated_at: ts,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("export-shape-test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: vec!["export-test".to_string()],
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::System,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: false,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: true,
                index_body: false,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "encrypted-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: String::new(),
        // write_encrypted derives the encrypted/ path from this original path.
        // The metadata_memory must NOT use encrypted/ prefix — the substrate maps it.
        path: Some(RepoPath::new(format!("agent/confidential/{}.md", id.as_str()))),
    }
}

// ---- main test -----------------------------------------------------------

#[tokio::test]
async fn export_json_shape_validates_v0_1_schema() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    // Memory 1: Plaintext
    let plain_id = "mem_20260501_aabbccdd00112233_000001";
    // Explicit sub-second component pins millisecond-precision round-trip (I1).
    let plain_ts = "2026-05-01T10:00:00.456Z";
    write_plaintext(
        &substrate,
        make_plaintext_memory(plain_id, "This is the plaintext body for the export fixture.", plain_ts),
    )
    .await;

    // Memory 2: Encrypted (Ciphertext) — write a personal body via
    // governance handler after onboarding a local key.
    FileKeyProvider::runtime_default(&temp.path().join("runtime"))
        .onboard_local_file()
        .expect("onboard local key for encrypted fixture");

    let enc_response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "export-enc-fixture",
            RequestPayload::WriteMemory {
                body: "Personal info: phone 202-555-0199".to_string(),
                title: Some("encrypted export fixture".to_string()),
                tags: vec!["export-test".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "encrypted export fixture",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.85,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(enc_write)) = enc_response.result else {
        panic!("expected encrypted governance write success, got {:?}", enc_response.result);
    };
    let enc_id = enc_write.id.expect("encrypted write id");

    // Memory 3: MetadataOnly — write an encrypted record with empty
    // ciphertext bytes.  When read back, the substrate returns MetadataOnly.
    let meta_id = "mem_20260503_aabbccdd00112233_000003";
    let meta_ts = "2026-05-03T12:00:00.789Z";
    let metadata_only_memory = make_metadata_only_memory(meta_id, meta_ts);
    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: metadata_only_memory,
            ciphertext: Vec::new(), // empty → MetadataOnly on read
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("write metadata-only memory");

    // Spawn memoryd export as a subprocess
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo path utf8"),
            "--runtime",
            runtime.to_str().expect("runtime path utf8"),
        ])
        .output()
        .expect("spawn memoryd export");

    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout is utf-8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr is utf-8");

    assert!(
        output.status.success(),
        "memoryd export exited with non-zero status {}\nstdout: {stdout}\nstderr: {stderr}",
        output.status,
    );

    // Parse JSON
    let value: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\nstdout: {stdout}"));

    // schema_version == 1
    assert_eq!(value["schema_version"], serde_json::json!(1), "schema_version must be 1");

    // exported_at: RFC3339 UTC millisecond-precision
    let exported_at = value["exported_at"].as_str().expect("exported_at is string");
    {
        // Must parse as RFC3339 and must end with 'Z' (UTC).
        let parsed = exported_at
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap_or_else(|e| panic!("exported_at '{exported_at}' is not valid RFC3339: {e}"));
        assert!(exported_at.ends_with('Z'), "exported_at must have UTC 'Z' suffix: {exported_at}");
        // Millisecond precision: must contain a '.' with exactly 3 digits before 'Z'.
        let dot_pos = exported_at.rfind('.').expect("exported_at missing milliseconds dot");
        let ms_part = &exported_at[dot_pos + 1..exported_at.len() - 1]; // strip 'Z'
        assert_eq!(ms_part.len(), 3, "exported_at must have 3ms digits, got '{ms_part}' in '{exported_at}'");
        // Also must be a recent timestamp (not epoch).
        let epoch = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("epoch reference")
            .with_timezone(&chrono::Utc);
        assert!(parsed > epoch, "exported_at must be a real recent timestamp");
    }

    // source_device_id matches fixture's device id and is non-empty
    let source_device_id = value["source_device_id"].as_str().expect("source_device_id is string");
    assert!(!source_device_id.is_empty(), "source_device_id must be non-empty");
    assert_eq!(source_device_id, DEVICE_ID, "source_device_id must match fixture's device id");

    // filters.since is JSON null (no --since was passed)
    assert_eq!(
        value["filters"]["since"],
        serde_json::Value::Null,
        "filters.since must be null when no --since is passed"
    );

    // memory_count == 3 and memories.length == 3
    let memory_count = value["memory_count"].as_u64().expect("memory_count is integer");
    assert_eq!(memory_count, 3, "memory_count must be 3");

    let memories = value["memories"].as_array().expect("memories is array");
    assert_eq!(memories.len(), 3, "memories.length must be 3");

    // Per-memory field assertions
    let permitted_scopes = ["user", "project", "org", "agent", "subagent"];
    let permitted_statuses = ["candidate", "active", "pinned", "superseded", "archived", "tombstoned"];

    for (i, mem) in memories.iter().enumerate() {
        // Required fields present
        for field in &["id", "scope", "status", "frontmatter", "body", "body_marker", "created_at", "updated_at"] {
            assert!(mem.get(field).is_some(), "memory[{i}] is missing field '{field}'");
        }

        // id is non-empty string
        assert!(
            mem["id"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
            "memory[{i}].id must be a non-empty string"
        );

        // scope is a permitted serde-canonical string
        let scope = mem["scope"].as_str().unwrap_or_else(|| panic!("memory[{i}].scope must be a string"));
        assert!(permitted_scopes.contains(&scope), "memory[{i}].scope '{scope}' must be one of {:?}", permitted_scopes);

        // status is a permitted serde-canonical string
        let status = mem["status"].as_str().unwrap_or_else(|| panic!("memory[{i}].status must be a string"));
        assert!(
            permitted_statuses.contains(&status),
            "memory[{i}].status '{status}' must be one of {:?}",
            permitted_statuses
        );

        // frontmatter is an object
        assert!(mem["frontmatter"].is_object(), "memory[{i}].frontmatter must be a JSON object");

        // created_at and updated_at are RFC3339 strings at millisecond precision (I1).
        for ts_field in &["created_at", "updated_at"] {
            let ts = mem[ts_field].as_str().unwrap_or_else(|| panic!("memory[{i}].{ts_field} must be a string"));
            ts.parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap_or_else(|e| panic!("memory[{i}].{ts_field} '{ts}' is not valid RFC3339: {e}"));
            // Pin millisecond precision: must end with `.NNNZ` so round-trips don't lose sub-second order.
            let dot_pos = ts
                .rfind('.')
                .unwrap_or_else(|| panic!("memory[{i}].{ts_field} '{ts}' must include milliseconds (\\.\\d{{3}}Z)"));
            let ms_part = &ts[dot_pos + 1..ts.len() - 1];
            assert_eq!(ms_part.len(), 3, "memory[{i}].{ts_field} '{ts}' must have 3-digit ms component");
            assert!(
                ms_part.chars().all(|c| c.is_ascii_digit()),
                "memory[{i}].{ts_field} ms part must be digits: '{ms_part}'"
            );
        }
    }

    // Sorted by (updated_at, id) ascending
    for window in memories.windows(2) {
        let a_updated = window[0]["updated_at"].as_str().expect("updated_at string");
        let b_updated = window[1]["updated_at"].as_str().expect("updated_at string");
        let a_id = window[0]["id"].as_str().expect("id string");
        let b_id = window[1]["id"].as_str().expect("id string");
        let a_key = (a_updated, a_id);
        let b_key = (b_updated, b_id);
        assert!(a_key <= b_key, "memories must be sorted by (updated_at, id) ascending: {a_key:?} > {b_key:?}");
    }

    // Body/body_marker routing per §6

    // Find each memory by checking body/body_marker
    let plain_mem = memories.iter().find(|m| m["id"].as_str() == Some(plain_id)).unwrap_or_else(|| {
        let ids: Vec<_> = memories.iter().map(|m| m["id"].as_str()).collect();
        panic!("plaintext memory with id {plain_id} not found; got ids: {ids:?}")
    });
    assert!(
        plain_mem["body"].is_string() && !plain_mem["body"].as_str().unwrap_or("").is_empty(),
        "plaintext memory must have non-null body"
    );
    assert_eq!(plain_mem["body_marker"], serde_json::Value::Null, "plaintext memory must have null body_marker");

    let enc_mem = memories.iter().find(|m| m["id"].as_str() == Some(&enc_id)).unwrap_or_else(|| {
        let ids: Vec<_> = memories.iter().map(|m| m["id"].as_str()).collect();
        panic!("encrypted memory with id {enc_id} not found; got ids: {ids:?}")
    });
    assert_eq!(enc_mem["body"], serde_json::Value::Null, "encrypted memory must have null body");
    assert_eq!(
        enc_mem["body_marker"],
        serde_json::json!("encrypted"),
        "encrypted memory must have body_marker 'encrypted'"
    );

    let meta_mem = memories.iter().find(|m| m["id"].as_str() == Some(meta_id)).unwrap_or_else(|| {
        let ids: Vec<_> = memories.iter().map(|m| m["id"].as_str()).collect();
        panic!("metadata-only memory with id {meta_id} not found; got ids: {ids:?}")
    });
    assert_eq!(meta_mem["body"], serde_json::Value::Null, "metadata-only memory must have null body");
    assert_eq!(
        meta_mem["body_marker"],
        serde_json::json!("metadata-only"),
        "metadata-only memory must have body_marker 'metadata-only'"
    );

    // Stderr: exactly one success-summary line matching ^memory_count=\d+ bytes=\d+$
    let stderr_trimmed = stderr.trim_end_matches('\n');
    let stderr_lines: Vec<&str> = stderr_trimmed.lines().collect();
    assert_eq!(
        stderr_lines.len(),
        1,
        "stderr must contain exactly one line (the success summary); got {} lines:\n{stderr}",
        stderr_lines.len()
    );
    let summary_line = stderr_lines[0];
    // Spec §8.1: stderr success-summary matches ^memory_count=\d+ bytes=\d+$
    let summary_re = regex::Regex::new(r"^memory_count=\d+ bytes=\d+$").expect("summary regex");
    assert!(
        summary_re.is_match(summary_line),
        "stderr success-summary must match ^memory_count=\\d+ bytes=\\d+$; got: '{summary_line}'"
    );
}
