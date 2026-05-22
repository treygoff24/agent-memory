use chrono::{TimeZone, Utc};
use memory_source::hash::sha256_prefixed;
use memory_source::storage::excerpts_jsonl;
use memory_source::{
    ArtifactStore, CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, ExcerptLocator,
    ExcerptMatchKind, ExcerptRecord, ExtractedTextStorage, RawStorage, SourceArtifactId, WebCaptureArtifact,
    WebCaptureManifest,
};
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EncryptedWriteRequest, EventContext, EventId, Frontmatter,
    InitOptions, Memory, MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath, RetrievalPolicy, Roots, Scope,
    Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::trust_artifact::{SafeContent, TrustArtifactBuilder};
use rusqlite::Connection;
use serde_json::json;

const TARGET_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000901";
const OLD_ID: &str = "mem_20260428_a1b2c3d4e5f60718_000004";
const NEWER_ID: &str = "mem_20260502_a1b2c3d4e5f60718_000002";

#[tokio::test]
async fn all_sections_present_for_plaintext_memory() {
    let fixture = Fixture::new().await;
    fixture.write(sample_memory(TARGET_ID, "Deploy target is production ECS", "Plaintext body")).await;
    fixture.append_recent_recall("evt_recall_a", 1, TARGET_ID).await;
    fixture.reindex_events();
    fixture.insert_governance_decision(TARGET_ID);

    let artifact = fixture.artifact(TARGET_ID).await;

    assert_eq!(artifact.id, MemoryId::new(TARGET_ID));
    assert!(matches!(artifact.title, SafeContent::Plaintext(_)));
    assert!(matches!(artifact.body, SafeContent::Plaintext(_)));
    assert_eq!(artifact.current_confidence, "0.95");
    assert_eq!(artifact.original_confidence, "0.90");
    assert_eq!(artifact.recall.total, 1);
    assert!(!artifact.provenance_chain.is_empty());
    assert_eq!(artifact.policy_decisions.len(), 1);
    assert_eq!(artifact.privacy_scan.storage_action, "plaintext");
    assert!(artifact.supersedes.is_empty());
    assert!(artifact.superseded_by.is_empty());
    assert!(!artifact.sync_state.devices.is_empty());
}

#[tokio::test]
async fn web_grounded_memory_shows_bounded_source_evidence_without_raw_body() {
    let fixture = Fixture::new().await;
    let source_ref = write_web_artifact(&fixture);
    let mut memory = sample_memory(TARGET_ID, "Web grounded", "The memory cites only the bounded excerpt.");
    memory.frontmatter.source.kind = SourceKind::Web;
    memory.frontmatter.source.reference = Some(source_ref);
    fixture.write(memory).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;
    let evidence = artifact.source_evidence.as_ref().expect("web evidence projected");

    assert!(evidence.available);
    assert_eq!(evidence.kind, "web");
    assert_eq!(evidence.original_url.as_deref(), Some("https://example.com/report"));
    assert_eq!(evidence.final_url.as_deref(), Some("https://example.com/report"));
    assert_eq!(evidence.quote.as_deref(), Some("bounded exact quote"));
    let json = serde_json::to_string(&artifact).expect("serialize artifact");
    assert!(!json.contains("full raw captured page body"));
}

#[tokio::test]
async fn encrypted_memory_shows_content_redacted_but_keeps_other_sections() {
    let fixture = Fixture::new().await;
    fixture.write_encrypted(sample_memory(TARGET_ID, "Secret deployment note", "")).await;
    fixture.append_recent_recall("evt_recall_encrypted", 1, TARGET_ID).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;

    assert_eq!(artifact.title, SafeContent::Encrypted);
    assert_eq!(artifact.body, SafeContent::Encrypted);
    assert_eq!(artifact.title.display_text(), "[encrypted - use memoryd reveal <id> to decrypt]");
    assert_eq!(artifact.recall.total, 1);
    assert_eq!(artifact.privacy_scan.storage_action, "encrypted");
    assert!(!artifact.provenance_chain.is_empty());
    assert_eq!(artifact.sync_state.claim_lock_status, None);
}

