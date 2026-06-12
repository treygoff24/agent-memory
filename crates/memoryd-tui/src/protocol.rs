//! Plain-data transfer types shared between the daemon client and the UI app.
//!
//! These types form a leaf module so that [`crate::client`] can construct and
//! consume daemon snapshots without depending on [`crate::app`] (the UI
//! orchestrator). Keeping the DTOs here breaks the former `app` <-> `client`
//! module cycle: both sides now depend on this leaf instead of each other.

use std::path::Path;

use crate::inbox::{InboxFilter, InboxItem};
use crate::state::ScoreBreakdown;
use crate::widgets::trust_artifact::TrustArtifact;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewAction {
    Approve,
    Reject,
    Forget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RealityCheckAction {
    Confirm,
    Correct { new_body: String },
    Forget,
    NotRelevant,
    SkipWeek,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonCall {
    Review { action: ReviewAction, memory_id: String },
    RealityCheck { action: RealityCheckAction, session_id: String, memory_id: String },
    ForceRefresh,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DaemonSnapshot {
    pub version: String,
    pub footer_hint: String,
    pub daemon_state: String,
    pub review_queue: Vec<ReviewQueueRow>,
    pub conflicts: Vec<ConflictRow>,
    pub recall: Vec<RecallHitRow>,
    pub dreams: Vec<DreamRow>,
    pub due: Vec<RealityCheckRow>,
    pub memories: Vec<MemoryRow>,
    pub trust_artifact: Option<TrustArtifact>,
}

impl DaemonSnapshot {
    pub fn loading(socket_path: &Path) -> Self {
        let mut snapshot = Self::empty();
        snapshot.daemon_state = format!("loading {}", socket_path.display());
        snapshot
    }

    pub fn empty() -> Self {
        Self {
            version: "v1.0.0".to_string(),
            footer_hint: "?:help  q:quit".to_string(),
            daemon_state: "loading".to_string(),
            review_queue: Vec::new(),
            conflicts: Vec::new(),
            recall: Vec::new(),
            dreams: Vec::new(),
            due: Vec::new(),
            memories: Vec::new(),
            trust_artifact: None,
        }
    }

    pub fn sample() -> Self {
        Self {
            version: "v1.0.0".to_string(),
            footer_hint: "?:help  q:quit".to_string(),
            daemon_state: "running".to_string(),
            review_queue: vec![
                ReviewQueueRow {
                    id: "mem_20260501_0123456789abcdef_000001".to_string(),
                    title: "Prefer CITEXT for email columns".to_string(),
                    namespace: "project:atlasos".to_string(),
                    status: "candidate".to_string(),
                    reason: Some("requires_user_confirmation".to_string()),
                },
                ReviewQueueRow {
                    id: "mem_20260501_0123456789abcdef_000007".to_string(),
                    title: "Dream candidate needs confirmation".to_string(),
                    namespace: "project:agent-memory".to_string(),
                    status: "dream_low_confidence".to_string(),
                    reason: Some("dream_low_confidence".to_string()),
                },
            ],
            conflicts: vec![ConflictRow {
                id: "mem_20260501_0123456789abcdef_000002".to_string(),
                title: "Database connection pool size".to_string(),
                namespace: "project:atlasos".to_string(),
                reason: Some("Pool size: 20 vs Pool size: 30".to_string()),
            }],
            recall: vec![RecallHitRow {
                id: "mem_20260501_0123456789abcdef_000009".to_string(),
                title: "Deploy target is production ECS".to_string(),
                namespace: "project:atlasos".to_string(),
                age: "11:02".to_string(),
            }],
            dreams: vec![DreamRow {
                id: "dream_project_20260501".to_string(),
                title: "Daily synthesis summary ready".to_string(),
                namespace: "project:agent-memory".to_string(),
            }],
            due: vec![RealityCheckRow {
                id: "mem_20260501_0123456789abcdef_000004".to_string(),
                title: "SSH key rotation every 90d".to_string(),
                namespace: "me".to_string(),
                score: "0.82".to_string(),
                breakdown: ScoreBreakdown {
                    recency: 0.91,
                    recall_frequency: 0.20,
                    corroboration: 0.0,
                    confidence_decay: 0.65,
                    sensitivity: 1.0,
                },
            }],
            memories: vec![MemoryRow {
                id: "mem_20260501_0123456789abcdef_000010".to_string(),
                title: "Agent memory uses private daemon socket".to_string(),
                namespace: "agent".to_string(),
            }],
            trust_artifact: Some(sample_trust_artifact()),
        }
    }

    pub fn inbox_items(&self) -> Vec<InboxItem> {
        let mut sources = vec![
            self.conflicts
                .iter()
                .map(|row| InboxItem::Conflict {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    reason: row.reason.clone(),
                    age_label: "now".to_string(),
                })
                .collect(),
            self.due
                .iter()
                .map(|row| InboxItem::RealityCheckDue {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    score: row.score.clone(),
                    age_label: "due".to_string(),
                })
                .collect(),
            self.review_queue
                .iter()
                .map(|row| InboxItem::ReviewCandidate {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    reason: row.reason.clone(),
                    age_label: row.status.clone(),
                })
                .collect(),
            self.dreams
                .iter()
                .map(|row| InboxItem::DreamOutput {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    age_label: "today".to_string(),
                })
                .collect(),
            self.recall
                .iter()
                .map(|row| InboxItem::RecallHit {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    age_label: row.age.clone(),
                })
                .collect(),
            self.memories
                .iter()
                .map(|row| InboxItem::Memory {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    age_label: "active".to_string(),
                })
                .collect(),
        ];
        crate::inbox::ranking::merge_and_filter(std::mem::take(&mut sources), InboxFilter::All, 50)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewQueueRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub status: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallHitRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub age: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DreamRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RealityCheckRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub score: String,
    pub breakdown: ScoreBreakdown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
}

fn sample_trust_artifact() -> TrustArtifact {
    serde_json::from_value(serde_json::json!({
        "id": "mem_20260501_0123456789abcdef_000009",
        "namespace": "project:atlasos",
        "status": "active",
        "sensitivity": "internal",
        "source": "substrate:projects/atlasos/deploy-target.md",
        "title": { "kind": "plaintext", "value": "Deploy target is production ECS" },
        "body": { "kind": "plaintext", "value": "The ECS cluster in us-east-1 is the production deployment target." },
        "current_confidence": "0.95",
        "original_confidence": "0.90",
        "confidence_reason": "user confirmed; policy-promoted",
        "trust_summary": "high trust; policy-promoted",
        "recall": { "total": 28, "last_30_days": 12, "last_recalled_at": "2026-05-01T11:02:00Z", "strength": "0.74" },
        "provenance_chain": [{ "timestamp": "2026-04-30T14:22:00Z", "kind": "write_committed", "summary": "written by codex-cli", "evidence": "sess_abc123", "device": "macbook" }],
        "policy_decisions": [{ "policy_applied": "project-standard@v2", "policy_source": "disk", "confidence_floor_pass": "pass", "grounding_satisfied": "2 source refs resolved", "contradiction_result": "none detected", "tombstone_enforced": "no matching tombstone", "sensitivity_gate_result": "pass" }],
        "privacy_scan": { "labels_detected": ["none"], "storage_action": "plaintext" },
        "supersedes": [],
        "superseded_by": [],
        "sync_state": { "devices": ["macbook"], "merge_status": "clean", "claim_lock_status": null }
    }))
    .expect("sample trust artifact fixture must match daemon DTO")
}
