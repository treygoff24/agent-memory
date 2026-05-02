use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::server::{backend_unavailable, WebState};

#[derive(Clone, Debug, Serialize)]
pub struct EntityGraphResponse {
    pub nodes: Vec<EntityNode>,
    pub edges: Vec<EntityEdge>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityNode {
    pub id: String,
    pub label: String,
    pub namespace: String,
    pub memory_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub weight: f64,
    pub temporal_from: Option<String>,
    pub temporal_to: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityDetailResponse {
    pub entity_id: String,
    pub label: String,
    pub memories: Vec<EntityMemorySummary>,
    pub supersession_chain: Vec<String>,
    pub recall_history: Vec<RecallHistoryPoint>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityMemorySummary {
    pub id: String,
    pub namespace: String,
    pub status: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecallHistoryPoint {
    pub at: String,
    pub count: u32,
}

impl EntityGraphResponse {
    pub fn fixture() -> Self {
        Self {
            nodes: vec![
                EntityNode {
                    id: "ent_agent_memory".to_owned(),
                    label: "agent-memory".to_owned(),
                    namespace: "project:agent-memory".to_owned(),
                    memory_count: 42,
                },
                EntityNode {
                    id: "ent_stream_g".to_owned(),
                    label: "Stream G".to_owned(),
                    namespace: "project:agent-memory".to_owned(),
                    memory_count: 8,
                },
            ],
            edges: vec![
                EntityEdge {
                    source: "ent_agent_memory".to_owned(),
                    target: "ent_stream_g".to_owned(),
                    kind: "co_mentioned".to_owned(),
                    weight: 0.72,
                    temporal_from: None,
                    temporal_to: None,
                },
                EntityEdge {
                    source: "mem_20260430_a1b2c3d4e5f60718_000004".to_owned(),
                    target: "mem_20260501_a1b2c3d4e5f60718_000010".to_owned(),
                    kind: "supersedes".to_owned(),
                    weight: 1.0,
                    temporal_from: Some("2026-04-30".to_owned()),
                    temporal_to: None,
                },
            ],
        }
    }
}

impl EntityDetailResponse {
    pub fn fixture() -> Self {
        Self {
            entity_id: "ent_agent_memory".to_owned(),
            label: "agent-memory".to_owned(),
            memories: vec![EntityMemorySummary {
                id: "mem_20260501_a1b2c3d4e5f60718_000010".to_owned(),
                namespace: "project:agent-memory".to_owned(),
                status: "active".to_owned(),
                confidence: 0.95,
            }],
            supersession_chain: vec![
                "mem_20260430_a1b2c3d4e5f60718_000004".to_owned(),
                "mem_20260501_a1b2c3d4e5f60718_000010".to_owned(),
            ],
            recall_history: vec![RecallHistoryPoint { at: "2026-05-01T11:02:00Z".to_owned(), count: 12 }],
        }
    }
}

pub async fn entity_graph(State(state): State<WebState>) -> impl IntoResponse {
    let Some(data) = state.dashboard_data() else {
        if state.daemon_socket().is_some() {
            return crate::routes::deferred_response("entity_graph").into_response();
        }
        return backend_unavailable("entity_graph").into_response();
    };
    Json(data.entity_graph.clone()).into_response()
}

pub async fn entity_detail(State(state): State<WebState>, Path(entity_id): Path<String>) -> impl IntoResponse {
    let Some(data) = state.dashboard_data() else {
        if state.daemon_socket().is_some() {
            return crate::routes::deferred_response("entity_detail").into_response();
        }
        return backend_unavailable("entity_detail").into_response();
    };
    let mut detail = data.entity_detail.clone();
    detail.entity_id = entity_id;
    Json(detail).into_response()
}
