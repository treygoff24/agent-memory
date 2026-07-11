use chrono::{DateTime, Utc};
use memory_substrate::{
    Author, AuthorKind, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath,
    RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy,
    WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult, SearchResponse};
use tempfile::TempDir;

#[tokio::test]
async fn fts_only_search_uses_relaxed_or_fallback_for_partial_query() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("quick brown fox", "quick brown fox").await;

    // strict-AND terms would fail; a single matching term must still surface the memory.
    let response = fixture.search("quick alien").await;
    let ResponseResult::Success(ResponsePayload::Search(SearchResponse { hits, .. })) = response.result else {
        panic!("expected search response, got {:?}", response.result);
    };
    assert!(!hits.is_empty(), "relaxed OR fallback should return a hit for partial match");
    assert_eq!(hits[0].id, id.as_str().to_string());
}

#[tokio::test]
async fn fts_only_search_collapses_chunk_hits_to_memory_level() {
    let fixture = Fixture::new().await;
    let id = fixture.write_memory("quick brown fox", "quick brown fox jumps over the lazy dog").await;

    let response = fixture.search("quick").await;
    let ResponseResult::Success(ResponsePayload::Search(SearchResponse { hits, .. })) = response.result else {
        panic!("expected search response, got {:?}", response.result);
    };
    let ids: Vec<_> = hits.iter().map(|h| h.id.clone()).collect();
    assert!(ids.iter().filter(|i| **i == id.as_str()).count() <= 1, "the same memory must appear at most once");
}

struct Fixture {
    _repo: TempDir,
    _runtime: TempDir,
    substrate: Substrate,
}

impl Fixture {
    async fn new() -> Self {
        let repo = tempfile::tempdir().expect("repo tempdir");
        let runtime = tempfile::tempdir().expect("runtime tempdir");
        let substrate = Substrate::init(
            Roots::new(repo.path(), runtime.path()),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_ftssearch".to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _repo: repo, _runtime: runtime, substrate }
    }

    async fn write_memory(&self, summary: &str, body: &str) -> MemoryId {
        let id = self.substrate.next_memory_id().await.expect("id");
        let memory = Memory {
            frontmatter: Frontmatter {
                schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
                id: id.clone(),
                memory_type: MemoryType::Pattern,
                scope: Scope::User,
                summary: summary.to_owned(),
                confidence: 0.8,
                original_confidence: Some(0.8),
                trust_level: TrustLevel::Trusted,
                sensitivity: Sensitivity::Internal,
                status: MemoryStatus::Active,
                created_at: instant("2026-04-01T12:00:00Z"),
                updated_at: instant("2026-04-01T12:00:00Z"),
                observed_at: None,
                author: Author {
                    kind: AuthorKind::User,
                    user_handle: Some("memoryd-test".to_owned()),
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    phase: None,
                    component: None,
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: vec!["fts-search-test".to_owned()],
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::User,
                    reference: Some("fts-search-test".to_owned()),
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
                    max_scope: Scope::User,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: true,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "fts-search-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: Default::default(),
            },
            body: body.to_owned(),
            path: Some(RepoPath::new(format!("me/knowledge/{}.md", id.as_str()))),
        };
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: memory_substrate::EventContext::default(),
                allow_best_effort_durability: true,
                classification: memory_substrate::ClassificationOutcome::Trusted,
            })
            .await
            .expect("memory writes");
        id
    }

    async fn search(&self, query: &str) -> memoryd::protocol::ResponseEnvelope {
        handle_request(
            &self.substrate,
            RequestEnvelope::new(
                "search",
                RequestPayload::Search { query: query.to_owned(), limit: Some(10), include_body: false },
            ),
        )
        .await
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).unwrap().with_timezone(&Utc)
}
