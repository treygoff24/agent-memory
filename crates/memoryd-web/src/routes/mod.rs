use std::collections::BTreeSet;

use chrono::{DateTime, TimeZone, Utc};
use memoryd::protocol::{ComponentScores, RealityCheckItem};
use memoryd::protocol::{MemoryId, MemoryStatus};
use memoryd::trust_artifact::{
    PolicyDecision, PrivacyScan, ProvenanceEvent, RecallStats, SafeContent, SupersessionLink, SyncState, TrustArtifact,
};
use serde::Serialize;
use serde_json::json;

pub mod audit;
pub mod entity_graph;
pub mod policy_editor;
pub mod reality_check;
pub mod recall_hits;
pub mod review;
pub mod roi;
pub mod search;
pub mod status;
pub mod sync_dashboard;

pub use audit::{audit, audit_temporal, audit_walk};
pub use entity_graph::{entity_detail, entity_graph, EntityDetailResponse, EntityGraphResponse};
pub use policy_editor::{policy_editor_get, policy_editor_post, PolicyEditorResponse};
pub use reality_check::{reality_check, reality_check_history, reality_check_respond, RealityCheckHistoryResponse};
pub use recall_hits::recall_hits;
pub use review::{review_action, review_queue, ReviewActionRequest};
pub use roi::{roi, RoiResponse};
pub use search::search;
pub use status::{notifications_stream, status, StatusDashboardResponse};
pub use sync_dashboard::{sync_dashboard, SyncDashboardResponse};

pub const AUDIT_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000010";
pub const REVIEWABLE_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000001";
pub const REALITY_CHECK_SESSION_ID: &str = "rc_session_task14";

#[derive(Clone, Debug)]
pub struct DashboardData {
    pub status: StatusDashboardResponse,
    pub entity_graph: EntityGraphResponse,
    pub entity_detail: EntityDetailResponse,
    pub roi: RoiFixture,
    pub reality_check_items: Vec<RealityCheckItem>,
    pub reality_check_history: RealityCheckHistoryResponse,
    pub audit_artifact: TrustArtifact,
    pub reviewable_memory_ids: BTreeSet<String>,
    pub notifications: Vec<NotificationSnapshot>,
    pub recall_hits: Vec<memoryd::protocol::RecallHitSummary>,
}

#[derive(Clone, Debug)]
pub struct RoiFixture {
    pub window_30: RoiResponse,
    pub window_90: RoiResponse,
    pub window_365: RoiResponse,
}

#[derive(Clone, Debug, Serialize)]
pub struct NotificationSnapshot {
    pub kind: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

impl Default for DashboardData {
    fn default() -> Self {
        let now = fixed_time((2026, 5, 1, 12, 0, 0));
        let reviewable_id = REVIEWABLE_MEMORY_ID.to_owned();
        let audit_id = memory_id(AUDIT_MEMORY_ID);

        Self {
            status: StatusDashboardResponse::fixture(now),
            entity_graph: EntityGraphResponse::fixture(),
            entity_detail: EntityDetailResponse::fixture(),
            roi: RoiFixture {
                window_30: RoiResponse::fixture(30),
                window_90: RoiResponse::fixture(90),
                window_365: RoiResponse::fixture(365),
            },
            reality_check_items: vec![reality_check_item(&reviewable_id, now)],
            reality_check_history: RealityCheckHistoryResponse::fixture(now),
            audit_artifact: trust_artifact_fixture(audit_id, now),
            reviewable_memory_ids: BTreeSet::from([reviewable_id]),
            notifications: vec![NotificationSnapshot {
                kind: "review_queue_over".to_owned(),
                message: "Review queue is over threshold: 7/5".to_owned(),
                created_at: now,
            }],
            recall_hits: vec![recall_hits::fixture_recall_hit(
                REVIEWABLE_MEMORY_ID,
                now,
                "Review Stream G dashboard contract",
            )],
        }
    }
}

impl DashboardData {
    pub fn roi_for_window(&self, window_days: u16) -> RoiResponse {
        match window_days {
            30 => self.roi.window_30.clone(),
            365 => self.roi.window_365.clone(),
            _ => self.roi.window_90.clone(),
        }
    }

