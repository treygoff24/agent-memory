use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const STREAM_E_POLICY: &str = "stream-e-v0.7";
pub const MAX_SERIALIZED_OMISSIONS: usize = 64;
pub const DEFAULT_STARTUP_BUDGET_TOKENS: usize = 3_600;
pub const DEFAULT_DELTA_BUDGET_TOKENS: usize = 400;

/// Hook-mode (passive) startup budget. Claude Code caps an injected SessionStart
/// block at 10k chars; Stream E estimates ~`ceil(bytes/4)` tokens, so the default
/// 3600-token budget (~14.4 KB) would spill past the cap. A reduced budget keeps
/// the rendered base block under [`HOOK_BLOCK_CHAR_CAP`] (plan Decision 8).
pub const HOOK_STARTUP_BUDGET_TOKENS: usize = 1_900;

/// Hook-mode (passive) delta budget. Subagent/per-turn deltas append to the live
/// tail (no cached prefix), so the channel stays small to keep injections cheap
/// (plan Decision 8).
pub const HOOK_DELTA_BUDGET_TOKENS: usize = 360;

/// Hard character cap for any passively-injected block. Claude Code drops a
/// SessionStart hook's output past 10k chars to a file, which defeats recall —
/// the passive render path truncates deterministically below this (plan Decision
/// 8, "Size" invariant guard).
pub const HOOK_BLOCK_CHAR_CAP: usize = 10_000;

