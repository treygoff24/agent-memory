use std::path::Path;
use std::process::Command;

use memory_privacy::FileKeyProvider;
use memory_substrate::{
    config::PromptVersion, events::EventKind, Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter,
    InitOptions, Memory, MemoryContent, MemoryId, MemoryStatus, MemoryType, RetrievalPolicy, Roots, Scope, Sensitivity,
    Source, SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::dream::{
    orchestration::{build_dream_run, DreamRunBuildRequest},
    run::{CandidateEvidenceRef, CandidateWriteRequest, CandidateWriter},
    scope::DreamScope,
};

// Dev-fixtures-only imports: the dreaming protocol tests below are gated behind the
// `dev-fixtures` feature (they exercise the deterministic EchoCli harness), and the
// dependency surface they need is gated to match.
#[cfg(feature = "dev-fixtures")]
use memory_privacy::PrivacyLabel;
#[cfg(feature = "dev-fixtures")]
use memory_substrate::{
    ObserveKind as SubstrateObserveKind, PrivacySpanRecord, SubstrateFragmentAppendRequest, SubstrateFragmentPayload,
};
#[cfg(feature = "dev-fixtures")]
use memoryd::dream::{orchestration::SubstrateCandidateWriter, run::DreamRunner};
use memoryd::handlers::handle_request;
#[cfg(feature = "dev-fixtures")]
use memoryd::protocol::LeaseRecord;
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, ObserveKind, RequestEnvelope, RequestPayload, ResponsePayload,
    ResponseResult,
};
#[cfg(feature = "dev-fixtures")]
use {chrono::Duration, chrono::Utc};

const TEST_PROJECT_CANONICAL_ID: &str = "proj_handler_contract";
const TEST_PROJECT_ALIAS: &str = "handler-contract";

fn project_identity_meta() -> serde_json::Value {
    serde_json::json!({
        "namespace": "project",
        "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
        "namespace_alias": TEST_PROJECT_ALIAS
    })
}

fn with_project_identity(mut meta: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(fields) = &mut meta else {
        return project_identity_meta();
    };
    fields.insert("namespace".to_string(), serde_json::json!("project"));
    fields.insert("canonical_namespace_id".to_string(), serde_json::json!(TEST_PROJECT_CANONICAL_ID));
    fields.insert("namespace_alias".to_string(), serde_json::json!(TEST_PROJECT_ALIAS));
    meta
}

#[tokio::test]
async fn search_with_cwd_scopes_to_visible_namespaces() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let in_scope = sample_memory("mem_20260719_a1b2c3d4e5f60718_310001", "nsscopeneedle in-scope agent fact");
    let mut project = sample_memory("mem_20260719_a1b2c3d4e5f60718_310002", "nsscopeneedle other-project fact");
    project.frontmatter.scope = Scope::Project;
    project.frontmatter.namespace = Some("project".to_string());
    project.frontmatter.canonical_namespace_id = Some("proj_elsewhere".to_string());
    project.frontmatter.retrieval_policy.max_scope = Scope::Project;

    for memory in [in_scope.clone(), project.clone()] {
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
            .expect("write memory through Stream A");
    }

    let search = |cwd: Option<String>| {
        let substrate = &substrate;
        async move {
            let response = handle_request(
                substrate,
                RequestEnvelope::new(
                    "req-scoped-search",
                    RequestPayload::Search {
                        query: "nsscopeneedle".to_string(),
                        limit: Some(10),
                        include_body: false,
                        cwd,
                    },
                ),
            )
            .await;
            let ResponseResult::Success(ResponsePayload::Search(search)) = response.result else {
                panic!("expected search success, got {:?}", response.result);
            };
            let mut ids = search.hits.iter().map(|hit| hit.id.clone()).collect::<Vec<_>>();
            ids.sort();
            ids
        }
    };

    // No cwd keeps the store-wide pre-scoping behavior (frozen MCP surface).
    assert_eq!(
        search(None).await,
        vec![in_scope.frontmatter.id.as_str().to_string(), project.frontmatter.id.as_str().to_string()]
    );

    // A cwd with no git remote resolves to me + agent, so the other project's
    // memory is filtered even though it matches the query.
    let cwd_dir = tempfile::tempdir().expect("cwd tempdir");
    assert_eq!(
        search(Some(cwd_dir.path().to_string_lossy().into_owned())).await,
        vec![in_scope.frontmatter.id.as_str().to_string()]
    );

    // An unresolvable cwd is a caller error, never a silent store-wide search.
    let invalid = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-invalid-cwd-search",
            RequestPayload::Search {
                query: "nsscopeneedle".to_string(),
                limit: Some(10),
                include_body: false,
                cwd: Some("/nonexistent/memorum-search-cwd".to_string()),
            },
        ),
    )
    .await;
    let ResponseResult::Error(error) = invalid.result else {
        panic!("expected invalid-cwd search to fail, got {:?}", invalid.result);
    };
    assert_eq!(error.code, "invalid_request");
}

