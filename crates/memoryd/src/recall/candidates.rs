use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use memory_substrate::{
    MemoryStatus, RecallIndexQuery, RecallIndexRow, Scope, Sensitivity, Substrate, SubstrateResult,
};

use crate::recall::types::{EntityMatchKind, OmissionReason, RecallOmission, RecallSectionName};

#[derive(Debug, Clone, PartialEq)]
pub struct RecallCandidate {
    pub id: String,
    pub row: RecallIndexRow,
    pub entity_match: EntityMatchKind,
}

impl RecallCandidate {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn with_entity_match(mut self, entity_match: EntityMatchKind) -> Self {
        self.entity_match = entity_match;
        self
    }
}

impl From<RecallIndexRow> for RecallCandidate {
    fn from(row: RecallIndexRow) -> Self {
        Self { id: row.id.to_string(), row, entity_match: EntityMatchKind::None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateCollection {
    pub facts: Vec<RecallCandidate>,
    pub omitted: Vec<RecallOmission>,
    pub pending_attention_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallCollectionRequest {
    pub section: RecallSectionName,
    pub namespace_prefixes: Vec<String>,
    pub updated_since: Option<DateTime<Utc>>,
}

pub type RecallIndexFuture<'a> = Pin<Box<dyn Future<Output = SubstrateResult<Vec<RecallIndexRow>>> + Send + 'a>>;

pub trait RecallIndexReader {
    fn query_recall_index(&self, query: RecallIndexQuery) -> RecallIndexFuture<'_>;
}

impl RecallIndexReader for Substrate {
    fn query_recall_index(&self, query: RecallIndexQuery) -> RecallIndexFuture<'_> {
        Box::pin(async move { Substrate::query_recall_index(self, query).await })
    }
}

pub async fn collect_recall_candidates_from_index(
    reader: &impl RecallIndexReader,
    request: RecallCollectionRequest,
) -> SubstrateResult<CandidateCollection> {
    let mut rows = BTreeMap::new();

    for namespace_prefix in &request.namespace_prefixes {
        for status in [MemoryStatus::Active, MemoryStatus::Pinned] {
            let query = RecallIndexQuery {
                namespace_prefix: Some(namespace_prefix.clone()),
                statuses: vec![status],
                passive_recall_only: true,
                updated_since: request.updated_since,
                match_terms: Vec::new(),
            };
            for row in reader.query_recall_index(query).await? {
                rows.entry(row.id.to_string()).or_insert(row);
            }
        }
    }

    Ok(collect_recall_candidates(request.section, rows.into_values().collect()))
}

pub fn collect_recall_candidates(section: RecallSectionName, rows: Vec<RecallIndexRow>) -> CandidateCollection {
    let mut rows = rows;
    rows.sort_by(|left, right| left.id.cmp(&right.id));

    let mut facts = Vec::new();
    let mut omitted = Vec::new();
    let mut pending_attention_count = 0;

    for row in rows {
        match omission_reason(&row) {
            Some(reason) => {
                if reason == OmissionReason::ReviewPending {
                    pending_attention_count += 1;
                }
                omitted.push(omission(row.id.as_str(), section, reason));
            }
            None => facts.push(RecallCandidate::from(row)),
        }
    }

    CandidateCollection { facts, omitted, pending_attention_count }
}

fn omission_reason(row: &RecallIndexRow) -> Option<OmissionReason> {
    if !row.passive_recall {
        return Some(OmissionReason::PassiveRecallDisabled);
    }
    if has_pending_review(row) {
        return Some(OmissionReason::ReviewPending);
    }

    match row.status {
        MemoryStatus::Active | MemoryStatus::Pinned => body_recall_omission_reason(row),
        MemoryStatus::Superseded => Some(OmissionReason::Superseded),
        MemoryStatus::Tombstoned => Some(OmissionReason::Tombstoned),
        MemoryStatus::Candidate | MemoryStatus::Quarantined | MemoryStatus::Archived => {
            Some(OmissionReason::StatusExcluded)
        }
    }
}

fn has_pending_review(row: &RecallIndexRow) -> bool {
    row.requires_user_confirmation
        || row.human_review_required
        || !review_state_allows_fact(row.review_state.as_deref())
}

fn review_state_allows_fact(review_state: Option<&str>) -> bool {
    review_state.is_none_or(|state| matches!(state, "approved" | "accepted" | "none"))
}

fn body_recall_omission_reason(row: &RecallIndexRow) -> Option<OmissionReason> {
    if !scope_within_max(row.scope, row.max_scope) {
        return Some(OmissionReason::NamespaceOutOfScope);
    }
    if !row.index_body || !matches!(row.sensitivity, Sensitivity::Public | Sensitivity::Internal) {
        return Some(OmissionReason::EncryptedBodyHidden);
    }
    None
}

fn scope_within_max(scope: Scope, max_scope: Scope) -> bool {
    scope_rank(scope) <= scope_rank(max_scope)
}

fn scope_rank(scope: Scope) -> u8 {
    match scope {
        Scope::Subagent => 0,
        Scope::Agent => 1,
        Scope::User => 2,
        Scope::Project => 3,
        Scope::Org => 4,
    }
}

fn omission(id: &str, section: RecallSectionName, reason: OmissionReason) -> RecallOmission {
    RecallOmission { id: Some(id.to_owned()), section, reason, alias: None, colliding_ids: Vec::new() }
}
