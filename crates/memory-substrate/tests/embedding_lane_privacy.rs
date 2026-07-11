use std::collections::BTreeSet;

use memory_substrate::index::{held_local_embedding_jobs, open_index, reconcile_pending_jobs, Index};
use memory_substrate::*;

#[test]
fn api_lane_fetch_and_counts_exclude_non_plaintext_tiers() {
    let fixture = PrivacyFixture::new();

    assert_eq!(
        fixture.memory_index_state(MASKED_PROJECTION_ID),
        (false, Sensitivity::Confidential.as_db_str().to_string()),
        "safe_body projections are indexed text, not metadata-only, but keep the persisted sensitive tier"
    );
    assert_eq!(fixture.pending_texts(EmbeddingLaneEligibility::AllTiers), all_texts());
    assert_eq!(fixture.pending_texts(EmbeddingLaneEligibility::PlaintextOnly), plaintext_texts());
    assert_eq!(
        reconcile_pending_jobs(fixture.index.connection(), &fixture.triple, EmbeddingLaneEligibility::AllTiers)
            .expect("all-tier active count"),
        5
    );
    assert_eq!(
        held_local_embedding_jobs(fixture.index.connection(), &fixture.triple, EmbeddingLaneEligibility::AllTiers)
            .expect("all-tier held-local count"),
        0
    );
    assert_eq!(
        reconcile_pending_jobs(fixture.index.connection(), &fixture.triple, EmbeddingLaneEligibility::PlaintextOnly)
            .expect("plaintext active count"),
        2
    );
    assert_eq!(
        held_local_embedding_jobs(fixture.index.connection(), &fixture.triple, EmbeddingLaneEligibility::PlaintextOnly)
            .expect("plaintext held-local count"),
        3
    );
}

#[test]
fn api_lane_reconcile_enqueues_only_plaintext_tiers() {
    let mut all_tiers = PrivacyFixture::new();
    all_tiers.clear_pending_jobs();

    assert_eq!(
        all_tiers.index.reconcile_active_embedding_jobs(EmbeddingLaneEligibility::AllTiers).expect("all reconcile"),
        5
    );
    assert_eq!(all_tiers.pending_texts(EmbeddingLaneEligibility::AllTiers), all_texts());
    assert_eq!(
        reconcile_pending_jobs(all_tiers.index.connection(), &all_tiers.triple, EmbeddingLaneEligibility::AllTiers)
            .expect("all-tier active count"),
        5
    );
    assert_eq!(
        held_local_embedding_jobs(all_tiers.index.connection(), &all_tiers.triple, EmbeddingLaneEligibility::AllTiers)
            .expect("all-tier held-local count"),
        0
    );

    let mut plaintext_only = PrivacyFixture::new();
    plaintext_only.clear_pending_jobs();

    assert_eq!(
        plaintext_only
            .index
            .reconcile_active_embedding_jobs(EmbeddingLaneEligibility::PlaintextOnly)
            .expect("plaintext reconcile"),
        2
    );
    assert_eq!(plaintext_only.pending_texts(EmbeddingLaneEligibility::AllTiers), plaintext_texts());
    assert_eq!(
        plaintext_only
            .index
            .reconcile_active_embedding_jobs(EmbeddingLaneEligibility::PlaintextOnly)
            .expect("second plaintext reconcile"),
        0,
        "held-local sensitive chunks must not be resurrected on the next reconcile"
    );
    assert_eq!(plaintext_only.pending_texts(EmbeddingLaneEligibility::AllTiers), plaintext_texts());
}

#[test]
fn sensitivity_api_lane_allowlist_is_plaintext_storage_tiers() {
    assert!(Sensitivity::Public.api_lane_eligible());
    assert!(Sensitivity::Internal.api_lane_eligible());
    assert!(!Sensitivity::Confidential.api_lane_eligible());
    assert!(!Sensitivity::Personal.api_lane_eligible());
    assert_eq!(
        Sensitivity::api_lane_eligible_db_strs(),
        vec![Sensitivity::Public.as_db_str(), Sensitivity::Internal.as_db_str()]
    );
}

struct PrivacyFixture {
    _temp: tempfile::TempDir,
    index: Index,
    triple: EmbeddingTriple,
}

impl PrivacyFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let triple =
            EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "privacy-fence".to_string(), dimension: 3 };
        let connection = open_index(&temp.path().join("index.sqlite")).expect("open index");
        let mut index = Index::with_active_embedding(connection, triple.clone());
        for (id, sensitivity, body) in [
            ("mem_20260709_aaaaaaaaaaaaaaaa_000001", Sensitivity::Public, PUBLIC_TEXT),
            ("mem_20260709_aaaaaaaaaaaaaaaa_000002", Sensitivity::Internal, INTERNAL_TEXT),
            ("mem_20260709_aaaaaaaaaaaaaaaa_000003", Sensitivity::Confidential, CONFIDENTIAL_TEXT),
            ("mem_20260709_aaaaaaaaaaaaaaaa_000004", Sensitivity::Personal, PERSONAL_TEXT),
            (MASKED_PROJECTION_ID, Sensitivity::Confidential, MASKED_PROJECTION_TEXT),
        ] {
            index.upsert_memory(&memory(id, sensitivity, body), false).expect("upsert memory");
        }
        Self { _temp: temp, index, triple }
    }

    fn clear_pending_jobs(&self) {
        self.index.connection().execute("DELETE FROM pending_embedding_jobs", []).expect("clear pending jobs");
    }

    fn pending_texts(&self, eligibility: EmbeddingLaneEligibility) -> BTreeSet<String> {
        self.index
            .pending_embedding_jobs(20, eligibility)
            .expect("pending jobs")
            .into_iter()
            .map(|job| job.text)
            .collect()
    }

    fn memory_index_state(&self, id: &str) -> (bool, String) {
        self.index
            .connection()
            .query_row("SELECT metadata_only, sensitivity FROM memories WHERE id = ?1", [id], |row| {
                Ok((row.get::<_, i64>(0)? != 0, row.get::<_, String>(1)?))
            })
            .expect("memory row")
    }
}

const PUBLIC_TEXT: &str = "public api lane body";
const INTERNAL_TEXT: &str = "internal api lane body";
const CONFIDENTIAL_TEXT: &str = "confidential held local body";
const PERSONAL_TEXT: &str = "personal held local body";
const MASKED_PROJECTION_ID: &str = "mem_20260709_aaaaaaaaaaaaaaaa_000005";
const MASKED_PROJECTION_TEXT: &str = "masked safe_body projection";

fn all_texts() -> BTreeSet<String> {
    [PUBLIC_TEXT, INTERNAL_TEXT, CONFIDENTIAL_TEXT, PERSONAL_TEXT, MASKED_PROJECTION_TEXT]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn plaintext_texts() -> BTreeSet<String> {
    [PUBLIC_TEXT, INTERNAL_TEXT].into_iter().map(str::to_string).collect()
}

fn memory(id: &str, sensitivity: Sensitivity, body: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-07-09T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "embedding lane privacy".to_string(),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity,
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
                component: Some("test".to_string()),
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
            extras: Default::default(),
        },
        body: body.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
