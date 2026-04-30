use std::fs;
use std::path::PathBuf;

use crate::recall::error::RecallError;
use crate::recall::project::resolve_project_binding;
use crate::recall::types::{SessionBinding, StartupRequest};

const MAX_BINDING_FIELD_BYTES: usize = 128;
const DEFAULT_BUDGET_TOKENS: usize = 3_600;
const MIN_BUDGET_TOKENS: usize = 512;
const MAX_BUDGET_TOKENS: usize = 8_000;

pub fn validate_startup_request(request: StartupRequest) -> Result<SessionBinding, RecallError> {
    let cwd = validate_cwd(&request.cwd)?;
    let session_id = validate_required_field("session_id", &request.session_id)?;
    let harness = validate_required_field("harness", &request.harness)?;
    let harness_version = validate_optional_field("harness_version", request.harness_version.as_deref())?;
    validate_budget(request.budget_tokens)?;

    let project = resolve_project_binding(&cwd)?;
    let namespaces_in_scope = namespaces_for(project.as_ref());

    Ok(SessionBinding {
        session_id,
        harness,
        harness_version,
        cwd: cwd.to_string_lossy().into_owned(),
        project,
        namespaces_in_scope,
    })
}

fn validate_cwd(cwd: &str) -> Result<PathBuf, RecallError> {
    let path = PathBuf::from(cwd.trim());
    if !path.is_absolute() {
        return Err(RecallError::invalid_request("cwd must be absolute"));
    }
    fs::canonicalize(&path)
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
    let budget = budget_tokens.unwrap_or(DEFAULT_BUDGET_TOKENS);
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
