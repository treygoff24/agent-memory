use chrono::{DateTime, Utc};
use memory_substrate::{
    Entity, MemoryId, MemoryStatus, RecallIndexQuery, RecallIndexRow, RepoPath, Scope, Sensitivity, SourceKind,
};
use memoryd::recall::{
    collect_recall_candidates, collect_recall_candidates_from_index, OmissionReason, RecallCollectionRequest,
    RecallIndexFuture, RecallIndexReader, RecallSectionName,
};

#[test]
fn active_and_pinned_rows_recall_as_sorted_fact_candidates() {
    let rows = vec![
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Active),
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Pinned),
    ];

    let collected = collect_recall_candidates(RecallSectionName::Identity, rows);

    assert_eq!(collected.pending_attention_count, 0);
    assert_eq!(collected.omitted, Vec::new());
    assert_eq!(
        collected.facts.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000001_000001", "mem_20260430_0000000000000002_000002",]
    );
}

#[test]
fn inactive_lifecycle_rows_do_not_recall_as_facts() {
    let rows = vec![
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Candidate),
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Quarantined),
        row("mem_20260430_0000000000000003_000003", MemoryStatus::Tombstoned),
        row("mem_20260430_0000000000000004_000004", MemoryStatus::Superseded),
        row("mem_20260430_0000000000000005_000005", MemoryStatus::Archived),
    ];

    let collected = collect_recall_candidates(RecallSectionName::ProjectState, rows);

    assert!(collected.facts.is_empty());
    assert_eq!(
        collected.omitted.iter().map(|omission| omission.reason).collect::<Vec<_>>(),
        vec![
            OmissionReason::StatusExcluded,
            OmissionReason::StatusExcluded,
            OmissionReason::Tombstoned,
            OmissionReason::Superseded,
            OmissionReason::StatusExcluded,
        ]
    );
}

#[test]
fn passive_recall_false_suppresses_fact_recall() {
    let mut memory = row("mem_20260430_0000000000000001_000001", MemoryStatus::Pinned);
    memory.passive_recall = false;

    let collected = collect_recall_candidates(RecallSectionName::Identity, vec![memory]);

    assert!(collected.facts.is_empty());
    assert_eq!(collected.omitted[0].reason, OmissionReason::PassiveRecallDisabled);
}

#[test]
fn pending_confirmation_human_review_and_review_state_suppress_facts_but_count_attention() {
    let mut requires_confirmation = row("mem_20260430_0000000000000001_000001", MemoryStatus::Active);
    requires_confirmation.requires_user_confirmation = true;
    let mut human_review_required = row("mem_20260430_0000000000000002_000002", MemoryStatus::Pinned);
    human_review_required.human_review_required = true;
    let mut pending_review = row("mem_20260430_0000000000000003_000003", MemoryStatus::Active);
    pending_review.review_state = Some("pending".to_owned());
    let mut approved_review = row("mem_20260430_0000000000000004_000004", MemoryStatus::Active);
    approved_review.review_state = Some("approved".to_owned());

    let collected = collect_recall_candidates(
        RecallSectionName::PendingAttention,
        vec![requires_confirmation, human_review_required, pending_review, approved_review],
    );

    assert_eq!(collected.pending_attention_count, 3);
    assert_eq!(
        collected.facts.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000004_000004",]
    );
    assert!(collected.omitted.iter().all(|omission| omission.reason == OmissionReason::ReviewPending));
}