#[tokio::test]
async fn provenance_chain_is_sorted_chronologically() {
    let fixture = Fixture::new().await;
    fixture.write(sample_memory(TARGET_ID, "Sorted provenance", "Body")).await;
    fixture.append_recall(RecallFixture::new("evt_late", 9, TARGET_ID, TimestampParts::new(2026, 5, 1, 50))).await;
    fixture.append_recall(RecallFixture::new("evt_early", 2, TARGET_ID, TimestampParts::new(2026, 5, 1, 5))).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;
    let timestamps: Vec<_> = artifact.provenance_chain.iter().map(|event| event.timestamp).collect();
    let mut sorted = timestamps.clone();
    sorted.sort();
    assert_eq!(timestamps, sorted);
}

#[tokio::test]
async fn policy_decision_expands_all_governance_fields() {
    let fixture = Fixture::new().await;
    fixture.write(sample_memory(TARGET_ID, "Policy decision", "Body")).await;
    fixture.reindex_events();
    fixture.insert_governance_decision(TARGET_ID);

    let artifact = fixture.artifact(TARGET_ID).await;
    let decision = artifact.policy_decisions.first().expect("policy decision");

    assert_eq!(decision.policy_applied, "project-standard@v2");
    assert_eq!(decision.policy_source, "disk");
    assert_eq!(decision.confidence_floor_pass, "pass (0.95 >= 0.80)");
    assert_eq!(decision.grounding_satisfied, "2 source refs resolved");
    assert_eq!(decision.contradiction_result, "none detected");
    assert_eq!(decision.tombstone_enforced, "no matching tombstone");
    assert_eq!(decision.sensitivity_gate_result, "pass (internal)");
}

#[tokio::test]
async fn policy_decisions_are_empty_without_governance_events() {
    let fixture = Fixture::new().await;
    fixture.write(sample_memory(TARGET_ID, "No governance event", "Body")).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;

    assert!(artifact.policy_decisions.is_empty());
}

#[tokio::test]
async fn missing_privacy_scan_runs_deterministic_classifier_for_plaintext() {
    let fixture = Fixture::new().await;
    let mut memory = sample_memory(TARGET_ID, "Classifier fallback", "Email reviewer@example.com before launch.");
    memory.frontmatter.extras.remove("privacy_scan");
    fixture.write(memory).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;

    assert!(artifact.privacy_scan.labels_detected.contains(&"private_email".to_owned()));
    assert_eq!(artifact.privacy_scan.storage_action, "encrypted");
}

#[tokio::test]
async fn recall_count_30d_and_last_recalled_are_derived_from_events_log() {
    let fixture = Fixture::new().await;
    fixture.write(sample_memory(TARGET_ID, "Recall stats", "Body")).await;
    for seq in 1..=5 {
        let event_id = format!("evt_recent_{seq}");
        fixture
            .append_recall(RecallFixture::new(&event_id, seq, TARGET_ID, TimestampParts::new(2026, 5, 1, seq as u32)))
            .await;
    }
    fixture.append_old_recall("evt_old", 99, TARGET_ID).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;

    assert_eq!(artifact.recall.total, 6);
    assert_eq!(artifact.recall.last_30_days, 5);
    assert_eq!(
        artifact.recall.last_recalled_at,
        Some(Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, 5).single().expect("fixture time"))
    );
}

#[tokio::test]
async fn supersession_links_are_resolved_from_projection() {
    let fixture = Fixture::new().await;
    fixture.write(sample_memory(OLD_ID, "Deploy target TBD", "Old body")).await;
    let mut target = sample_memory(TARGET_ID, "Deploy target is ECS", "New body");
    target.frontmatter.supersedes.push(MemoryId::new(OLD_ID));
    fixture.write(target).await;
    let mut newer = sample_memory(NEWER_ID, "Deploy target is ECS in us-east-1", "Newest body");
    newer.frontmatter.supersedes.push(MemoryId::new(TARGET_ID));
    fixture.write(newer).await;
    fixture.reindex_events();

    let artifact = fixture.artifact(TARGET_ID).await;

    assert_eq!(artifact.supersedes.len(), 1);
    assert_eq!(artifact.supersedes[0].id, MemoryId::new(OLD_ID));
    assert_eq!(artifact.superseded_by.len(), 1);
    assert_eq!(artifact.superseded_by[0].id, MemoryId::new(NEWER_ID));
}