/// Maximum `recent-memory` entries surfaced in a passive (hook-mode) block.
///
/// The token budget alone admits ~30 scaffolding-heavy entries that overflow
/// [`HOOK_BLOCK_CHAR_CAP`] and truncate mid-entry, so the hook also caps the
/// section by entry *count* to a small, high-signal set. Applied only to the
/// passive path; the active startup path is unbounded as before. The cap is a
/// pure prefix of the rank-ordered input, so the passive block stays
/// byte-deterministic (cache-safety invariant).
pub const HOOK_RECENT_MEMORY_MAX_ENTRIES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMatchKind {
    None,
    Tag,
    ExactLabelOrAlias,
    ExactId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupRequest {
    pub cwd: String,
    pub session_id: String,
    pub harness: String,
    pub harness_version: Option<String>,
    #[serde(default = "default_include_recent")]
    pub include_recent: bool,
    pub since_event_id: Option<String>,
    pub budget_tokens: Option<usize>,
    /// Read-only recall: when `true`, assembly must not mutate substrate or
    /// ranking state (no surface-marker writes, no recall-hit feedback). Set by
    /// the passive hook handler, which fires on every session/turn/subagent.
    /// Stream E §12 read-only-recall contract (2026-06-19 amendment).
    ///
    /// Additive on the wire: skipped when `false`, so existing serialized
    /// request shapes stay byte-identical (protocol-contract stable).
    #[serde(default, skip_serializing_if = "is_false")]
    pub passive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartupResponse {
    pub session_binding: SessionBinding,
    pub recall_block: String,
    pub budget_used_tokens: usize,
    pub recall_explanation: RecallExplanation,
    pub guidance: String,
    #[serde(skip)]
    pub dream_question_omissions: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaRequest {
    pub cwd: String,
    pub session_id: String,
    pub harness: String,
    pub message: String,
    pub budget_tokens: Option<usize>,
    /// Read-only recall: see [`StartupRequest::passive`]. Gates every write on
    /// the per-turn delta recall path. Additive on the wire: skipped when
    /// `false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub passive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaResponse {
    pub delta_block: String,
    pub budget_used_tokens: usize,
    pub guidance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_recall_degraded: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaPeerDelivery {
    pub delivered_at: DateTime<Utc>,
    pub from_harness: String,
    pub from_session_id: String,
    pub to_harness: String,
    pub to_session_id: String,
    pub memory_id: String,
    pub relevance: f64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBinding {
    pub session_id: String,
    pub harness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_version: Option<String>,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectBinding>,
    pub namespaces_in_scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBinding {
    pub canonical_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrent_session_mode: Option<ConcurrentSessionMode>,
    pub resolved_via: ProjectBindingSource,
}

// Canonical definition lives in `memorum_coordination`; re-exported here so the
// long-standing `memoryd::recall::ConcurrentSessionMode` path keeps resolving.
pub use memorum_coordination::ConcurrentSessionMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBindingSource {
    YamlOverride,
    GitRemote,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecallExplanation {
    pub budget_tokens: usize,
    pub budget_used_tokens: usize,
    pub policy: String,
    pub sections: Vec<RecallSectionExplanation>,
    pub omitted: Vec<RecallOmission>,
    pub omitted_truncated_count: u32,
    /// Per-memory use-driven strength (memory-dynamics-v0.1 §3 observability).
    /// Lets an operator see *why* a memory ranked. Empty when dynamics is off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strengths: Vec<RecallStrength>,
    /// `true` when the usage query soft-failed and ranking fell back to
    /// structural-only (spec §3 soft-failure rule).
    #[serde(default, skip_serializing_if = "is_false")]
    pub dynamics_degraded: bool,
}

/// One memory's strength, surfaced in the recall explanation (spec §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecallStrength {
    pub id: String,
    /// Strength in `[0, 1]`, rendered to 2 decimals downstream.
    pub strength: f64,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallSectionExplanation {
    pub name: RecallSectionName,
    pub selected_ids: Vec<String>,
    pub matched_entities: Vec<String>,
    pub budget_used_tokens: usize,
    pub omitted_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallOmission {
    pub id: Option<String>,
    pub section: RecallSectionName,
    pub reason: OmissionReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colliding_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmissionReason {
    BudgetExhausted,
    /// Dropped by the passive hook's fixed entry cap (`HOOK_RECENT_MEMORY_MAX_ENTRIES`),
    /// distinct from token-`BudgetExhausted` so it does not inflate the
    /// budget-exhaustion metric.
    HookEntryCap,
    StatusExcluded,
    PassiveRecallDisabled,
    ReviewPending,
    EncryptedBodyHidden,
    AmbiguousAlias,
    NamespaceOutOfScope,
    Superseded,
    Tombstoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecallSectionName {
    Identity,
    ProjectState,
    EntityRecall,
    RecentMemory,
    PendingAttention,
    RecallExplanation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOmissions {
    pub omitted: Vec<RecallOmission>,
    pub omitted_truncated_count: u32,
}

impl RecallExplanation {
    pub fn empty(budget_tokens: usize) -> Self {
        Self {
            budget_tokens,
            budget_used_tokens: 0,
            policy: STREAM_E_POLICY.to_owned(),
            sections: Vec::new(),
            omitted: Vec::new(),
            omitted_truncated_count: 0,
            strengths: Vec::new(),
            dynamics_degraded: false,
        }
    }
}

impl RecallSectionName {
    pub const STARTUP_ORDER: [Self; 6] = [
        Self::Identity,
        Self::ProjectState,
        Self::EntityRecall,
        Self::RecentMemory,
        Self::PendingAttention,
        Self::RecallExplanation,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::ProjectState => "project-state",
            Self::EntityRecall => "entity-recall",
            Self::RecentMemory => "recent-memory",
            Self::PendingAttention => "pending-attention",
            Self::RecallExplanation => "recall-explanation",
        }
    }
}

impl OmissionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BudgetExhausted => "budget_exhausted",
            Self::HookEntryCap => "hook_entry_cap",
            Self::StatusExcluded => "status_excluded",
            Self::PassiveRecallDisabled => "passive_recall_disabled",
            Self::ReviewPending => "review_pending",
            Self::EncryptedBodyHidden => "encrypted_body_hidden",
            Self::AmbiguousAlias => "ambiguous_alias",
            Self::NamespaceOutOfScope => "namespace_out_of_scope",
            Self::Superseded => "superseded",
            Self::Tombstoned => "tombstoned",
        }
    }
}

impl ProjectBindingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::YamlOverride => "yaml_override",
            Self::GitRemote => "git_remote",
        }
    }
}

pub fn bounded_omissions(mut omissions: Vec<RecallOmission>) -> BoundedOmissions {
    omissions.sort_by(omission_sort);

    let omitted_truncated_count = omissions.len().saturating_sub(MAX_SERIALIZED_OMISSIONS) as u32;
    omissions.truncate(MAX_SERIALIZED_OMISSIONS);

    BoundedOmissions { omitted: omissions, omitted_truncated_count }
}

fn omission_sort(left: &RecallOmission, right: &RecallOmission) -> std::cmp::Ordering {
    let left_key = omission_sort_key(left);
    let right_key = omission_sort_key(right);
    left_key.cmp(&right_key)
}

fn omission_sort_key(omission: &RecallOmission) -> (&str, &str, &str, &str) {
    (
        omission.section.as_str(),
        omission.reason.as_str(),
        omission.alias.as_deref().unwrap_or(""),
        omission.id.as_deref().unwrap_or(""),
    )
}

fn default_include_recent() -> bool {
    true
}
