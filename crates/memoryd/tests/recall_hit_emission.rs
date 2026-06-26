mod common;

use common::trust_for_status;
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use memory_substrate::{
    events::EventKind, Author, AuthorKind, ClassificationOutcome, EncryptedWriteRequest, EventContext, Frontmatter,
    IndexProjection, InitOptions, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope,
    Sensitivity, Source, SourceKind, Substrate, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::recall::{build_delta_response, build_startup_response, DeltaRequest, StartupRequest};

#[tokio::test]
async fn test_startup_recall_emits_recall_hit_per_memory() {
    let fixture = RecallHitFixture::new("dev_recallhitstartup").await;
    let expected_ids = [
        "mem_20260501_1111111111111111_000001",
        "mem_20260501_1111111111111111_000002",
        "mem_20260501_1111111111111111_000003",
    ];
    for id in expected_ids {
        fixture.write_memory(id, &format!("startup recall fact {id}"), MemoryStatus::Active).await;
    }

    let response = fixture.startup().await;

    for id in expected_ids {
        assert!(response.recall_block.contains(id), "startup XML should include {id}");
    }
    assert_eq!(fixture.recall_hit_ids(), sorted_set(expected_ids));
}

#[tokio::test]
async fn test_recall_hit_deduped_within_response() {
    let fixture = RecallHitFixture::new("dev_recallhitdedupe").await;
    let expected_ids = ["mem_20260501_2222222222222222_000001", "mem_20260501_2222222222222222_000002"];
    for id in expected_ids {
        fixture.write_memory(id, &format!("dedupe recall fact {id}"), MemoryStatus::Pinned).await;
    }

    let response = fixture.startup().await;

    let recall_hit_ids = fixture.recall_hit_ids();
    assert_eq!(recall_hit_ids, sorted_set(expected_ids));
    assert_eq!(recall_hit_ids.len(), response.recall_explanation.sections[3].selected_ids.len());
}

#[tokio::test]
async fn test_encrypted_memory_emits_recall_hit() {
    let fixture = RecallHitFixture::new("dev_recallhitencrypted").await;
    let id = "mem_20260501_3333333333333333_000001";
    let mut memory = fixture.memory(id, "metadata-safe encrypted recall summary", MemoryStatus::Pinned);
    memory.frontmatter.sensitivity = Sensitivity::Internal;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;

    fixture.write_encrypted(memory, "metadata-safe encrypted recall summary").await;

    let response = fixture.startup().await;

    assert!(response.recall_block.contains("metadata-safe encrypted recall summary"));
    assert_eq!(fixture.recall_hit_ids(), sorted_set([id]));
}

#[tokio::test]
async fn test_recall_output_xml_unchanged() {
    let fixture = RecallHitFixture::new("dev_recallhitxml").await;
    fixture
        .write_memory("mem_20260501_4444444444444444_000001", "xml stability recall fact", MemoryStatus::Pinned)
        .await;

    let first = fixture.startup().await;
    let second = fixture.startup().await;

    assert_eq!(first.recall_block, second.recall_block);
    assert!(first.recall_block.starts_with("<memory-recall version=\"stream-e-v0.6\""));
    assert!(!first.recall_block.contains("<recall-hit"));
    assert!(!first.recall_block.contains("RecallHit"));
}

#[tokio::test]
async fn test_delta_recall_emits_recall_hit_per_memory() {
    let fixture = RecallHitFixture::new("dev_recallhitdelta").await;
    let expected_ids = ["mem_20260501_5555555555555555_000001", "mem_20260501_5555555555555555_000002"];
    for id in expected_ids {
        fixture.write_memory(id, &format!("deltarecallneedle passive recall fact {id}"), MemoryStatus::Active).await;
    }
    let duplicate_id = "mem_20260501_5555555555555555_000003";
    let mut duplicate = fixture.memory(duplicate_id, "deltarecallneedle duplicate chunk fact", MemoryStatus::Active);
    duplicate.body = duplicate_delta_body();
    fixture.write(duplicate).await;

    let response = fixture.delta("deltarecallneedle").await;

    for id in expected_ids {
        assert!(response.delta_block.contains(id), "delta XML should include {id}");
    }
    assert!(response.delta_block.contains(duplicate_id), "delta XML should include {duplicate_id}");
    assert!(response.delta_block.contains("deltarecallneedle passive recall fact"));

    let recall_hit_ids = fixture.recall_hit_id_list();
    assert_eq!(recall_hit_ids.len(), 3, "one RecallHit should be emitted per included memory");
    assert_eq!(
        recall_hit_ids.into_iter().collect::<BTreeSet<_>>(),
        sorted_set([expected_ids[0], expected_ids[1], duplicate_id])
    );
    assert!(
        response.delta_block.matches(duplicate_id).count() > 1,
        "fixture should cheaply prove duplicate rendered items for one memory"
    );
}

#[tokio::test]
async fn test_concurrent_recall_emission_uses_unique_central_sequences() {
    let fixture = RecallHitFixture::new("dev_recallhitconcurrent").await;
    for index in 1..=3 {
        fixture
            .write_memory(
                &format!("mem_20260501_6666666666666666_{index:06}"),
                &format!("concurrent recall fact {index}"),
                MemoryStatus::Pinned,
            )
            .await;
    }

    let responses = tokio::join!(fixture.startup(), fixture.startup(), fixture.startup(), fixture.startup());
    let responses = vec![responses.0, responses.1, responses.2, responses.3];
    assert!(responses.iter().all(|response| response.recall_block.contains("concurrent recall fact")));

    let events = fixture.substrate.events().expect("read events after concurrent recall");
    let recall_sequences = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::RecallHit { .. }))
        .map(|event| event.seq)
        .collect::<Vec<_>>();
    let unique_sequences = recall_sequences.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique_sequences.len(), recall_sequences.len(), "RecallHit sequences must be unique under concurrency");

    fixture
        .substrate
        .record_encrypted_content_revealed(
            MemoryId::new("mem_20260501_6666666666666666_000001"),
            "post-recall sequence probe".to_string(),
        )
        .expect("record post-recall event through substrate API");
    let all_sequences = fixture.substrate.events().expect("read events after probe").into_iter().map(|event| event.seq);
    assert_unique(all_sequences, "central allocator must not reuse RecallHit sequences");
}