#[tokio::test]
async fn search_and_get_return_bounded_protocol_responses_from_substrate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let memory = sample_memory(
        "mem_20260428_a1b2c3d4e5f60718_300001",
        "Handler contracts search Stream A chunks and return bounded protocol snippets. \
         This extra sentence should not force unbounded response bodies into search results.",
    );

    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write memory through Stream A");

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-search",
            RequestPayload::Search {
                query: "bounded protocol snippets".to_string(),
                limit: Some(1),
                include_body: false,
                cwd: None,
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search success, got {:?}", search.result);
    };
    assert_eq!(search.hits.len(), 1);
    assert_eq!(search.total, 1);
    assert_eq!(search.hits[0].id, memory.frontmatter.id.as_str());
    assert!(search.hits[0].snippet.len() <= 240, "search snippets stay bounded");
    assert_eq!(search.hits[0].body, None);
    assert!(search.guidance.contains("memory_get"));

    let search_with_body = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-search-body",
            RequestPayload::Search {
                query: "bounded protocol snippets".to_string(),
                limit: Some(1),
                include_body: true,
                cwd: None,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search_with_body)) = search_with_body.result else {
        panic!("expected search success with body");
    };
    assert_eq!(search_with_body.hits[0].body.as_deref(), Some(memory.body.as_str()));

    let get = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-get",
            RequestPayload::Get {
                id: memory.frontmatter.id.as_str().to_string(),
                include_provenance: false,
                full_body: false,
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Get(get)) = get.result else {
        panic!("expected get success, got {:?}", get.result);
    };
    assert_eq!(get.id, memory.frontmatter.id.as_str());
    assert_eq!(get.summary, memory.frontmatter.summary);
    assert!(get.body.len() <= 4_096, "get bodies are bounded protocol previews");
    assert_eq!(get.provenance, None);
    assert!(get.guidance.contains("bounded"));

    let get_with_provenance = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-get-provenance",
            RequestPayload::Get {
                id: memory.frontmatter.id.as_str().to_string(),
                include_provenance: true,
                full_body: false,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Get(get_with_provenance)) = get_with_provenance.result else {
        panic!("expected get provenance success");
    };
    let provenance = get_with_provenance.provenance.expect("provenance included");
    assert_eq!(provenance.path.as_deref(), memory.path.as_ref().map(|path| path.as_str()));
    assert_eq!(provenance.source_kind, SourceKind::Import.as_db_str());
    assert_eq!(provenance.author_kind, AuthorKind::System.as_db_str());
}

#[tokio::test]
async fn get_full_body_returns_unbounded_body_for_large_memory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let large_body = "x".repeat(5_000);
    let memory = sample_memory("mem_20260428_a1b2c3d4e5f60718_300004", &large_body);
    substrate
        .write_memory(memory_substrate::WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: memory_substrate::WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write memory");

    let get_full = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-get-full",
            RequestPayload::Get {
                id: memory.frontmatter.id.as_str().to_string(),
                include_provenance: false,
                full_body: true,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Get(get_full)) = get_full.result else {
        panic!("expected full get success, got {:?}", get_full.result);
    };
    assert_eq!(get_full.body, large_body);
    assert!(!get_full.truncated);
    assert_eq!(get_full.status, Some(MemoryStatus::Active));

    let get_preview = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-get-preview",
            RequestPayload::Get {
                id: memory.frontmatter.id.as_str().to_string(),
                include_provenance: false,
                full_body: false,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Get(get_preview)) = get_preview.result else {
        panic!("expected preview get success");
    };
    assert_eq!(get_preview.body.chars().count(), 4_096);
    assert!(get_preview.truncated);
}

#[tokio::test]
async fn conflicts_list_is_served_from_the_index_with_summary_reason_and_updated_at() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    // A quarantined memory carries the fields the conflicts list surfaces: a
    // distinct summary, a distinct updated_at, and _merge_diagnostics. After the
    // index-first rewrite these must round-trip from the index projection, not a
    // per-row canonical-file read.
    let mut memory = sample_memory("mem_20260428_a1b2c3d4e5f60718_300003", "conflicting fact body");
    memory.frontmatter.summary = "quarantined conflict summary".to_string();
    memory.frontmatter.status = MemoryStatus::Quarantined;
    // Quarantined requires the Quarantined trust level (validate.rs lifecycle pair).
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.updated_at = chrono::DateTime::parse_from_rfc3339("2026-05-02T09:30:00Z")
        .expect("fixed updated_at")
        .with_timezone(&chrono::Utc);
    memory.frontmatter.merge_diagnostics = Some(serde_json::json!({ "conflict": "both-edited", "winner": "remote" }));

    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write quarantined memory through Stream A");

    let conflicts = handle_request(
        &substrate,
        RequestEnvelope::new("req-conflicts", RequestPayload::ConflictsList { limit: None }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::ConflictsList(conflicts)) = conflicts.result else {
        panic!("expected conflicts-list success, got {:?}", conflicts.result);
    };
    assert_eq!(conflicts.conflicts.len(), 1, "the single quarantined memory is listed");
    let conflict = &conflicts.conflicts[0];
    assert_eq!(conflict.id, memory.frontmatter.id);
    assert_eq!(conflict.summary, memory.frontmatter.summary, "summary comes from the index projection");
    assert_eq!(conflict.updated_at, memory.frontmatter.updated_at, "updated_at comes from the index column");
    // reason is the rendered _merge_diagnostics JSON; assert on its content, not
    // exact whitespace, since it is the stored JSON string.
    let reason = conflict.reason.as_deref().expect("merge diagnostics surface as a reason");
    assert!(reason.contains("both-edited"), "reason carries merge_diagnostics: {reason}");
    assert!(reason.contains("remote"), "reason carries merge_diagnostics: {reason}");
}