struct Fixture {
    roots: Roots,
    substrate: Substrate,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_trustartifact".to_owned()) },
        )
        .await
        .expect("init substrate");
        std::mem::forget(temp);
        Self { roots, substrate }
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
            .expect("write memory");
    }

    async fn write_encrypted(&self, mut memory: Memory) {
        memory.frontmatter.sensitivity = Sensitivity::Confidential;
        memory.frontmatter.retrieval_policy.index_body = false;
        memory.frontmatter.retrieval_policy.index_embeddings = false;
        self.substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: b"ciphertext".to_vec(),
                safe_index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .expect("write encrypted memory");
    }

    async fn artifact(&self, id: &str) -> memoryd::trust_artifact::TrustArtifact {
        TrustArtifactBuilder::new(&self.substrate)
            .with_now(Utc.with_ymd_and_hms(2026, 5, 2, 0, 0, 0).single().expect("fixture time"))
            .build(&MemoryId::new(id))
            .await
            .expect("build trust artifact")
    }

    async fn append_recent_recall(&self, event_id: &str, seq: u64, memory_id: &str) {
        self.append_recall(RecallFixture::new(event_id, seq, memory_id, TimestampParts::new(2026, 5, 1, 0))).await;
    }

    async fn append_recall(&self, fixture: RecallFixture<'_>) {
        append_event(&self.roots.repo.join("events/dev_peerdevice01.jsonl"), &recall_event(fixture))
            .expect("append recall event");
    }

    async fn append_old_recall(&self, event_id: &str, seq: u64, memory_id: &str) {
        append_event(
            &self.roots.repo.join("events/dev_peerdevice01.jsonl"),
            &recall_event(RecallFixture {
                event_id,
                device: "dev_peerdevice01",
                seq,
                memory_id,
                timestamp: timestamp_from_parts(TimestampParts::new(2026, 3, 1, 0)),
            }),
        )
        .expect("append old recall event");
    }

    fn reindex_events(&self) {
        self.substrate.doctor_reindex_events_log().expect("reindex events log");
    }

    fn insert_governance_decision(&self, memory_id: &str) {
        let connection = Connection::open(self.roots.runtime.join("index.sqlite")).expect("open index");
        connection
            .execute(
                "INSERT INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
                 VALUES (?1, ?2, ?3, 'governance_decision', ?4, ?5, ?6)",
                (
                    format!("evt_governance_{memory_id}"),
                    "dev_trustartifact",
                    10_000_i64,
                    memory_id,
                    fixture_time_rfc3339(),
                    json!({
                        "policy_applied": "project-standard@v2",
                        "policy_source": "disk",
                        "confidence_floor_pass": "pass (0.95 >= 0.80)",
                        "grounding_satisfied": "2 source refs resolved",
                        "contradiction_result": "none detected",
                        "tombstone_enforced": "no matching tombstone",
                        "sensitivity_gate_result": "pass (internal)"
                    })
                    .to_string(),
                ),
            )
            .expect("insert governance decision");
    }
}

struct RecallFixture<'a> {
    event_id: &'a str,
    device: &'a str,
    seq: u64,
    memory_id: &'a str,
    timestamp: chrono::DateTime<Utc>,
}

impl<'a> RecallFixture<'a> {
    fn new(event_id: &'a str, seq: u64, memory_id: &'a str, timestamp: TimestampParts) -> Self {
        Self { event_id, device: "dev_peerdevice01", seq, memory_id, timestamp: timestamp_from_parts(timestamp) }
    }
}

fn recall_event(fixture: RecallFixture<'_>) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(fixture.event_id),
        at: fixture.timestamp,
        device: DeviceId::new(fixture.device),
        seq: fixture.seq,
        operation_id: Some(OperationId::new(format!("op_{}", fixture.event_id))),
        kind: EventKind::RecallHit { id: MemoryId::new(fixture.memory_id), recalled_at: fixture.timestamp },
        crc32c: 0,
    }
}