struct RecallHitFixture {
    _temp: tempfile::TempDir,
    repo: std::path::PathBuf,
    substrate: Substrate,
}

impl RecallHitFixture {
    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, repo, substrate }
    }

    async fn startup(&self) -> memoryd::recall::StartupResponse {
        build_startup_response(
            &self.substrate,
            StartupRequest {
                cwd: self.repo.to_string_lossy().into_owned(),
                session_id: "sess_recall_hit".to_owned(),
                harness: "codex".to_owned(),
                harness_version: None,
                include_recent: true,
                since_event_id: None,
                budget_tokens: Some(1024),
                passive: false,
            },
        )
        .await
        .expect("startup recall")
    }

    async fn delta(&self, message: &str) -> memoryd::recall::DeltaResponse {
        build_delta_response(
            &self.substrate,
            DeltaRequest {
                cwd: self.repo.to_string_lossy().into_owned(),
                session_id: "sess_recall_hit".to_owned(),
                harness: "codex".to_owned(),
                message: message.to_owned(),
                budget_tokens: Some(8_000),
                passive: false,
            },
        )
        .await
        .expect("delta recall")
    }

    async fn write_memory(&self, id: &str, summary: &str, status: MemoryStatus) {
        self.write(self.memory(id, summary, status)).await;
    }

    async fn write(&self, memory: Memory) {
        self.substrate
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
            .expect("fixture write");
    }

    async fn write_encrypted(&self, memory: Memory, safe_body: &str) {
        self.substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: b"age encrypted bytes".to_vec(),
                safe_index_projection: Some(IndexProjection { safe_body: Some(safe_body.to_owned()) }),
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .expect("fixture encrypted write");
    }

    fn memory(&self, id: &str, summary: &str, status: MemoryStatus) -> Memory {
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(id),
                memory_type: MemoryType::Project,
                scope: Scope::User,
                summary: summary.to_owned(),
                confidence: 0.9,
                original_confidence: None,
                trust_level: trust_for_status(status),
                sensitivity: Sensitivity::Internal,
                status,
                created_at: instant("2026-05-01T12:00:00Z"),
                updated_at: instant("2026-05-01T12:00:00Z"),
                observed_at: None,
                author: Author {
                    kind: AuthorKind::Agent,
                    user_handle: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_recall_hit".to_owned()),
                    subagent_id: None,
                    phase: None,
                    component: None,
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::AgentPrimary,
                    reference: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_recall_hit".to_owned()),
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
                    max_scope: Scope::User,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: true,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "stream-g-recall-hit-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: BTreeMap::new(),
            },
            body: summary.to_owned(),
            path: Some(RepoPath::new(format!("me/{id}.md"))),
        }
    }

    fn recall_hit_ids(&self) -> BTreeSet<String> {
        self.recall_hit_id_list().into_iter().collect()
    }

    fn recall_hit_id_list(&self) -> Vec<String> {
        self.substrate
            .events()
            .expect("read events")
            .into_iter()
            .filter_map(|event| match event.kind {
                EventKind::RecallHit { id, .. } => Some(id.to_string()),
                _ => None,
            })
            .collect()
    }
}

fn sorted_set<const N: usize>(ids: [&str; N]) -> BTreeSet<String> {
    ids.into_iter().map(str::to_owned).collect()
}

fn assert_unique(values: impl IntoIterator<Item = u64>, label: &str) {
    let values = values.into_iter().collect::<Vec<_>>();
    let unique = values.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), values.len(), "{label}; values were {values:?}");
}

fn duplicate_delta_body() -> String {
    (0..760).map(|index| format!("deltarecallneedle marker{index}")).collect::<Vec<_>>().join(" ")
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