#[tokio::test]
async fn search_include_body_is_bounded_to_the_get_cap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    // A plaintext body well over the 4 KiB get cap, with a unique term to retrieve it.
    let long_body = format!("oversize disclosure canary {}", "x".repeat(8_192));
    let memory = sample_memory("mem_20260428_a1b2c3d4e5f60718_300002", &long_body);
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write oversize memory through Stream A");

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-search-oversize",
            RequestPayload::Search {
                query: "oversize disclosure canary".to_string(),
                limit: Some(1),
                include_body: true,
                cwd: None,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search success, got {:?}", search.result);
    };
    let body = search.hits[0].body.as_deref().expect("plaintext body included");
    assert!(
        body.chars().count() <= 4_096,
        "search include_body must be bounded to the get cap, got {} chars",
        body.chars().count()
    );
}

#[tokio::test]
async fn write_note_creates_candidate_safe_record_through_substrate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-write",
            RequestPayload::WriteNote {
                text: "Candidate note from handler".to_string(),
                meta: serde_json::Value::Null,
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::WriteNote(write)) = response.result else {
        panic!("expected write-note success, got {:?}", response.result);
    };
    let saved = substrate.read_memory(&MemoryId::new(&write.id)).await.expect("candidate note is readable");

    assert_eq!(saved.frontmatter.status, MemoryStatus::Candidate);
    assert_eq!(saved.frontmatter.sensitivity, Sensitivity::Internal);
    assert!(saved.frontmatter.tags.iter().any(|tag| tag == "candidate"));
    assert!(saved.frontmatter.requires_user_confirmation);
    assert_eq!(saved.body, "Candidate note from handler");
}

#[tokio::test]
async fn mcp_human_write_defaults_to_explicit_context_and_promotes_project_memory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-governance-human-default",
            RequestPayload::WriteMemory {
                body: "Human MCP writes should not fight the operator during dogfood.".to_string(),
                title: Some("dogfood human write".to_string()),
                tags: vec!["dogfood".to_string()],
                meta: project_identity_meta(),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance write success, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Promoted);
    assert_eq!(write.reason, None);
    let id = write.id.expect("promoted id");
    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("read promoted memory");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Active);
    assert_eq!(saved.frontmatter.confidence, 0.9);
    assert_eq!(saved.frontmatter.write_policy.policy_applied, "project-standard@v2");
}

#[tokio::test]
async fn mcp_human_write_with_partial_meta_inherits_human_defaults() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-governance-human-partial-meta",
            RequestPayload::WriteMemory {
                body: "Trey prefers narrow verification after isolated stale assertions.".to_string(),
                title: None,
                tags: Vec::new(),
                meta: with_project_identity(serde_json::json!({ "type": "claim" })),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance write success, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Promoted);
    let id = write.id.expect("promoted id");
    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("read promoted memory");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Active);
    assert_eq!(saved.frontmatter.confidence, 0.9);
    assert_eq!(saved.frontmatter.scope, Scope::Project);
}

#[tokio::test]
async fn supersede_null_meta_keeps_strict_programmatic_defaults() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let old_response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-governance-old",
            RequestPayload::WriteMemory {
                body: "The deployment target is staging.".to_string(),
                title: None,
                tags: Vec::new(),
                meta: project_identity_meta(),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(old_write)) = old_response.result else {
        panic!("expected old write success, got {:?}", old_response.result);
    };
    let old_id = old_write.id.expect("old id");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-supersede-default",
            RequestPayload::Supersede {
                old_id,
                content: "The deployment target is production.".to_string(),
                reason: "target changed".to_string(),
                meta: serde_json::Value::Null,
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede response, got {:?}", response.result);
    };
    assert_eq!(supersede.status, GovernanceStatus::Refused);
    assert_eq!(supersede.reason, Some(GovernanceRefusalReason::Grounding));
}

