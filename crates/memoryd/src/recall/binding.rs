use std::path::PathBuf;

use crate::recall::error::RecallError;
use crate::recall::project::resolve_project_binding;
use crate::recall::types::{SessionBinding, StartupRequest, DEFAULT_STARTUP_BUDGET_TOKENS};

const MAX_BINDING_FIELD_BYTES: usize = 128;
const MIN_BUDGET_TOKENS: usize = 512;
const MAX_BUDGET_TOKENS: usize = 8_000;

pub async fn validate_startup_request(request: StartupRequest) -> Result<SessionBinding, RecallError> {
    let harness_version = validate_optional_field("harness_version", request.harness_version.as_deref())?;
    validate_budget(request.budget_tokens)?;

    let mut binding = validate_session_fields(&request.cwd, &request.session_id, &request.harness).await?;
    binding.harness_version = harness_version;

    Ok(binding)
}

pub(crate) async fn validate_session_fields(
    cwd: &str,
    session_id: &str,
    harness: &str,
) -> Result<SessionBinding, RecallError> {
    let cwd = validate_cwd(cwd).await?;
    let session_id = validate_required_field("session_id", session_id)?;
    let harness = validate_required_field("harness", harness)?;

    let project = resolve_project_binding(&cwd).await?;
    let namespaces_in_scope = namespaces_for(project.as_ref());

    Ok(SessionBinding {
        session_id,
        harness,
        harness_version: None,
        cwd: cwd.to_string_lossy().into_owned(),
        project,
        namespaces_in_scope,
    })
}

async fn validate_cwd(cwd: &str) -> Result<PathBuf, RecallError> {
    let path = PathBuf::from(cwd.trim());
    if !path.is_absolute() {
        return Err(RecallError::invalid_request("cwd must be absolute"));
    }
    tokio::fs::canonicalize(&path)
        .await
        .map_err(|error| RecallError::invalid_request(format!("cwd must exist and canonicalize cleanly: {error}")))
}

fn validate_required_field(name: &str, value: &str) -> Result<String, RecallError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RecallError::invalid_request(format!("{name} must be non-empty")));
    }
    if trimmed.len() > MAX_BINDING_FIELD_BYTES {
        return Err(RecallError::invalid_request(format!("{name} must be at most 128 bytes")));
    }
    Ok(trimmed.to_owned())
}

fn validate_optional_field(name: &str, value: Option<&str>) -> Result<Option<String>, RecallError> {
    value.map(|field| validate_required_field(name, field)).transpose()
}

fn validate_budget(budget_tokens: Option<usize>) -> Result<(), RecallError> {
    let budget = budget_tokens.unwrap_or(DEFAULT_STARTUP_BUDGET_TOKENS);
    if !(MIN_BUDGET_TOKENS..=MAX_BUDGET_TOKENS).contains(&budget) {
        return Err(RecallError::invalid_request("budget_tokens must be in 512..=8000"));
    }
    Ok(())
}

fn namespaces_for(project: Option<&crate::recall::types::ProjectBinding>) -> Vec<String> {
    let mut namespaces = vec!["me".to_owned()];
    if let Some(project) = project {
        namespaces.push(format!("project:{}", project.canonical_id));
    }
    namespaces.push("agent".to_owned());
    namespaces
}
