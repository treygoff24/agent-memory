use std::path::Path;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use memory_governance::{Policy, PolicySet};
use memoryd::policy_editor::{is_safe_yaml_file_name, summarize_policy_set};
use memoryd::protocol::{
    GovernancePolicySnapshot, GovernancePolicySummary, RequestPayload, ResponsePayload, ResponseResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::routes::status::daemon_error;
use crate::state::{backend_unavailable, WebState};

const FIXTURE_POLICY_YAML: &str = r#"name: project-standard
version: 2
scope: project
confidence_floor: 0.7
requires_grounding: true
tombstone_enforcement: review
contradiction_policy: supersede
review_gates:
  - low_confidence
"#;

#[derive(Clone, Debug, Serialize)]
pub struct PolicyEditorResponse {
    pub source: String,
    pub raw_yaml: String,
    pub policies: Vec<GovernancePolicySummary>,
    pub files: Vec<String>,
    pub current_file: Option<String>,
    pub writable: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PolicyEditorPostRequest {
    pub raw_yaml: String,
    pub file_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PolicyEditorPostResponse {
    pub accepted: bool,
    pub file_name: String,
    pub policies: Vec<GovernancePolicySummary>,
}

pub async fn policy_editor_get(State(state): State<WebState>) -> impl IntoResponse {
    if let Some(policy_dir) = state.policy_dir() {
        return match policy_editor_from_dir(policy_dir) {
            Ok(response) => Json(response).into_response(),
            Err(error) => invalid_policy_response(error).into_response(),
        };
    }

    if state.dashboard_data().is_some() {
        return Json(PolicyEditorResponse {
            source: "fixture".to_owned(),
            raw_yaml: FIXTURE_POLICY_YAML.to_owned(),
            policies: summarize_policy_set(&PolicySet::builtin(), "built_in_fallback"),
            files: vec!["project-standard.yaml".to_owned()],
            current_file: Some("project-standard.yaml".to_owned()),
            writable: false,
        })
        .into_response();
    }

    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("policy_editor").into_response();
    };

    match memoryd::client::request(socket_path, "web-policy-editor", RequestPayload::GovernancePolicyDump).await {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::GovernancePolicyDump(snapshot)) => {
                Json(response_from_snapshot(snapshot)).into_response()
            }
            ResponseResult::Error(error) => daemon_error("policy_editor", error.code, error.message).into_response(),
            other => daemon_error("policy_editor", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("policy_editor", "daemon_unavailable", error.to_string()).into_response(),
    }
}

pub async fn policy_editor_post(
    State(state): State<WebState>,
    Json(payload): Json<PolicyEditorPostRequest>,
) -> impl IntoResponse {
    let Ok(file_name) = target_file_name(&payload) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_governance_policy",
                "message": "policy YAML must parse and file_name must be a plain .yaml filename"
            })),
        )
            .into_response();
    };

    let Some(policy_dir) = state.policy_dir() else {
        if state.dashboard_data().is_some() {
            return match parse_single_policy(&payload.raw_yaml) {
                Ok(()) => Json(PolicyEditorPostResponse {
                    accepted: true,
                    file_name,
                    policies: summarize_policy_set(&PolicySet::builtin(), "built_in_fallback"),
                })
                .into_response(),
                Err(error) => invalid_policy_response(error).into_response(),
            };
        }
        let Some(socket_path) = state.daemon_socket() else {
            return backend_unavailable("policy_editor").into_response();
        };
        return match memoryd::client::request(
            socket_path,
            "web-policy-editor-write",
            RequestPayload::PolicyWrite { raw_yaml: payload.raw_yaml, file_name: Some(file_name) },
        )
        .await
        {
            Ok(response) => match response.result {
                ResponseResult::Success(ResponsePayload::PolicyWrite(result)) => Json(PolicyEditorPostResponse {
                    accepted: result.accepted,
                    file_name: result.file_name,
                    policies: result.policies,
                })
                .into_response(),
                ResponseResult::Error(error) if error.code == "invalid_request" => {
                    invalid_policy_response(error.message).into_response()
                }
                ResponseResult::Error(error) => {
                    daemon_error("policy_editor", error.code, error.message).into_response()
                }
                other => daemon_error("policy_editor", "unexpected_response", format!("{other:?}")).into_response(),
            },
            Err(error) => daemon_error("policy_editor", "daemon_unavailable", error.to_string()).into_response(),
        };
    };

    match memoryd::policy_editor::write_to_dir(policy_dir, &payload.raw_yaml, Some(&file_name)) {
        Ok(response) => Json(PolicyEditorPostResponse {
            accepted: response.accepted,
            file_name: response.file_name,
            policies: response.policies,
        })
        .into_response(),
        Err(error) => invalid_policy_response(error).into_response(),
    }
}

fn response_from_snapshot(snapshot: GovernancePolicySnapshot) -> PolicyEditorResponse {
    PolicyEditorResponse {
        source: snapshot.source,
        raw_yaml: snapshot.raw_yaml.unwrap_or_default(),
        policies: snapshot.policies,
        files: snapshot.files,
        current_file: snapshot.current_file,
        writable: snapshot.writable,
    }
}

fn policy_editor_from_dir(policy_dir: &Path) -> Result<PolicyEditorResponse, String> {
    let snapshot = memoryd::policy_editor::snapshot_from_dir(policy_dir)?;
    Ok(PolicyEditorResponse {
        source: snapshot.source,
        raw_yaml: snapshot.raw_yaml.unwrap_or_default(),
        policies: snapshot.policies,
        files: snapshot.files,
        current_file: snapshot.current_file,
        writable: snapshot.writable,
    })
}

fn target_file_name(payload: &PolicyEditorPostRequest) -> Result<String, String> {
    let file_name = match &payload.file_name {
        Some(file_name) => file_name.clone(),
        None => {
            let policy: Policy = serde_yaml::from_str(&payload.raw_yaml).map_err(|error| error.to_string())?;
            format!("{}.yaml", policy.name())
        }
    };
    if is_safe_yaml_file_name(&file_name) {
        Ok(file_name)
    } else {
        Err("invalid file_name".to_owned())
    }
}

fn parse_single_policy(raw_yaml: &str) -> Result<(), String> {
    serde_yaml::from_str::<Policy>(raw_yaml).map(|_| ()).map_err(|error| error.to_string())
}

fn invalid_policy_response(error: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "invalid_governance_policy",
            "message": error.into()
        })),
    )
}