#[tokio::test]
async fn supersede_succeeds_for_plaintext_and_encrypted_memories_alike() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let old_response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-supersede-old-plain",
            RequestPayload::WriteMemory {
                body: "The deployment target is staging.".to_string(),
                title: Some("deployment target".to_string()),
                tags: Vec::new(),
                meta: with_project_identity(serde_json::json!({
                    "explicit_user_context": true,
                    "confidence": 0.95,
                    "source_kind": "user"
                })),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(old_write)) = old_response.result else {
        panic!("expected old write success, got {:?}", old_response.result);
    };
    let old_id = old_write.id.expect("old id");

    let supersede_response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-supersede-plain",
            RequestPayload::Supersede {
                old_id: old_id.clone(),
                content: "The deployment target is production.".to_string(),
                reason: "operator corrected target".to_string(),
                meta: serde_json::json!({
                    "explicit_user_context": true,
                    "confidence": 0.95,
                    "source_kind": "user"
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = supersede_response.result else {
        panic!("expected supersede success response, got {:?}", supersede_response.result);
    };
    assert_eq!(supersede.status, GovernanceStatus::Promoted);
    assert_eq!(supersede.old_id.as_deref(), Some(old_id.as_str()));
    assert!(supersede.new_id.is_some());

    FileKeyProvider::runtime_default(&substrate.roots().runtime).onboard_local_file().expect("privacy key");
    let encrypted_old = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-supersede-old-encrypted",
            RequestPayload::WriteMemory {
                body: "Rep. Mills Chief of Staff cell is 202-555-0198.".to_string(),
                title: Some("Rep. Mills Chief of Staff cell".to_string()),
                tags: vec!["contact".to_string()],
                meta: with_project_identity(serde_json::json!({
                    "explicit_user_context": true,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "summary": "Rep. Mills Chief of Staff cell phone",
                    "privacy_descriptors": {
                        "subject": "Rep. Mills Chief of Staff",
                        "role": "Chief of Staff",
                        "value_kind": "phone"
                    }
                })),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(encrypted_write)) = encrypted_old.result else {
        panic!("expected encrypted write response, got {:?}", encrypted_old.result);
    };
    let encrypted_id = encrypted_write.id.expect("encrypted id");
    let encrypted_envelope =
        substrate.read_memory_envelope(&MemoryId::new(&encrypted_id)).await.expect("encrypted envelope");
    assert!(matches!(encrypted_envelope.content, MemoryContent::Ciphertext { .. }));

    let encrypted_supersede = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-supersede-encrypted",
            RequestPayload::Supersede {
                old_id: encrypted_id,
                content: "Rep. Mills Chief of Staff cell is 202-555-0100.".to_string(),
                reason: "operator corrected contact".to_string(),
                meta: serde_json::json!({
                    "explicit_user_context": true,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "privacy_descriptors": {
                        "subject": "Rep. Mills Chief of Staff",
                        "role": "Chief of Staff",
                        "value_kind": "phone"
                    }
                }),
            },
        ),
    )
    .await;
    // Regression: encrypted-to-encrypted supersession used to refuse with a runbook
    // pointer. The fix routes the new ciphertext through `write_encrypted` and
    // mutates the old envelope's metadata via `update_encrypted_memory_metadata`,
    // so the user can correct an encrypted memory without losing the chain.
    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(encrypted_supersede_ok)) =
        encrypted_supersede.result
    else {
        panic!("expected encrypted supersede success response, got {:?}", encrypted_supersede.result);
    };
    assert_eq!(encrypted_supersede_ok.status, GovernanceStatus::Promoted);
    let new_encrypted_id = encrypted_supersede_ok.new_id.expect("encrypted replacement id");
    let new_envelope = substrate.read_memory_envelope(&MemoryId::new(&new_encrypted_id)).await.expect("new envelope");
    assert!(matches!(new_envelope.content, MemoryContent::Ciphertext { .. }));
    let old_envelope_after = substrate
        .read_memory_envelope(&MemoryId::new(encrypted_supersede_ok.old_id.as_deref().expect("old id echoed")))
        .await
        .expect("old envelope still readable");
    assert_eq!(old_envelope_after.metadata.frontmatter.status, memory_substrate::MemoryStatus::Superseded);
    assert!(old_envelope_after.metadata.frontmatter.superseded_by.iter().any(|id| id.as_str() == new_encrypted_id));
}

