use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use memoryd::trust_artifact::{
    PolicyDecision, PrivacyScan, ProvenanceEvent, SupersessionLink, SyncState, TrustArtifact,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::routes::status::daemon_error;
use crate::state::{backend_unavailable, WebState};

#[derive(Clone, Debug, Serialize)]
pub struct AuditMemoryResponse {
    pub memory_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub namespace: String,
    pub confidence: f64,
    pub confidence_reason: Option<String>,
    pub recall_count_total: u32,
    pub recall_count_30d: u32,
    pub last_recalled: Option<DateTime<Utc>>,
    pub provenance_chain: Vec<ProvenanceEvent>,
    pub policy_decisions: Vec<PolicyDecision>,
    pub privacy_scan: PrivacyScan,
    pub supersession_history: Vec<SupersessionHistoryEntry>,
    pub sync_state: SyncState,
}

impl AuditMemoryResponse {
    fn from_artifact(memory_id: String, artifact: TrustArtifact) -> Self {
        let title = artifact.title.display_text().to_owned();
        let body = artifact.body.display_text().to_owned();
        let supersession_history = supersession_history(&artifact);

        Self {
            memory_id,
            title,
            body,
            status: artifact.status,
            namespace: artifact.namespace,
            confidence: parse_confidence(&artifact.current_confidence),
            confidence_reason: artifact.confidence_reason,
            recall_count_total: artifact.recall.total,
            recall_count_30d: artifact.recall.last_30_days,
            last_recalled: artifact.recall.last_recalled_at,
            provenance_chain: artifact.provenance_chain,
            policy_decisions: artifact.policy_decisions,
            privacy_scan: artifact.privacy_scan,
            supersession_history,
            sync_state: artifact.sync_state,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SupersessionHistoryEntry {
    pub direction: SupersessionDirection,
    pub memory_id: String,
    pub at: Option<DateTime<Utc>>,
    pub title: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupersessionDirection {
    Supersedes,
    SupersededBy,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProvenanceWalkQuery {
    pub direction: Option<String>,
    pub depth: Option<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WalkDirection {
    Up,
    Down,
}

impl WalkDirection {
    fn parse(raw: Option<String>) -> Result<Self, String> {
        match raw.as_deref().unwrap_or("up") {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            other => Err(format!("direction must be `up` or `down`, got `{other}`")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProvenanceWalkResponse {
    pub memory_id: String,
    pub direction: String,
    pub depth: u8,
    pub nodes: Vec<WalkNode>,
    pub edges: Vec<WalkEdge>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WalkNode {
    pub id: String,
    pub kind: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WalkEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TemporalQuery {
    pub at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TemporalStateResponse {
    pub memory_id: String,
    pub at: Option<String>,
    pub viewing_historical_state: bool,
    pub artifact: TrustArtifact,
}

pub async fn audit(State(state): State<WebState>, Path(id): Path<String>) -> impl IntoResponse {
    if let Some(data) = state.dashboard_data() {
        return Json(AuditMemoryResponse::from_artifact(id.clone(), data.audit_for(&id))).into_response();
    }
    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("audit").into_response();
    };
    match memoryd::client::request(
        socket_path,
        format!("web-audit-{id}"),
        RequestPayload::TrustArtifact { id: id.clone() },
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::TrustArtifact(artifact)) => {
                Json(AuditMemoryResponse::from_artifact(id, *artifact)).into_response()
            }
            ResponseResult::Error(error) => daemon_error("audit", error.code, error.message).into_response(),
            other => daemon_error("audit", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("audit", "daemon_unavailable", error.to_string()).into_response(),
    }
}

pub async fn audit_walk(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Query(query): Query<ProvenanceWalkQuery>,
) -> impl IntoResponse {
    let direction = match WalkDirection::parse(query.direction.clone()) {
        Ok(direction) => direction,
        Err(message) => return invalid_query("audit_walk", message).into_response(),
    };
    if let Some(data) = state.dashboard_data() {
        return Json(provenance_walk_from_artifact(&id, query, direction, data.audit_for(&id))).into_response();
    }
    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("audit_walk").into_response();
    };
    match memoryd::client::request(
        socket_path,
        format!("web-audit-walk-{id}"),
        RequestPayload::TrustArtifact { id: id.clone() },
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::TrustArtifact(artifact)) => {
                Json(provenance_walk_from_artifact(&id, query, direction, *artifact)).into_response()
            }
            ResponseResult::Error(error) => daemon_error("audit_walk", error.code, error.message).into_response(),
            other => daemon_error("audit_walk", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("audit_walk", "daemon_unavailable", error.to_string()).into_response(),
    }
}

pub async fn audit_temporal(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Query(query): Query<TemporalQuery>,
) -> impl IntoResponse {
    if let Some(data) = state.dashboard_data() {
        return Json(TemporalStateResponse {
            memory_id: id.clone(),
            at: query.at,
            viewing_historical_state: true,
            artifact: data.audit_for(&id),
        })
        .into_response();
    }
    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("audit_temporal").into_response();
    };
    match memoryd::client::request(
        socket_path,
        format!("web-audit-temporal-{id}"),
        RequestPayload::TrustArtifact { id: id.clone() },
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::TrustArtifact(artifact)) => Json(TemporalStateResponse {
                memory_id: id,
                at: query.at,
                viewing_historical_state: true,
                artifact: *artifact,
            })
            .into_response(),
            ResponseResult::Error(error) => daemon_error("audit_temporal", error.code, error.message).into_response(),
            other => daemon_error("audit_temporal", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("audit_temporal", "daemon_unavailable", error.to_string()).into_response(),
    }
}

fn supersession_history(artifact: &TrustArtifact) -> Vec<SupersessionHistoryEntry> {
    artifact
        .supersedes
        .iter()
        .map(|link| supersession_entry(SupersessionDirection::Supersedes, link))
        .chain(artifact.superseded_by.iter().map(|link| supersession_entry(SupersessionDirection::SupersededBy, link)))
        .collect()
}

fn provenance_walk_from_artifact(
    id: &str,
    query: ProvenanceWalkQuery,
    direction: WalkDirection,
    artifact: TrustArtifact,
) -> ProvenanceWalkResponse {
    let depth = query.depth.unwrap_or(3).clamp(1, 8);
    let mut nodes = vec![WalkNode {
        id: id.to_owned(),
        kind: "memory".to_owned(),
        label: artifact.title.display_text().to_owned(),
    }];
    let mut edges = Vec::new();

    for (index, event) in artifact.provenance_chain.into_iter().take(depth as usize).enumerate() {
        let event_id = format!("event_{index}_{}", event.kind);
        nodes.push(WalkNode {
            id: event_id.clone(),
            kind: "event".to_owned(),
            label: format!("{} at {}", event.summary, event.timestamp),
        });
        edges.push(WalkEdge { source: event_id, target: id.to_owned(), kind: "provenance".to_owned() });
    }

    for link in artifact.supersedes.into_iter().take(depth as usize) {
        let link_id = link.id.to_string();
        nodes.push(WalkNode {
            id: link_id.clone(),
            kind: "memory".to_owned(),
            label: link.title.display_text().to_owned(),
        });
        edges.push(WalkEdge { source: id.to_owned(), target: link_id, kind: "supersedes".to_owned() });
    }

    for link in artifact.superseded_by.into_iter().take(depth as usize) {
        let link_id = link.id.to_string();
        nodes.push(WalkNode {
            id: link_id.clone(),
            kind: "memory".to_owned(),
            label: link.title.display_text().to_owned(),
        });
        edges.push(WalkEdge { source: link_id, target: id.to_owned(), kind: "superseded_by".to_owned() });
    }

    ProvenanceWalkResponse { memory_id: id.to_owned(), direction: direction.as_str().to_owned(), depth, nodes, edges }
}

fn invalid_query(route: &'static str, message: String) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "invalid_query",
            "route": route,
            "message": message
        })),
    )
}

fn parse_confidence(raw: &str) -> f64 {
    raw.parse::<f64>().unwrap_or(0.0)
}

fn supersession_entry(direction: SupersessionDirection, link: &SupersessionLink) -> SupersessionHistoryEntry {
    SupersessionHistoryEntry {
        direction,
        memory_id: link.id.to_string(),
        at: link.timestamp,
        title: link.title.display_text().to_owned(),
    }
}
