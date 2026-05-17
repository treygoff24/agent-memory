//! Wright item `export-since-filter-02`: integration test for
//! `memoryd export --since <ISO>` semantics.
//!
//! Verifies the §5 / §8.2 closure: the filter is `updated_at >= since`
//! (inclusive at the boundary) and the parser strictly rejects bare
//! dates with exit code 2 plus a stderr hint at the canonical RFC3339
//! form.

use std::process::Command;

use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};

const DEVICE_ID: &str = "dev_exportsince02";

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some(DEVICE_ID.to_string()) })
        .await
        .expect("init substrate")
}

/// Build a plaintext memory at a fixed timestamp. Only `id` and
/// `updated_at` matter for the filter; everything else uses the same
/// shape as the `export_json_shape.rs` fixture so the substrate accepts
/// it cleanly.
fn make_plaintext_memory(id: &str, updated_at_str: &str) -> Memory {
    let ts = chrono::DateTime::parse_from_rfc3339(updated_at_str)
        .expect("fixed ts")
        .with_timezone(&chrono::Utc);
    let id = MemoryId::new(id);
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: id.clone(),
            memory_type: MemoryType::Claim,
            scope: Scope::Agent,
            summary: format!("since-filter fixture {id}"),
            confidence: 0.9,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
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
                component: Some("export-since-test".to_string()),
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
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "trusted-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: format!("body for {id}"),
        path: Some(RepoPath::new(format!("agent/claims/{}.md", id.as_str()))),
    }
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write memory");
}

#[tokio::test]
async fn since_filter_is_inclusive_and_rejects_bare_dates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    // Four memories at exact 1-day intervals. T0 chosen well after any
    // chrono epoch defaults so a missing timestamp would never sort
    // ahead of any of these.
    let t0 = "2026-05-01T00:00:00Z"; // not included
    let t1 = "2026-05-02T00:00:00Z"; // not included
    let boundary = "2026-05-03T00:00:00Z"; // INCLUDED (== --since)
    let t3 = "2026-05-04T00:00:00Z"; // INCLUDED

    let id_t0 = "mem_20260501_aaaa00000000aaaa_000001";
    let id_t1 = "mem_20260502_aaaa00000000aaaa_000002";
    let id_boundary = "mem_20260503_aaaa00000000aaaa_000003";
    let id_t3 = "mem_20260504_aaaa00000000aaaa_000004";

    write_memory(&substrate, make_plaintext_memory(id_t0, t0)).await;
    write_memory(&substrate, make_plaintext_memory(id_t1, t1)).await;
    write_memory(&substrate, make_plaintext_memory(id_boundary, boundary)).await;
    write_memory(&substrate, make_plaintext_memory(id_t3, t3)).await;

    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");

    // ------------------------------------------------------------------
    // Sub-case 1: inclusive boundary
    // ------------------------------------------------------------------
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
            "--since",
            boundary,
        ])
        .output()
        .expect("spawn memoryd export with --since");
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf-8");
    assert!(
        output.status.success(),
        "expected exit 0 with valid --since; got {}\nstderr: {stderr}",
        output.status
    );

    let value: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\nstdout: {stdout}"));

    assert_eq!(value["memory_count"], serde_json::json!(2), "memory_count must be 2 (boundary inclusive)");
    let memories = value["memories"].as_array().expect("memories array");
    assert_eq!(memories.len(), 2, "memories.length must be 2");

    // Sort order is (updated_at, id) ascending — boundary comes first, t3 second.
    let included_ids: Vec<&str> = memories.iter().map(|m| m["id"].as_str().expect("id string")).collect();
    assert_eq!(
        included_ids,
        vec![id_boundary, id_t3],
        "included ids must be exactly the boundary (T+2d) and T+3d memories"
    );

    // filters.since should echo back the verbatim ISO string the operator passed.
    assert_eq!(
        value["filters"]["since"].as_str(),
        Some(boundary),
        "filters.since must be the verbatim --since value"
    );

    // ------------------------------------------------------------------
    // Sub-case 2: bare-date input -> exit 2 with canonical-form hint
    // ------------------------------------------------------------------
    let bare = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
            "--since",
            "2026-05-01",
        ])
        .output()
        .expect("spawn memoryd export with bare-date --since");
    let bare_stderr = String::from_utf8(bare.stderr.clone()).expect("stderr utf-8");
    let bare_exit = bare.status.code().expect("export should exit with a code");
    assert_eq!(bare_exit, 2, "bare-date --since must exit 2 (argparse failure); got {bare_exit}\nstderr: {bare_stderr}");
    // The error message must point at the canonical RFC3339 form. The
    // spec allows wording flexibility — the test asserts the canonical
    // example token appears so an operator pasting a bare date sees the
    // exact form to use next.
    assert!(
        bare_stderr.contains("2026-05-01T00:00:00Z"),
        "bare-date error must mention the canonical RFC3339 form; got:\n{bare_stderr}"
    );
}
