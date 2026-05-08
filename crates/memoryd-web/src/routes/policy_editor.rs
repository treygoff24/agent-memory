use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use memory_governance::{CandidateContext, Policy, PolicySet, Scope};
use memoryd::protocol::{
    GovernancePolicySnapshot, GovernancePolicySummary, RequestPayload, ResponsePayload, ResponseResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::routes::status::daemon_error;
use crate::server::{backend_unavailable, WebState};

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
        return backend_unavailable("policy_editor").into_response();
    };

    match validate_and_write_policy(policy_dir, &file_name, &payload.raw_yaml) {
        Ok(policies) => Json(PolicyEditorPostResponse { accepted: true, file_name, policies }).into_response(),
        Err(error) => invalid_policy_response(error).into_response(),
    }
}

fn response_from_snapshot(snapshot: GovernancePolicySnapshot) -> PolicyEditorResponse {
    PolicyEditorResponse {
        source: snapshot.source,
        raw_yaml: snapshot.raw_yaml.unwrap_or_default(),
        policies: snapshot.policies,
        files: Vec::new(),
        writable: false,
    }
}

fn policy_editor_from_dir(policy_dir: &Path) -> Result<PolicyEditorResponse, String> {
    let policies = PolicySet::load_from_dir(policy_dir).map_err(|error| error.to_string())?;
    let files = policy_files(policy_dir)?;
    Ok(PolicyEditorResponse {
        source: "disk".to_owned(),
        raw_yaml: concatenate_policy_yaml(policy_dir, &files)?,
        policies: summarize_policy_set(&policies, "disk"),
        files,
        writable: true,
    })
}

fn validate_and_write_policy(
    policy_dir: &Path,
    file_name: &str,
    raw_yaml: &str,
) -> Result<Vec<GovernancePolicySummary>, String> {
    parse_single_policy(raw_yaml)?;
    let validation_dir = validation_dir(policy_dir)?;
    copy_policy_dir_for_validation(policy_dir, &validation_dir, file_name)?;
    fs::write(validation_dir.join(file_name), raw_yaml).map_err(|error| error.to_string())?;
    let policies = match PolicySet::load_from_dir(&validation_dir) {
        Ok(policies) => policies,
        Err(error) => {
            let _ = fs::remove_dir_all(&validation_dir);
            return Err(error.to_string());
        }
    };
    fs::remove_dir_all(&validation_dir).map_err(|error| error.to_string())?;

    atomic_write(policy_dir.join(file_name), raw_yaml)?;
    Ok(summarize_policy_set(&policies, "disk"))
}

fn parse_single_policy(raw_yaml: &str) -> Result<(), String> {
    serde_yaml::from_str::<Policy>(raw_yaml).map(|_| ()).map_err(|error| error.to_string())
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

fn is_safe_yaml_file_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.ends_with(".yaml")
        && !file_name.contains('/')
        && !file_name.contains('\\')
        && !file_name.starts_with('.')
        && file_name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
}

fn policy_files(policy_dir: &Path) -> Result<Vec<String>, String> {
    let mut files = fs::read_dir(policy_dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "yaml") {
                path.file_name().and_then(|file_name| file_name.to_str()).map(str::to_owned)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn concatenate_policy_yaml(policy_dir: &Path, files: &[String]) -> Result<String, String> {
    let mut output = String::new();
    for file in files {
        output.push_str(&format!("# file: {file}\n"));
        output.push_str(&fs::read_to_string(policy_dir.join(file)).map_err(|error| error.to_string())?);
        if !output.ends_with('\n') {
            output.push('\n');
        }
    }
    Ok(output)
}

fn copy_policy_dir_for_validation(policy_dir: &Path, validation_dir: &Path, target_file: &str) -> Result<(), String> {
    fs::create_dir_all(validation_dir).map_err(|error| error.to_string())?;
    for file in policy_files(policy_dir)? {
        if file != target_file {
            fs::copy(policy_dir.join(&file), validation_dir.join(&file)).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn validation_dir(policy_dir: &Path) -> Result<PathBuf, String> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    Ok(policy_dir.join(format!(".policy-editor-validate-{}-{nonce}", std::process::id())))
}

fn atomic_write(path: PathBuf, raw_yaml: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "policy file has no parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp_path = path.with_extension("yaml.tmp");
    fs::write(&temp_path, raw_yaml).map_err(|error| error.to_string())?;
    fs::File::open(&temp_path).and_then(|file| file.sync_all()).map_err(|error| error.to_string())?;
    fs::rename(&temp_path, &path).map_err(|error| error.to_string())?;
    fs::File::open(parent).and_then(|file| file.sync_all()).map_err(|error| error.to_string())?;
    Ok(())
}

fn summarize_policy_set(policies: &PolicySet, source: &str) -> Vec<GovernancePolicySummary> {
    [Scope::Me, Scope::Project, Scope::Agent, Scope::Dreaming]
        .into_iter()
        .filter_map(|scope| {
            let policy = policies.policy_for_scope(scope).ok()?;
            let preview = policy.dry_run(&CandidateContext::new(scope).with_confidence(0.0).with_grounding(false));
            Some(GovernancePolicySummary {
                scope: format!("{scope:?}").to_ascii_lowercase(),
                selected_policy: preview.selected_policy,
                policy_source: source.to_owned(),
                confidence_floor: preview.confidence_floor,
                review_gates: preview.triggered_review_gates,
                requires_grounding: preview.requires_grounding,
            })
        })
        .collect()
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
