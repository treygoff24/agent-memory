use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use memory_substrate::index::Index;
use memory_substrate::{
    AuxScope, MemoryStatus, RecallIndexQuery, RecallIndexRow, Scope, Sensitivity, Substrate, SubstrateResult,
};

use crate::dynamics::strength::{strength, StrengthFacts, StrengthWeights};
use crate::dynamics::usage::{distinct_sources_for_conn, recall_usage_for_conn};
use crate::recall::types::{EntityMatchKind, OmissionReason, RecallOmission, RecallSectionName};

#[derive(Debug, Clone, PartialEq)]
pub struct RecallCandidate {
    pub id: String,
    pub row: RecallIndexRow,
    pub entity_match: EntityMatchKind,
    /// Use-driven memory strength in `[0, 1]` (memory-dynamics-v0.1 §3).
    ///
    /// `None` when dynamics is disabled, or when the usage query failed and the
    /// block falls back to a structural-only ranking (soft failure, spec §3).
    /// `rank.rs` reads this to add the bounded `strength_points` term; absent →
    /// zero points.
    pub strength: Option<f64>,
}

impl RecallCandidate {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn with_entity_match(mut self, entity_match: EntityMatchKind) -> Self {
        self.entity_match = entity_match;
        self
    }

    pub fn with_strength(mut self, strength: Option<f64>) -> Self {
        self.strength = strength;
        self
    }
}

impl From<RecallIndexRow> for RecallCandidate {
    fn from(row: RecallIndexRow) -> Self {
        Self { id: row.id.to_string(), row, entity_match: EntityMatchKind::None, strength: None }
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

    // `statuses` accepts both lifecycle states in a single query, so merge
    // Active+Pinned into one call per prefix rather than issuing two queries
    // (each of which fans out 3-4 auxiliary hydration queries). Dedup by id
    // keeps the merge behavior-preserving.
    for namespace_prefix in &request.namespace_prefixes {
        let query = RecallIndexQuery {
            namespace_prefix: Some(namespace_prefix.clone()),
            statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned],
            passive_recall_only: true,
            updated_since: request.updated_since,
            match_terms: Vec::new(),
            // Omission/recall scoring is scalar-only, but these recalled rows
            // also feed `active_entity_ids`/`startup_salient_entities`, which
            // read `row.entities` to match dream-question pending-attention and
            // seed the relevance gate. Hydrate entities; tags/aliases stay unused.
            hydrate: AuxScope::Entities,
            // Ranking/omission reads none of the identity/merge-diagnostics
            // fields, so skip the per-row json_extract for them on this hot path.
            source_identity: false,
        };
        for row in reader.query_recall_index(query).await? {
            rows.entry(row.id.to_string()).or_insert(row);
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

/// Strength hydration inputs (memory-dynamics-v0.1 §3).
pub struct StrengthHydration {
    pub weights: StrengthWeights,
    pub tau_days: f64,
}

/// Hydrate use-driven `strength` onto each candidate in one batched usage query
/// (memory-dynamics-v0.1 §3 hydration rule).
///
/// All candidate ids are fetched through **one** `events_log` recall-usage query
/// and **one** supersession-chain corroboration query — no per-candidate round
/// trips. The pool maximum recall count is computed over this active candidate
/// set (the `freq_norm` denominator). Strength is then attached to each candidate.
///
/// Returns `true` when hydration succeeded. On any index/query error the
/// candidates are left with `strength = None` (structural-only ranking) and the
/// function returns `false` so the caller can flag `dynamics_degraded` — never a
/// hard recall failure (spec §3 soft-failure rule).
pub fn hydrate_candidate_strength(
    index: &Index,
    candidates: &mut [RecallCandidate],
    hydration: &StrengthHydration,
    now: DateTime<Utc>,
) -> bool {
    if candidates.is_empty() {
        return true;
    }

    let connection = index.connection();

    let ids = candidates.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>();
    let usage = match recall_usage_for_conn(connection, &ids, now) {
        Ok(usage) => usage,
        Err(error) => {
            tracing::warn!(%error, "dynamics: recall-usage query failed; ranking structural-only");
            return false;
        }
    };
    let distinct_sources = match distinct_sources_for_conn(connection, &ids) {
        Ok(sources) => sources,
        Err(error) => {
            tracing::warn!(%error, "dynamics: corroboration query failed; ranking structural-only");
            return false;
        }
    };

    let max_recall = candidates
        .iter()
        .map(|candidate| usage.get(candidate.id.as_str()).map_or(0, |summary| summary.count))
        .max()
        .unwrap_or(0);

    for candidate in candidates.iter_mut() {
        let summary = usage.get(candidate.id.as_str()).copied().unwrap_or_default();
        let facts = StrengthFacts {
            recall_count_30d: summary.count,
            last_recalled_at: summary.last_recalled_at,
            max_recall_30d_active: max_recall,
            distinct_sources: distinct_sources.get(candidate.id.as_str()).copied().unwrap_or(0),
        };
        candidate.strength = Some(strength(facts, hydration.weights, hydration.tau_days, now));
    }

    true
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