#[tokio::test]
async fn forget_rejects_empty_reason_and_sanitizes_sensitive_reason_before_tombstone_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let write = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-forget-write",
            RequestPayload::WriteMemory {
                body: "The stale onboarding code is alpha-42.".to_string(),
                title: Some("stale onboarding code".to_string()),
                tags: Vec::new(),
                meta: project_identity_meta(),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = write.result else {
        panic!("expected write success, got {:?}", write.result);
    };
    let id = write.id.expect("memory id");

    let empty = handle_request(
        &substrate,
        RequestEnvelope::new("req-forget-empty", RequestPayload::Forget { id: id.clone(), reason: "  ".to_string() }),
    )
    .await;
    let ResponseResult::Error(error) = empty.result else {
        panic!("expected empty forget reason error, got {:?}", empty.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("forget reason must not be empty"));

    let forget = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-forget-sensitive",
            RequestPayload::Forget { id, reason: "remove after email from reviewer@example.com".to_string() },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceForget(forget)) = forget.result else {
        panic!("expected forget success, got {:?}", forget.result);
    };
    assert_eq!(forget.status, GovernanceStatus::Tombstoned);
    let tombstones =
        std::fs::read_to_string(substrate.roots().repo.join("tombstones/memoryd-forget.jsonl")).expect("tombstones");
    assert!(tombstones.contains(r#""reason_text":"[redacted]""#), "{tombstones}");
    assert!(!tombstones.contains("reviewer@example.com"), "{tombstones}");
}

#[tokio::test]
async fn reveal_accepts_url_reason_uses_envelope_metadata_and_bounds_reason_length() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    FileKeyProvider::runtime_default(&substrate.roots().runtime).onboard_local_file().expect("privacy key");

    let write = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-reveal-write",
            RequestPayload::WriteMemory {
                body: "Rep. Mills Chief of Staff cell is 202-555-0198.".to_string(),
                title: Some("Rep. Mills Chief of Staff cell".to_string()),
                tags: vec!["contact".to_string()],
                meta: with_project_identity(serde_json::json!({
                    "explicit_user_context": true,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "summary": "Rep. Mills Chief of Staff cell phone",
                    "privacy_descriptors": {
                        "subject": "Rep. Mills Chief of Staff",
                        "role": "Chief of Staff",
                        "value_kind": "phone"
                    }
                })),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = write.result else {
        panic!("expected encrypted write success, got {:?}", write.result);
    };
    let id = write.id.expect("encrypted id");

    let reveal = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-reveal-url-reason",
            RequestPayload::Reveal {
                id: id.clone(),
                reason: "operator opened https://example.com/tickets/123?email=reviewer@example.com".to_string(),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Reveal(reveal)) = reveal.result else {
        panic!("expected reveal success for URL reason, got {:?}", reveal.result);
    };
    assert!(reveal.body.contains("202-555-0198"));

    let empty = handle_request(
        &substrate,
        RequestEnvelope::new("req-reveal-empty", RequestPayload::Reveal { id: id.clone(), reason: " ".to_string() }),
    )
    .await;
    let ResponseResult::Error(error) = empty.result else {
        panic!("expected empty reveal reason error, got {:?}", empty.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("reveal reason must not be empty"));

    let long = handle_request(
        &substrate,
        RequestEnvelope::new("req-reveal-long", RequestPayload::Reveal { id, reason: "a".repeat(513) }),
    )
    .await;
    let ResponseResult::Error(error) = long.result else {
        panic!("expected long reveal reason error, got {:?}", long.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("at most 512"));
}

#[tokio::test]
async fn mcp_human_write_still_refuses_secret_content_before_governance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-governance-secret",
            RequestPayload::WriteMemory {
                body: "SSN 123-45-6789 must not persist.".to_string(),
                title: None,
                tags: Vec::new(),
                meta: project_identity_meta(),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance write response, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Refused);
    assert_eq!(write.reason, Some(GovernanceRefusalReason::Privacy));
    assert!(write.id.is_none());
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_echo_request_returns_dream_now_report() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-now",
            RequestPayload::DreamNow { scope: "me".to_string(), force: false, cli_override: Some("echo".to_string()) },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DreamNow(report)) = response.result else {
        panic!("expected dream-now success, got {:?}", response.result);
    };
    assert_eq!(report.scope, "me");
    assert_eq!(report.cli_used.as_deref(), Some("echo"));
    assert_eq!(report.pass_1.status, memoryd::protocol::PassStatus::Success);
    assert_eq!(report.pass_3.status, memoryd::protocol::PassStatus::Success);
    let journal = report.pass_1.output_path.as_deref().expect("journal output path");
    assert!(substrate.roots().repo.join(journal).is_file(), "journal should be written at {journal}");
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_echo_writes_pass_2_candidate_to_canonical_queue() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());
    substrate
        .append_substrate_fragment(SubstrateFragmentAppendRequest {
            id: Some("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string()),
            at: chrono::Utc::now(),
            session: Some("sess_dream".to_string()),
            harness: Some("codex".to_string()),
            scope: "agent".to_string(),
            entities: vec!["ent_dream_contract".to_string()],
            kind: SubstrateObserveKind::Observation,
            source_ref: Some("session:sess_dream:memory_observe".to_string()),
            privacy_spans: Vec::new(),
            payload: SubstrateFragmentPayload::Plaintext { text: "Keep auth retry state behind one seam".to_string() },
            classification: ClassificationOutcome::Trusted,
            operation_id: None,
        })
        .await
        .expect("seed substrate fragment");
    commit_dirty_fixture_files(substrate.roots().repo.as_path());

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-candidate",
            RequestPayload::DreamNow {
                scope: "agent".to_string(),
                force: false,
                cli_override: Some("echo".to_string()),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DreamNow(report)) = response.result else {
        panic!("expected dream-now success, got {:?}", response.result);
    };
    assert_eq!(report.pass_2.status, memoryd::protocol::PassStatus::Success);
    let candidate_id = report.pass_2.candidate_results[0].id.as_deref().expect("candidate id");
    let candidate = substrate.read_memory(&MemoryId::new(candidate_id)).await.expect("read dream candidate");

    assert_eq!(candidate.frontmatter.status, MemoryStatus::Candidate);
    assert_eq!(candidate.frontmatter.author.kind, AuthorKind::Dreaming);
    assert_eq!(candidate.frontmatter.write_policy.policy_applied, "dreaming-strict");
    assert!(candidate.frontmatter.grounding_rehydration_required());
    assert_eq!(candidate.frontmatter.evidence[0].reference, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A");
}

#[tokio::test]
async fn dream_candidate_writer_refuses_encrypt_at_rest_candidates_without_plaintext_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let build = build_dream_run(
        &substrate,
        DreamRunBuildRequest {
            scope: DreamScope::Agent,
            run_id: "run_private_candidate".to_string(),
            run_date: chrono::Utc::now().date_naive(),
            prompt_version: PromptVersion::V2,
            notifications: None,
            pass_timeout: std::time::Duration::from_secs(1),
            pass_2_max_candidates: 8,
            pass_1_window_days: 7,
        },
    )
    .await
    .expect("dream run builds");

    let result = build
        .writer
        .write_candidate(CandidateWriteRequest {
            claim: "Call 202-555-0198 before publishing the launch runbook.".to_string(),
            namespace: "agent".to_string(),
            kind: "claim".to_string(),
            evidence: vec![CandidateEvidenceRef {
                kind: "substrate".to_string(),
                reference: "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string(),
                excerpt: Some("launch runbook needs one owner".to_string()),
            }],
            confidence: 0.82,
            rationale: "Contains useful operational signal but requires encrypt-at-rest privacy handling.".to_string(),
            policy: "dreaming-strict".to_string(),
            grounding_rehydration_required: true,
        })
        .await;

    assert!(!result.accepted, "{result:?}");
    assert_eq!(result.reason.as_deref(), Some("privacy_required_encryption"));
    assert!(
        !substrate.roots().repo.join("agent/claims").exists(),
        "dreaming-strict refuses encrypt-at-rest candidates instead of writing plaintext"
    );
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_masks_disk_loaded_substrate_privacy_spans_before_prompting() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());
    let fragment = substrate
        .append_substrate_fragment(SubstrateFragmentAppendRequest {
            id: Some("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1B".to_string()),
            at: chrono::Utc::now(),
            session: Some("sess_dream_mask".to_string()),
            harness: Some("codex".to_string()),
            scope: "agent".to_string(),
            entities: vec!["ent_dream_contract".to_string()],
            kind: SubstrateObserveKind::Observation,
            source_ref: Some("session:sess_dream_mask:memory_observe".to_string()),
            privacy_spans: vec![PrivacySpanRecord {
                label: serde_json::to_value(PrivacyLabel::PrivatePerson)
                    .expect("label serializes")
                    .as_str()
                    .expect("label string")
                    .to_string(),
                start: 0,
                end: 5,
            }],
            payload: SubstrateFragmentPayload::Plaintext {
                text: "Alice keeps auth retry state behind one seam".to_string(),
            },
            classification: ClassificationOutcome::Trusted,
            operation_id: None,
        })
        .await
        .expect("seed substrate fragment");
    let substrate_record = std::fs::read_to_string(substrate.roots().repo.join(fragment.path.as_path()))
        .expect("seeded substrate fragment record is readable");
    assert!(
        substrate_record.contains(r#""privacy_spans":[{"label":"private_person","start":0,"end":5}]"#),
        "seeded substrate fragment should persist privacy spans: {substrate_record}"
    );
    commit_dirty_fixture_files(substrate.roots().repo.as_path());

    let build = build_dream_run(
        &substrate,
        DreamRunBuildRequest {
            scope: DreamScope::parse("agent").expect("scope"),
            run_id: "run_masked_prompt_preview".to_string(),
            run_date: chrono::Utc::now().date_naive(),
            prompt_version: PromptVersion::V2,
            notifications: None,
            pass_timeout: std::time::Duration::from_secs(1),
            pass_2_max_candidates: 8,
            pass_1_window_days: 7,
        },
    )
    .await
    .expect("dream run builds");
    let pass_1_prompt =
        DreamRunner::<SubstrateCandidateWriter>::preview_pass_1_prompt(&build.options).expect("preview pass 1");
    assert!(pass_1_prompt.contains("Person_A"), "prompt should contain masked token: {pass_1_prompt}");
    assert!(!pass_1_prompt.contains("Alice"), "prompt must not contain original private value: {pass_1_prompt}");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-masked-substrate",
            RequestPayload::DreamNow {
                scope: "agent".to_string(),
                force: false,
                cli_override: Some("echo".to_string()),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DreamNow(report)) = response.result else {
        panic!("expected dream-now success, got {:?}", response.result);
    };
    let candidate_id = report.pass_2.candidate_results[0].id.as_deref().expect("candidate id");
    let candidate = substrate.read_memory(&MemoryId::new(candidate_id)).await.expect("read dream candidate");

    assert!(candidate.body.contains("Alice"), "Pass 2 candidate write-back restores masked fields: {}", candidate.body);
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_rejects_active_foreign_lease_without_writing_journal() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());
    seed_foreign_active_lease(&substrate);

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-lease-held",
            RequestPayload::DreamNow { scope: "me".to_string(), force: false, cli_override: Some("echo".to_string()) },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected lease_held error, got {:?}", response.result);
    };
    assert_eq!(error.code, "lease_held");
    assert!(error.retryable);
    assert!(
        !substrate.roots().repo.join("dreams/journal/me").exists(),
        "blocked daemon dreams must not write pass outputs"
    );
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_force_overrides_active_foreign_lease() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());
    seed_foreign_active_lease(&substrate);

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-force",
            RequestPayload::DreamNow { scope: "me".to_string(), force: true, cli_override: Some("echo".to_string()) },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DreamNow(report)) = response.result else {
        panic!("expected forced dream-now success, got {:?}", response.result);
    };
    assert_eq!(report.scope, "me");
    assert_eq!(report.pass_1.status, memoryd::protocol::PassStatus::Success);
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_acquires_lease_before_writing_pipeline_outputs() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-lease-first",
            RequestPayload::DreamNow { scope: "me".to_string(), force: false, cli_override: Some("echo".to_string()) },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DreamNow(report)) = response.result else {
        panic!("expected dream-now success, got {:?}", response.result);
    };
    let lease_text = std::fs::read_to_string(substrate.roots().repo.join("leases/journal.lease"))
        .expect("daemon dream writes lease journal");
    assert!(lease_text.contains("\"device\":\"dev_handlercontract\""));
    assert!(lease_text.contains("\"scope\":\"me\""));
    // F1's post-dream flush commits the dream's pipeline outputs ON TOP of the
    // lease-acquire commit, so HEAD is that output flush and its parent is the lease
    // acquire — which is exactly what proves the lease was acquired *before* the dream
    // wrote (and committed) its pipeline outputs.
    let recent = git(substrate.roots().repo.as_path(), ["log", "--format=%s", "-n", "2"]);
    let mut recent = recent.lines();
    let head = recent.next().unwrap_or_default();
    let parent = recent.next().unwrap_or_default();
    assert!(head.starts_with("substrate: commit"), "HEAD should be the post-dream output flush (F1), got: {head:?}");
    assert_eq!(
        parent, "dream: lease acquire me on dev_handlercontract",
        "the lease must be acquired before the dream's pipeline outputs are committed"
    );
    let journal = report.pass_1.output_path.as_deref().expect("journal output path");
    assert!(substrate.roots().repo.join(journal).is_file(), "journal should be written at {journal}");
}