#[tokio::test]
async fn collection_queries_stream_a_recall_index_without_envelope_hydration() {
    let updated_since = instant("2026-04-23T12:00:00Z");
    let mut index = RecordingRecallIndex::new(vec![
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Pinned),
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Active),
    ]);

    let collected = collect_recall_candidates_from_index(
        &mut index,
        RecallCollectionRequest {
            section: RecallSectionName::RecentMemory,
            namespace_prefixes: vec!["me".to_owned(), "project:proj_agent_memory".to_owned()],
            updated_since: Some(updated_since),
        },
    )
    .await
    .expect("recall index collection succeeds");

    assert_eq!(index.envelope_reads, 0, "collection must not hydrate envelopes while enumerating candidates");
    assert_eq!(index.queries.len(), 4);
    assert_eq!(
        index.queries.iter().map(|query| query.namespace_prefix.as_deref()).collect::<Vec<_>>(),
        vec![Some("me"), Some("me"), Some("project:proj_agent_memory"), Some("project:proj_agent_memory")]
    );
    assert!(index.queries.iter().all(|query| query.passive_recall_only));
    assert!(index.queries.iter().all(|query| query.updated_since == Some(updated_since)));
    assert_eq!(
        index.queries.iter().map(|query| query.statuses.as_slice()).collect::<Vec<_>>(),
        vec![
            &[MemoryStatus::Active][..],
            &[MemoryStatus::Pinned][..],
            &[MemoryStatus::Active][..],
            &[MemoryStatus::Pinned][..],
        ]
    );
    assert_eq!(
        collected.facts.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000001_000001", "mem_20260430_0000000000000002_000002"]
    );
}

#[test]
fn max_scope_sensitivity_and_metadata_only_rows_do_not_recall_factual_bodies() {
    let mut out_of_scope = row("mem_20260430_0000000000000001_000001", MemoryStatus::Active);
    out_of_scope.scope = Scope::Org;
    out_of_scope.max_scope = Scope::User;
    let mut encrypted_confidential = row("mem_20260430_0000000000000002_000002", MemoryStatus::Active);
    encrypted_confidential.sensitivity = Sensitivity::Confidential;
    encrypted_confidential.index_body = false;
    let mut metadata_only = row("mem_20260430_0000000000000003_000003", MemoryStatus::Pinned);
    metadata_only.index_body = false;
    let safe = row("mem_20260430_0000000000000004_000004", MemoryStatus::Pinned);

    let collected = collect_recall_candidates(
        RecallSectionName::RecentMemory,
        vec![out_of_scope, encrypted_confidential, metadata_only, safe],
    );

    assert_eq!(
        collected.facts.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000004_000004"]
    );
    assert_eq!(
        collected.omitted.iter().map(|omission| omission.reason).collect::<Vec<_>>(),
        vec![
            OmissionReason::NamespaceOutOfScope,
            OmissionReason::EncryptedBodyHidden,
            OmissionReason::EncryptedBodyHidden,
        ]
    );
}

struct RecordingRecallIndex {
    rows: Vec<RecallIndexRow>,
    queries: Vec<RecallIndexQuery>,
    envelope_reads: usize,
}

impl RecordingRecallIndex {
    fn new(rows: Vec<RecallIndexRow>) -> Self {
        Self { rows, queries: Vec::new(), envelope_reads: 0 }
    }
}

impl RecallIndexReader for RecordingRecallIndex {
    fn query_recall_index(&mut self, query: RecallIndexQuery) -> RecallIndexFuture<'_> {
        self.queries.push(query.clone());
        let rows = self.rows.iter().filter(|row| query.statuses.contains(&row.status)).cloned().collect::<Vec<_>>();
        Box::pin(async move { Ok(rows) })
    }
}

fn row(id: &str, status: MemoryStatus) -> RecallIndexRow {
    RecallIndexRow {
        id: MemoryId::new(id),
        path: RepoPath::new(format!("me/{id}.md")),
        summary: format!("summary for {id}"),
        status,
        scope: Scope::User,
        canonical_namespace_id: None,
        updated_at: instant("2026-04-30T12:00:00Z"),
        confidence: 0.7,
        source_kind: SourceKind::User,
        sensitivity: Sensitivity::Internal,
        passive_recall: true,
        index_body: true,
        requires_user_confirmation: false,
        review_state: None,
        human_review_required: false,
        max_scope: Scope::User,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: Vec::<Entity>::new(),
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
