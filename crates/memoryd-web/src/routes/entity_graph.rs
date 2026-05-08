use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::{EntitySummary, RequestPayload, ResponsePayload, ResponseResult};
use serde::Serialize;
use std::collections::BTreeSet;

use crate::routes::status::daemon_error;
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
    pub kind: String,
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
    pub mentions: Vec<String>,
    pub related_memories: Vec<EntityMemorySummary>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
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
                    kind: "entity".to_owned(),
                    namespace: "project:agent-memory".to_owned(),
                    memory_count: 42,
                },
                EntityNode {
                    id: "ent_stream_g".to_owned(),
                    label: "Stream G".to_owned(),
                    kind: "entity".to_owned(),
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
            mentions: vec!["mem_20260501_a1b2c3d4e5f60718_000010".to_owned()],
            related_memories: vec![EntityMemorySummary {
                id: "mem_20260501_a1b2c3d4e5f60718_000010".to_owned(),
                namespace: "project:agent-memory".to_owned(),
                status: "active".to_owned(),
                confidence: 0.95,
            }],
            first_seen: Some("2026-05-01T11:02:00Z".to_owned()),
            last_seen: Some("2026-05-01T11:02:00Z".to_owned()),
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
        if let Some(socket_path) = state.daemon_socket() {
            return match inspect_entities(socket_path, None).await {
                Ok(entities) => Json(graph_from_entities(&entities)).into_response(),
                Err(error) => error.into_response(),
            };
        }
        return backend_unavailable("entity_graph").into_response();
    };
    Json(data.entity_graph.clone()).into_response()
}

pub async fn entity_detail(State(state): State<WebState>, Path(entity_id): Path<String>) -> impl IntoResponse {
    let Some(data) = state.dashboard_data() else {
        if let Some(socket_path) = state.daemon_socket() {
            return match inspect_entities(socket_path, Some(entity_id.clone())).await {
                Ok(entities) => Json(detail_from_entities(&entity_id, &entities)).into_response(),
                Err(error) => error.into_response(),
            };
        }
        return backend_unavailable("entity_detail").into_response();
    };
    let mut detail = data.entity_detail.clone();
    detail.entity_id = entity_id;
    Json(detail).into_response()
}

async fn inspect_entities(
    socket_path: &std::path::Path,
    prefix: Option<String>,
) -> Result<Vec<EntitySummary>, axum::response::Response> {
    match memoryd::client::request(
        socket_path,
        "web-entity-graph",
        RequestPayload::InspectEntities { limit: None, prefix },
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::InspectEntities(response)) => Ok(response.entities),
            ResponseResult::Error(error) => {
                Err(daemon_error("entity_graph", error.code, error.message).into_response())
            }
            other => Err(daemon_error("entity_graph", "unexpected_response", format!("{other:?}")).into_response()),
        },
        Err(error) => Err(daemon_error("entity_graph", "daemon_unavailable", error.to_string()).into_response()),
    }
}

fn graph_from_entities(entities: &[EntitySummary]) -> EntityGraphResponse {
    EntityGraphResponse { nodes: entities.iter().map(node_from_entity).collect(), edges: co_mention_edges(entities) }
}

fn node_from_entity(entity: &EntitySummary) -> EntityNode {
    EntityNode {
        id: entity.entity_id.clone(),
        label: entity.label.clone(),
        kind: "entity".to_owned(),
        namespace: "daemon".to_owned(),
        memory_count: entity.memory_count as u32,
    }
}

fn co_mention_edges(entities: &[EntitySummary]) -> Vec<EntityEdge> {
    let mut edges = Vec::new();
    for (index, left) in entities.iter().enumerate() {
        let left_ids = left.recent_memory_ids.iter().map(|id| id.as_str()).collect::<BTreeSet<_>>();
        for right in entities.iter().skip(index + 1) {
            let shared = right.recent_memory_ids.iter().filter(|id| left_ids.contains(id.as_str())).count();
            if shared > 0 {
                edges.push(EntityEdge {
                    source: left.entity_id.clone(),
                    target: right.entity_id.clone(),
                    kind: "co_mentioned".to_owned(),
                    weight: shared as f64,
                    temporal_from: None,
                    temporal_to: None,
                });
            }
        }
    }
    edges
}

fn detail_from_entities(entity_id: &str, entities: &[EntitySummary]) -> EntityDetailResponse {
    let entity = entities.iter().find(|entity| entity.entity_id == entity_id).or_else(|| entities.first());
    let (label, mentions) = match entity {
        Some(entity) => {
            (entity.label.clone(), entity.recent_memory_ids.iter().map(|id| id.as_str().to_owned()).collect::<Vec<_>>())
        }
        None => (entity_id.to_owned(), Vec::new()),
    };
    let related_memories = mentions
        .iter()
        .map(|id| EntityMemorySummary {
            id: id.clone(),
            namespace: "daemon".to_owned(),
            status: "unknown".to_owned(),
            confidence: 0.0,
        })
        .collect::<Vec<_>>();
    EntityDetailResponse {
        entity_id: entity_id.to_owned(),
        label,
        mentions,
        related_memories: related_memories.clone(),
        first_seen: None,
        last_seen: None,
        memories: related_memories,
        supersession_chain: Vec::new(),
        recall_history: Vec::new(),
    }
}