#[tokio::test]
async fn dreaming_protocol_rejects_invalid_or_unavailable_harness_requests_with_typed_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());

    let invalid = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-invalid",
            RequestPayload::DreamNow { scope: "me".to_string(), force: false, cli_override: Some("bogus".to_string()) },
        ),
    )
    .await;
    let ResponseResult::Error(error) = invalid.result else {
        panic!("expected invalid_request error, got {:?}", invalid.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(!error.retryable);

    let unavailable = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-unavailable",
            RequestPayload::DreamNow {
                scope: "me".to_string(),
                force: false,
                cli_override: Some("gemini".to_string()),
            },
        ),
    )
    .await;
    let ResponseResult::Error(error) = unavailable.result else {
        panic!("expected dream_unavailable error, got {:?}", unavailable.result);
    };
    assert_eq!(error.code, "dream_unavailable");
    assert!(error.retryable);
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn dreaming_protocol_respects_device_disabled_sentinel_before_lease_or_outputs() {
    enable_echo_harness_for_test();
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    install_origin(substrate.roots().repo.as_path(), temp.path());
    std::fs::write(substrate.roots().runtime.join("dream-disabled"), "disabled\n").expect("disabled sentinel");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-dream-disabled",
            RequestPayload::DreamNow { scope: "me".to_string(), force: true, cli_override: Some("echo".to_string()) },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected dream_disabled error, got {:?}", response.result);
    };
    assert_eq!(error.code, "dream_disabled");
    assert!(!error.retryable);
    assert!(
        !substrate.roots().repo.join("leases/journal.lease").exists(),
        "disabled daemon dreams must not acquire a lease"
    );
    assert!(
        !substrate.roots().repo.join("dreams/journal/me").exists(),
        "disabled daemon dreams must not write pass outputs"
    );
}

