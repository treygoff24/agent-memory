//! Regression coverage for FTS5 query sanitization in `Substrate::query_chunks`.
//!
//! Before sanitization, free-form user queries with FTS5-meaningful punctuation
//! (`-`, `:`, `"`, `^`, `*`, etc.) were forwarded into `MATCH ?1` raw, where
//! FTS5 reinterpreted them as expression syntax. The most visible symptom: a
//! query like `end-to-end` failed with `sqlite error: no such column: to`
//! because FTS5 parsed it as `end NOT to NOT end` and treated `to` as a column
//! qualifier. These tests pin the contract that
//! [`Substrate::query_chunks`] accepts a search string, not an FTS5 expression.

use std::collections::BTreeMap;

use memory_substrate::{
    Author, AuthorKind, ChunkQuery, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};

#[tokio::test]
async fn query_chunks_accepts_hyphenated_user_text() {
    let substrate = init().await;
    let memory_id = "mem_20260428_a1b2c3d4e5f60718_400001";
    write_memory(&substrate, memory_id, "live daemon end-to-end note about caching").await;

    // Before the fix, this query produced
    //   "sqlite error: no such column: to"
    // because FTS5 read `end-to-end` as `end NOT to NOT end`.
    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("end-to-end".to_string()), triple: None, vector: None, namespaces: None })
        .await
        .expect("hyphenated query must not error out of FTS5");

    assert!(hits.iter().any(|hit| hit.memory_id.as_str() == memory_id), "the hyphenated body must be findable");
}

#[tokio::test]
async fn query_chunks_accepts_fts5_operator_keywords_as_plain_terms() {
    let substrate = init().await;
    let memory_id = "mem_20260428_a1b2c3d4e5f60718_400002";
    write_memory(&substrate, memory_id, "the AND operator is just a word here").await;

    // `AND` and `NOT` are FTS5 keywords; sanitization must demote them to
    // plain phrase tokens so user queries containing them don't blow up the
    // expression parser.
    let hits = substrate
        .query_chunks(ChunkQuery {
            text: Some("AND operator".to_string()),
            triple: None,
            vector: None,
            namespaces: None,
        })
        .await
        .expect("operator keywords as plain terms must not error");

    assert!(hits.iter().any(|hit| hit.memory_id.as_str() == memory_id));
}

#[tokio::test]
async fn query_chunks_accepts_double_quotes_in_user_text() {
    let substrate = init().await;
    let memory_id = "mem_20260428_a1b2c3d4e5f60718_400003";
    write_memory(&substrate, memory_id, "she said hello to the agent").await;

    // A stray double-quote in user text would unbalance the FTS5 phrase quoter
    // if not escaped. The sanitizer doubles the quote per FTS5 rules.
    let hits = substrate
        .query_chunks(ChunkQuery {
            text: Some("she said \"hello\"".to_string()),
            triple: None,
            vector: None,
            namespaces: None,
        })
        .await
        .expect("double-quoted user text must not unbalance FTS5 phrase parsing");

    assert!(hits.iter().any(|hit| hit.memory_id.as_str() == memory_id));
}

#[tokio::test]
async fn query_chunks_returns_empty_for_punctuation_only_query() {
    let substrate = init().await;
    write_memory(&substrate, "mem_20260428_a1b2c3d4e5f60718_400004", "any body").await;

    // No alphanumeric tokens means no usable phrases. Rather than feed an
    // empty MATCH (which is a syntax error in FTS5), the sanitizer
    // short-circuits to an empty result set.
    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("--- !@#".to_string()), triple: None, vector: None, namespaces: None })
        .await
        .expect("punctuation-only query must not error");

    assert!(hits.is_empty(), "no usable terms means no hits");
}

#[tokio::test]
async fn query_chunks_still_finds_plain_single_word_queries() {
    // The whole point of sanitization is to be invisible to existing well-formed
    // queries — bare-word lookups must keep working unchanged.
    let substrate = init().await;
    let memory_id = "mem_20260428_a1b2c3d4e5f60718_400005";
    write_memory(&substrate, memory_id, "regression sentinel needle term").await;

    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("needle".to_string()), triple: None, vector: None, namespaces: None })
        .await
        .expect("plain word query");

    assert!(hits.iter().any(|hit| hit.memory_id.as_str() == memory_id));
}

async fn init() -> Substrate {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    // Leak the tempdir so the substrate's filesystem state survives the
    // function return; the OS reaps the temp on process exit.
    std::mem::forget(temp);
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_ftssan".to_string()) })
        .await
        .expect("substrate init")
}

async fn write_memory(substrate: &Substrate, id: &str, body: &str) {
    let memory = build_memory(id, body);
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

fn build_memory(id: &str, body: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "fts sanitization fixture".to_string(),
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
                component: Some("fts-sanitization-test".to_string()),
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
            extras: BTreeMap::new(),
        },
        body: body.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
