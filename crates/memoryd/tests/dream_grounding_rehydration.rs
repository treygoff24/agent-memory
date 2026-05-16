use chrono::{TimeZone, Utc};
use memory_substrate::error::WriteFailureKind;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Evidence, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, ObserveKind, PrivacySpanRecord, RetrievalPolicy, Roots, Scope, Sensitivity,
    Source, SourceKind, Substrate, SubstrateFragmentAppendRequest, SubstrateFragmentPayload, TrustLevel, WriteMode,
    WritePolicy, WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

const GROUNDING_FAILED: &str = "grounding_rehydration_failed";

#[tokio::test]
async fn dream_candidate_with_missing_substrate_ref_quarantines_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110001",
        [evidence_ref("ev_missing", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A", "missing")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_aged_out_substrate_ref_quarantines_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    append_fragment(
        &substrate,
        "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1B",
        "old substrate observation",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
    )
    .await;
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110002",
        [evidence_ref("ev_aged", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1B", "old substrate observation")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_drifted_substrate_ref_quarantines_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    append_fragment(&substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1C", "current text is materially different", Utc::now())
        .await;
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110003",
        [evidence_ref("ev_drift", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1C", "original text")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_missing_repo_relative_file_ref_quarantines_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110020",
        [evidence_ref("ev_missing_file", "docs/missing-grounding.md", "missing file grounding")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_missing_file_scheme_ref_quarantines_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110021",
        [evidence_ref("ev_missing_file", "file:docs/missing-grounding.md", "missing file grounding")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_drifted_file_ref_uses_configured_threshold() {
    let (_temp, substrate) = initialized_substrate().await;
    write_drift_threshold_config(&substrate, 0.05);
    write_repo_file(&substrate, "docs/grounding.md", "currant grounding facts!\n");
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110022",
        [evidence_ref("ev_file_drift", "docs/grounding.md", "current grounding fact")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_valid_file_ref_promotes_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    write_repo_file(&substrate, "docs/grounding.md", "stable file grounding\n");
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110023",
        [evidence_ref("ev_file_valid", "file:docs/grounding.md", "stable file grounding")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_promoted(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_file_refs_reject_absolute_traversal_and_symlink_escape() {
    for (index, reference) in
        ["/etc/passwd".to_string(), "file:/etc/passwd".to_string(), "../outside.md".to_string()].into_iter().enumerate()
    {
        let (_temp, substrate) = initialized_substrate().await;
        let candidate = dream_candidate(
            &format!("mem_20260430_a1b2c3d4e5f60718_{:06}", 110040 + index),
            [evidence_ref("ev_escape", &reference, "root")],
        );
        write_memory(&substrate, candidate.clone()).await;

        approve(&substrate, candidate.frontmatter.id.as_str()).await;

        assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
    }

    let (temp, substrate) = initialized_substrate().await;
    let outside = temp.path().join("outside-grounding.md");
    std::fs::write(&outside, "outside grounding\n").expect("outside file");
    let link = substrate.roots().repo.join("docs/escape-link.md");
    std::fs::create_dir_all(link.parent().expect("link parent")).expect("link parent");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink to outside file");
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110043",
        [evidence_ref("ev_symlink_escape", "docs/escape-link.md", "outside grounding")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn dream_candidate_with_inactive_memory_ref_quarantines_on_approve() {
    for (status, trust, id_suffix) in [
        (MemoryStatus::Candidate, TrustLevel::Candidate, "110011"),
        (MemoryStatus::Quarantined, TrustLevel::Quarantined, "110012"),
        (MemoryStatus::Tombstoned, TrustLevel::Trusted, "110004"),
        (MemoryStatus::Superseded, TrustLevel::Trusted, "110005"),
        (MemoryStatus::Archived, TrustLevel::Trusted, "110006"),
    ] {
        let (_temp, substrate) = initialized_substrate().await;
        let cited_id = format!("mem_20260430_a1b2c3d4e5f60718_{id_suffix}");
        let cited = lifecycle_memory(&cited_id, status, trust);
        write_memory(&substrate, cited).await;
        let candidate_id = format!("mem_20260430_a1b2c3d4e5f60718_{}", id_suffix.parse::<u32>().unwrap() + 100);
        let candidate = dream_candidate(&candidate_id, [evidence_ref("ev_inactive", &cited_id, "cited memory body")]);
        write_memory(&substrate, candidate.clone()).await;

        approve(&substrate, candidate.frontmatter.id.as_str()).await;

        assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
    }
}

#[tokio::test]
async fn dream_candidate_with_acceptable_memory_ref_promotes_on_approve() {
    for (status, trust, cited_suffix, candidate_suffix) in [
        (MemoryStatus::Active, TrustLevel::Trusted, "110009", "110109"),
        (MemoryStatus::Pinned, TrustLevel::Pinned, "110010", "110110"),
    ] {
        let (_temp, substrate) = initialized_substrate().await;
        let cited_id = format!("mem_20260430_a1b2c3d4e5f60718_{cited_suffix}");
        let cited = lifecycle_memory(&cited_id, status, trust);
        write_memory(&substrate, cited).await;
        let candidate_id = format!("mem_20260430_a1b2c3d4e5f60718_{candidate_suffix}");
        let candidate =
            dream_candidate(&candidate_id, [evidence_ref("ev_memory_valid", &cited_id, "cited memory body")]);
        write_memory(&substrate, candidate.clone()).await;

        approve(&substrate, candidate.frontmatter.id.as_str()).await;

        assert_promoted(&substrate, candidate.frontmatter.id.as_str()).await;
    }
}

#[tokio::test]
async fn dream_candidate_with_valid_refs_promotes_on_approve() {
    let (_temp, substrate) = initialized_substrate().await;
    append_fragment(&substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1D", "stable substrate observation", Utc::now()).await;
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110007",
        [evidence_ref("ev_valid", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1D", "stable substrate observation")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_promoted(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn malformed_dream_config_fails_rehydration_closed_without_defaults() {
    let (_temp, substrate) = initialized_substrate().await;
    std::fs::write(
        substrate.roots().repo.join("config.yaml"),
        r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  pass_2_drift_threshold: definitely-not-a-number
"#,
    )
    .expect("write malformed dreams config");
    write_repo_file(&substrate, "docs/grounding.md", "stable file grounding\n");
    let candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110044",
        [evidence_ref("ev_file_valid", "docs/grounding.md", "stable file grounding")],
    );
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    assert_quarantined_for_rehydration(&substrate, candidate.frontmatter.id.as_str()).await;
}

#[tokio::test]
async fn non_dream_candidate_existing_review_behavior_is_unchanged() {
    let (_temp, substrate) = initialized_substrate().await;
    let mut candidate = dream_candidate(
        "mem_20260430_a1b2c3d4e5f60718_110008",
        [evidence_ref("ev_missing", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1E", "missing")],
    );
    candidate.frontmatter.author.kind = AuthorKind::Agent;
    candidate.frontmatter.author.phase = None;
    candidate.frontmatter.author.harness = Some("memoryd".to_string());
    candidate.frontmatter.author.session_id = Some("session".to_string());
    candidate.frontmatter.set_grounding_rehydration_required(false);
    write_memory(&substrate, candidate.clone()).await;

    approve(&substrate, candidate.frontmatter.id.as_str()).await;

    let saved = substrate.read_memory(&candidate.frontmatter.id).await.expect("read approved candidate");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Active);
}

#[tokio::test]
async fn dream_prose_source_refs_are_refused_before_disk_effects() {
    let (_temp, substrate) = initialized_substrate().await;
    for (index, (field, reference)) in [
        ("source", "dreams/journal/me/2026-04-30.md"),
        ("source", "dreams/questions/me/2026-04-30.jsonl"),
        ("source", "file:dreams/journal/me/2026-04-30.md#L12"),
        ("source", "file:dreams/questions/me/2026-04-30.jsonl#q1"),
        ("evidence", "dreams/journal/me/2026-04-30.md"),
        ("evidence", "dreams/questions/me/2026-04-30.jsonl"),
        ("evidence", "file:dreams/journal/me/2026-04-30.md#L12"),
        ("evidence", "file:dreams/questions/me/2026-04-30.jsonl#q1"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("mem_20260430_a1b2c3d4e5f60718_{:06}", 110030 + index);
        let mut candidate =
            dream_candidate(&id, [evidence_ref("ev_valid", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1F", "unused")]);
        if field == "source" {
            candidate.frontmatter.source.reference = Some(reference.to_string());
        } else {
            candidate.frontmatter.evidence = vec![evidence_ref("ev_dream_prose", reference, "dream prose")];
        }
        let path = candidate.path.clone().expect("candidate path");

        let failure = match write_raw(&substrate, candidate).await {
            Ok(_) => panic!("{field} ref {reference} should be refused"),
            Err(failure) => failure,
        };
        assert_eq!(failure.kind, WriteFailureKind::DreamProseAsSource, "{field} ref {reference}");
        assert!(!substrate.roots().repo.join(path.as_path()).exists(), "{field} ref {reference}");
    }
}

async fn initialized_substrate() -> (tempfile::TempDir, Substrate) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_dreamrehydration".to_string()) },
    )
    .await
    .expect("init substrate");
    (temp, substrate)
}

async fn append_fragment(substrate: &Substrate, id: &str, text: &str, at: chrono::DateTime<Utc>) {
    substrate
        .append_substrate_fragment(SubstrateFragmentAppendRequest {
            id: Some(id.to_string()),
            at,
            session: Some("dream-session".to_string()),
            harness: Some("codex".to_string()),
            scope: "agent".to_string(),
            entities: vec!["entity:test".to_string()],
            kind: ObserveKind::Pattern,
            source_ref: Some("file:/tmp/source.md".to_string()),
            privacy_spans: Vec::<PrivacySpanRecord>::new(),
            payload: SubstrateFragmentPayload::Plaintext { text: text.to_string() },
            classification: ClassificationOutcome::Trusted,
            operation_id: None,
        })
        .await
        .expect("append fragment");
}

async fn approve(substrate: &Substrate, id: &str) {
    let response = handle_request(
        substrate,
        RequestEnvelope::new("req-review-approve", RequestPayload::ReviewApprove { id: id.to_string() }),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::ReviewApprove(_)) = response.result else {
        panic!("expected review approval response, got {:?}", response.result);
    };
}

async fn assert_quarantined_for_rehydration(substrate: &Substrate, id: &str) {
    let saved = substrate.read_memory(&MemoryId::new(id)).await.expect("read quarantined candidate");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Quarantined);
    assert_eq!(saved.frontmatter.trust_level, TrustLevel::Quarantined);
    assert_eq!(
        saved.frontmatter.extras.get("governance_reason").and_then(serde_json::Value::as_str),
        Some(GROUNDING_FAILED)
    );
}

async fn assert_promoted(substrate: &Substrate, id: &str) {
    let saved = substrate.read_memory(&MemoryId::new(id)).await.expect("read promoted candidate");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Active);
    assert_eq!(saved.frontmatter.trust_level, TrustLevel::Trusted);
    assert_eq!(saved.frontmatter.review_state, None);
}

fn write_repo_file(substrate: &Substrate, relative_path: &str, contents: &str) {
    let path = substrate.roots().repo.join(relative_path);
    std::fs::create_dir_all(path.parent().expect("repo file parent")).expect("create repo file parent");
    std::fs::write(path, contents).expect("write repo file");
}

fn write_drift_threshold_config(substrate: &Substrate, threshold: f64) {
    std::fs::write(
        substrate.roots().repo.join("config.yaml"),
        format!(
            r#"schema_version: 1
active_embedding:
  provider: synthetic
  model_ref: stream-a-test
  dimension: 32
dreams:
  pass_2_drift_threshold: {threshold}
"#
        ),
    )
    .expect("write dreams config");
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
    write_raw(substrate, memory).await.expect("write memory");
}

async fn write_raw(
    substrate: &Substrate,
    memory: Memory,
) -> Result<memory_substrate::WriteOutcome, memory_substrate::error::WriteFailure> {
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
}

fn dream_candidate<const N: usize>(id: &str, evidence: [Evidence; N]) -> Memory {
    let mut memory = base_memory(id, MemoryStatus::Candidate, TrustLevel::Candidate);
    memory.frontmatter.author = Author {
        kind: AuthorKind::Dreaming,
        user_handle: None,
        harness: Some("codex".to_string()),
        harness_version: Some("test".to_string()),
        session_id: Some("dream-session".to_string()),
        subagent_id: None,
        phase: Some("pass_2".to_string()),
        component: None,
    };
    memory.frontmatter.evidence = evidence.into();
    memory.frontmatter.requires_user_confirmation = true;
    memory.frontmatter.review_state = Some("candidate".to_string());
    memory.frontmatter.write_policy.human_review_required = true;
    memory.frontmatter.write_policy.policy_applied = "dreaming-strict@v1".to_string();
    memory.frontmatter.set_grounding_rehydration_required(true);
    memory
}

fn lifecycle_memory(id: &str, status: MemoryStatus, trust: TrustLevel) -> Memory {
    let mut memory = base_memory(id, status, trust);
    memory.body = "cited memory body".to_string();
    match status {
        MemoryStatus::Candidate => {
            memory.frontmatter.requires_user_confirmation = true;
            memory.frontmatter.review_state = Some("candidate".to_string());
            memory.frontmatter.write_policy.human_review_required = true;
        }
        MemoryStatus::Quarantined => {
            memory.frontmatter.requires_user_confirmation = true;
            memory.frontmatter.review_state = Some("quarantined".to_string());
            memory.frontmatter.write_policy.human_review_required = true;
            memory.frontmatter.merge_diagnostics = Some(serde_json::json!({"reason": "test"}));
        }
        MemoryStatus::Tombstoned => memory.frontmatter.tombstone_events.push(memory_substrate::TombstoneEvent {
            id: "tomb_01HZX0YA0".to_string(),
            applied_at: Utc::now(),
            actor: memory_substrate::TombstoneActor {
                kind: memory_substrate::TombstoneActorKind::System,
                reference: "test".to_string(),
            },
            reason: memory_substrate::TombstoneKind::Stale,
            reason_text: None,
            reason_hash: None,
            prior_status: MemoryStatus::Active,
        }),
        MemoryStatus::Superseded => {
            memory.frontmatter.superseded_by.push(MemoryId::new("mem_20260430_a1b2c3d4e5f60718_999999"));
        }
        _ => {}
    }
    memory
}

fn base_memory(id: &str, status: MemoryStatus, trust_level: TrustLevel) -> Memory {
    let memory_id = MemoryId::try_new(id).unwrap_or_else(|err| panic!("invalid test memory id: {err}"));
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: memory_id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "dream rehydration candidate".to_string(),
            confidence: 0.99,
            original_confidence: None,
            trust_level,
            sensitivity: Sensitivity::Internal,
            status,
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
                component: Some("dream-rehydration-test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: vec![Entity {
                id: "entity:test".to_string(),
                label: "Test Entity".to_string(),
                aliases: Vec::new(),
            }],
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Synthesis,
                reference: None,
                harness: Some("codex".to_string()),
                harness_version: Some("test".to_string()),
                session_id: Some("dream-session".to_string()),
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
                policy_applied: "test-policy@v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: "candidate body".to_string(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn evidence_ref(id: &str, reference: &str, quote: &str) -> Evidence {
    Evidence {
        id: id.to_string(),
        quote: quote.to_string(),
        quote_norm_hash: None,
        reference: reference.to_string(),
        weight: 1.0,
        observed_at: None,
        source: None,
    }
}