#[cfg(feature = "dev-fixtures")]
fn enable_echo_harness_for_test() {
    // SAFETY: `std::env::set_var` is `unsafe` since Rust 1.85 because libc setenv can
    // race with concurrent getenv from other threads (logging, tokio init, etc.).
    // This helper is a setup-only call from `dev-fixtures`-gated integration tests; the
    // value never changes once written and is read via `var_os` from a single
    // orchestration path. Threading the value through `DreamRunOptions` would remove
    // the hazard entirely and is tracked as release-only follow-up.
    unsafe {
        std::env::set_var("MEMORYD_ENABLE_ECHO_DREAM_HARNESS", "1");
    }
}

#[tokio::test]
async fn observe_handler_appends_substrate_fragment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-observe",
            RequestPayload::Observe {
                text: "handler observe writes substrate".to_string(),
                kind: ObserveKind::Observation,
                entities: vec!["ent_handler".to_string()],
                cwd: temp.path().join("repo").to_string_lossy().into_owned(),
                session_id: "sess_handler".to_string(),
                harness: "codex".to_string(),
                harness_version: None,
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Observe(observe)) = response.result else {
        panic!("expected observe success, got {:?}", response.result);
    };
    assert!(observe.fragment_id.starts_with("sub_"));
}

#[tokio::test]
async fn status_response_includes_default_dream_counters() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(&substrate, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    let ResponseResult::Success(ResponsePayload::Status(status)) = response.result else {
        panic!("expected status success, got {:?}", response.result);
    };

    assert_eq!(status.dreams.dream_runs_invoked_total, 0);
    assert!(status.dreams.pass_failed_total.is_empty());
}