    pub fn audit_for(&self, id: &str) -> TrustArtifact {
        let mut artifact = self.audit_artifact.clone();
        artifact.id = MemoryId::try_new(id).unwrap_or_else(|_| memory_id(AUDIT_MEMORY_ID));
        artifact
    }
}

pub fn deferred_response(route: &'static str) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        axum::Json(json!({
            "status": "not_implemented",
            "route": route,
            "note": "deferred Stream G future section; policy editor and sync dashboard are not part of Task 14"
        })),
    )
}

fn trust_artifact_fixture(id: MemoryId, now: DateTime<Utc>) -> TrustArtifact {
    TrustArtifact {
        id,
        namespace: "project:agent-memory".to_owned(),
        status: "active".to_owned(),
        sensitivity: "internal".to_owned(),
        source: "agent/patterns/task-14-audit.md".to_owned(),
        source_evidence: None,
        title: SafeContent::Plaintext("Task 14 audit fixture".to_owned()),
        body: SafeContent::Plaintext("Task 14 audit-only fixture body".to_owned()),
        current_confidence: "0.95".to_owned(),
        original_confidence: "0.90".to_owned(),
        confidence_reason: Some("deterministic web fallback fixture".to_owned()),
        trust_summary: "trusted / project-standard@v2".to_owned(),
        recall: RecallStats { total: 28, last_30_days: 12, last_recalled_at: Some(now) },
        provenance_chain: vec![
            ProvenanceEvent {
                timestamp: fixed_time((2026, 4, 30, 14, 22, 0)),
                kind: "written_by_agent".to_owned(),
                summary: "Observed during Stream G implementation".to_owned(),
                evidence: "test fixture".to_owned(),
                device: "dev_web_fixture".to_owned(),
            },
            ProvenanceEvent {
                timestamp: fixed_time((2026, 4, 30, 14, 22, 1)),
                kind: "governance_promoted".to_owned(),
                summary: "Promoted by project-standard@v2".to_owned(),
                evidence: "policy fixture".to_owned(),
                device: "dev_web_fixture".to_owned(),
            },
        ],
        policy_decisions: vec![PolicyDecision {
            policy_applied: "project-standard@v2".to_owned(),
            policy_source: "fixture".to_owned(),
            confidence_floor_pass: "true".to_owned(),
            grounding_satisfied: "true".to_owned(),
            contradiction_result: "none".to_owned(),
            tombstone_enforced: "false".to_owned(),
            sensitivity_gate_result: "allowed".to_owned(),
        }],
        privacy_scan: PrivacyScan { labels_detected: Vec::new(), storage_action: "plaintext".to_owned() },
        supersedes: vec![SupersessionLink {
            id: memory_id("mem_20260430_a1b2c3d4e5f60718_000004"),
            timestamp: Some(fixed_time((2026, 4, 30, 14, 22, 0))),
            title: SafeContent::Plaintext("Previous deployment target".to_owned()),
        }],
        superseded_by: Vec::new(),
        sync_state: SyncState {
            devices: vec!["macbook".to_owned(), "desktop".to_owned()],
            merge_status: "clean".to_owned(),
            claim_lock_status: Some("Stream I not active".to_owned()),
        },
    }
}

fn reality_check_item(memory_id_value: &str, now: DateTime<Utc>) -> RealityCheckItem {
    RealityCheckItem {
        memory_id: memory_id(memory_id_value),
        title: "Review Stream G dashboard contract".to_owned(),
        namespace: "project:agent-memory".to_owned(),
        status: MemoryStatus::Active,
        sensitivity: None,
        score: 0.82,
        component_scores: ComponentScores {
            days_since_observed_norm: 0.7,
            recall_frequency_norm: 0.2,
            cross_source_corroboration: 0.5,
            confidence_decay: 0.6,
            sensitivity_weight: 0.1,
        },
        encrypted: false,
        last_observed_at: fixed_time((2026, 4, 24, 8, 30, 0)),
        recall_count_30d: 4,
        last_recalled_at: Some(now),
    }
}

pub fn memory_id(value: &str) -> MemoryId {
    MemoryId::try_new(value).expect("memoryd-web fixture memory id must be valid")
}

pub fn fixed_time(parts: (i32, u32, u32, u32, u32, u32)) -> DateTime<Utc> {
    let (year, month, day, hour, minute, second) = parts;
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("memoryd-web fixture timestamp must be valid")
}