fn sample_memory(id: &str, summary: &str, body: &str) -> Memory {
    let now = Utc.with_ymd_and_hms(2026, 5, 1, 10, 0, 0).single().expect("fixture time");
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Project,
            summary: summary.to_owned(),
            confidence: 0.95,
            original_confidence: Some(0.90),
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
                component: Some("trust-artifact-test".to_owned()),
            },
            namespace: Some("project:agent-memory".to_owned()),
            canonical_namespace_id: Some("agent-memory".to_owned()),
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: Some("codex-cli".to_owned()),
                harness: Some("codex-cli".to_owned()),
                harness_version: None,
                session_id: Some("sess_trust".to_owned()),
                subagent_id: None,
                device: Some("dev_trustartifact".to_owned()),
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
                max_scope: Scope::Project,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "project-standard@v2".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: [
                ("confidence_reason".to_owned(), json!("user confirmed; corroborated by 2 sources")),
                (
                    "governance_decision".to_owned(),
                    json!({
                        "policy_applied": "project-standard@v2",
                        "policy_source": "disk",
                        "confidence_floor_pass": "pass (0.95 >= 0.80)",
                        "grounding_satisfied": "2 source refs resolved",
                        "contradiction_result": "none detected",
                        "tombstone_enforced": "no matching tombstone",
                        "sensitivity_gate_result": "pass (internal)"
                    }),
                ),
                ("privacy_scan".to_owned(), json!({"labels_detected": ["none"], "storage_action": "plaintext"})),
            ]
            .into_iter()
            .collect(),
        },
        body: body.to_owned(),
        path: Some(RepoPath::new(format!("projects/agent-memory/{id}.md"))),
    }
}

fn write_web_artifact(fixture: &Fixture) -> String {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").expect("artifact id");
    let extracted_text = "bounded exact quote from the captured page".to_string();
    let excerpts = vec![ExcerptRecord {
        excerpt_id: "quote_0001".to_string(),
        artifact_id: artifact_id.clone(),
        quote: "bounded exact quote".to_string(),
        quote_sha256: sha256_prefixed("bounded exact quote".as_bytes()),
        locator: ExcerptLocator::ByteRange { start: 0, end: 19 },
        match_kind: ExcerptMatchKind::Exact,
        created_at: Utc.with_ymd_and_hms(2026, 5, 5, 18, 0, 0).single().expect("fixture time"),
    }];
    let excerpts_text = excerpts_jsonl(&excerpts).expect("excerpts jsonl");
    let manifest = WebCaptureManifest {
        schema_version: 2,
        artifact_id: artifact_id.clone(),
        kind: "web_capture".to_string(),
        original_url: "https://example.com/report".to_string(),
        final_url: "https://example.com/report".to_string(),
        redirect_chain: Vec::new(),
        captured_at: Utc.with_ymd_and_hms(2026, 5, 5, 18, 0, 0).single().expect("fixture time"),
        capture_method: CaptureMethod::HttpStaticV1,
        request: CaptureRequestSnapshot::default(),
        response: CaptureResponseSnapshot { http_status: 200, ..CaptureResponseSnapshot::default() },
        raw_sha256: Some(sha256_prefixed(b"full raw captured page body")),
        raw_zstd_sha256: None,
        raw_encrypted_sha256: None,
        raw_storage: RawStorage::OmittedPrivacy,
        raw_omitted_reason: Some("privacy".to_string()),
        extracted_text_storage: ExtractedTextStorage::Plaintext,
        encryption_envelope: None,
        extracted_text_sha256: Some(sha256_prefixed(extracted_text.as_bytes())),

        extracted_text_encrypted_sha256: None,
        excerpts_sha256: sha256_prefixed(excerpts_text.as_bytes()),
        raw_byte_len: "full raw captured page body".len(),
        extracted_text_byte_len: Some(extracted_text.len()),

        extracted_text_encrypted_byte_len: None,
        capture_status: CaptureStatus::CompleteTextOnly,
        warnings: Vec::new(),
        merge_conflict: None,
    };
    ArtifactStore::new(fixture.roots.repo.clone())
        .write_web_capture(&WebCaptureArtifact {
            manifest,
            extracted_text,
            excerpts,
            raw_bytes: None,
            encrypted_extracted_bytes: None,
            encrypted_raw_bytes: None,
        })
        .expect("write web artifact");
    format!("webcap:{artifact_id}#quote_0001")
}

struct TimestampParts {
    year: i32,
    month: u32,
    day: u32,
    second: u32,
}

impl TimestampParts {
    const fn new(year: i32, month: u32, day: u32, second: u32) -> Self {
        Self { year, month, day, second }
    }
}

fn timestamp_from_parts(parts: TimestampParts) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(parts.year, parts.month, parts.day, 12, 30, parts.second).single().expect("fixture time")
}

fn fixture_time_rfc3339() -> String {
    Utc.with_ymd_and_hms(2026, 5, 1, 10, 0, 1).single().expect("fixture time").to_rfc3339()
}