#[tokio::test]
async fn status_response_includes_live_dashboard_counts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let active = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-status-active",
            RequestPayload::WriteMemory {
                body: "Active memory for status dashboard".to_owned(),
                title: Some("Status active".to_owned()),
                tags: vec!["status-dashboard".to_owned()],
                meta: project_identity_meta(),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(active_write)) = active.result else {
        panic!("expected active memory write, got {:?}", active.result);
    };
    assert_eq!(active_write.status, GovernanceStatus::Promoted);
    let candidate = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-status-candidate",
            RequestPayload::WriteNote {
                text: "Candidate note for status dashboard".to_owned(),
                meta: serde_json::Value::Null,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::WriteNote(_)) = candidate.result else {
        panic!("expected candidate note write, got {:?}", candidate.result);
    };
    substrate
        .record_event_best_effort(EventKind::StartupReconciliationCompleted { reindexed: 3, repaired_events: 0 })
        .expect("startup reconciliation event writes");

    let response = handle_request(&substrate, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    let ResponseResult::Success(ResponsePayload::Status(status)) = response.result else {
        panic!("expected status success, got {:?}", response.result);
    };

    let index = status.index_stats.expect("status includes live index stats");
    assert_eq!(index.active_memories, 1);
    assert!(index.last_reindex.is_some());

    let review = status.review_queue_counts.expect("status includes review queue counts");
    assert_eq!(review.candidate, 1);
    assert_eq!(review.quarantined, 0);
    assert_eq!(review.dream_low_confidence, 0);

    assert_eq!(status.conflicts_count, Some(0));
    assert_eq!(status.peer_update_count, Some(0));
    assert!(status.peer_sessions.is_empty());
}

#[tokio::test]
async fn status_response_surfaces_shared_passive_notifications() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let state = memoryd::handlers::HandlerState::new();
    state.passive_notifications().append("Blocked secret write attempt detected.");

    let response = memoryd::handlers::handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-status", RequestPayload::Status),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Status(status)) = response.result else {
        panic!("expected status success, got {:?}", response.result);
    };

    assert_eq!(status.passive_notifications.len(), 1);
    assert_eq!(status.passive_notifications[0].message, "Blocked secret write attempt detected.");
}

#[tokio::test]
async fn trust_artifact_handler_returns_daemon_assembled_artifact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let memory = sample_memory("mem_20260501_0123456789abcdef_000009", "Trust artifact handler memory body");

    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write memory through substrate");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-trust", RequestPayload::TrustArtifact { id: memory.frontmatter.id.to_string() }),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::TrustArtifact(artifact)) = response.result else {
        panic!("expected trust artifact success, got {:?}", response.result);
    };

    assert_eq!(artifact.id, memory.frontmatter.id);
    assert_eq!(artifact.body.display_text(), "Trust artifact handler memory body");
    assert_eq!(artifact.title.display_text(), "handler contract memory");
}

async fn init_substrate(roots: Roots) -> Substrate {
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_handlercontract".to_string()) },
    )
    .await
    .expect("init substrate")
}

fn install_origin(repo: &Path, temp_root: &Path) {
    commit_dirty_fixture_files(repo);
    let origin = temp_root.join("origin.git");
    command_in(temp_root, "git", ["init", "--bare", origin.to_str().expect("origin path")]);
    command_in(repo, "git", ["branch", "-M", "main"]);
    command_in(repo, "git", ["remote", "add", "origin", origin.to_str().expect("origin path")]);
    command_in(repo, "git", ["push", "-u", "origin", "main"]);
}

fn commit_dirty_fixture_files(repo: &Path) {
    if git(repo, ["status", "--porcelain=v1", "--untracked-files=all"]).is_empty() {
        return;
    }
    git(repo, ["add", "."]);
    git(repo, ["commit", "-m", "prepare handler lease fixtures"]);
}

#[cfg(feature = "dev-fixtures")]
fn seed_foreign_active_lease(substrate: &Substrate) {
    let lease = LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: Utc::now() - Duration::minutes(1),
        expires_at: Utc::now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    };
    let lease_path = substrate.roots().repo.join("leases/journal.lease");
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&lease_path).expect("open lease journal");
    use std::io::Write;
    writeln!(file, "{}", serde_json::to_string(&lease).expect("lease serializes")).expect("append lease");
    git(substrate.roots().repo.as_path(), ["add", "leases/journal.lease"]);
    git(substrate.roots().repo.as_path(), ["commit", "-m", "seed foreign lease"]);
    git(substrate.roots().repo.as_path(), ["push"]);
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) -> String {
    command_in(repo, "git", args)
}

fn command_in<const N: usize>(cwd: &Path, program: &str, args: [&str; N]) -> String {
    let output = Command::new(program).args(args).current_dir(cwd).output().expect("command runs");
    if output.status.success() {
        String::from_utf8(output.stdout).expect("stdout utf8").trim().to_string()
    } else {
        panic!(
            "{program} {args:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn sample_memory(id: &str, body: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z")
        .expect("fixed test date")
        .with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "handler contract memory".to_string(),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("memoryd-handler-test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
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
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        body: body.to_string(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
