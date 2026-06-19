use std::fs;

use memoryd::recall::{validate_startup_request, ConcurrentSessionMode, RecallError, StartupRequest};

#[tokio::test]
async fn test_concurrent_session_mode_collaborative_parses() {
    let project = parse_project("canonical_id: proj_agent_memory\nconcurrent_session_mode: collaborative\n")
        .await
        .expect("project binding");

    assert_eq!(project.concurrent_session_mode, Some(ConcurrentSessionMode::Collaborative));
}

#[tokio::test]
async fn test_concurrent_session_mode_minimal_parses() {
    let project = parse_project("canonical_id: proj_agent_memory\nconcurrent_session_mode: minimal\n")
        .await
        .expect("project binding");

    assert_eq!(project.concurrent_session_mode, Some(ConcurrentSessionMode::Minimal));
}

#[tokio::test]
async fn test_concurrent_session_mode_default_parses() {
    let project = parse_project("canonical_id: proj_agent_memory\nconcurrent_session_mode: default\n")
        .await
        .expect("project binding");

    assert_eq!(project.concurrent_session_mode, Some(ConcurrentSessionMode::Default));
}

#[tokio::test]
async fn test_concurrent_session_mode_absent_defaults_none() {
    let project =
        parse_project("canonical_id: proj_agent_memory\nalias: agent-memory\n").await.expect("project binding");

    assert_eq!(project.concurrent_session_mode, None);
}

#[tokio::test]
async fn test_concurrent_session_mode_unknown_value_rejects() {
    let error = parse_project("canonical_id: proj_agent_memory\nconcurrent_session_mode: gibberish\n")
        .await
        .expect_err("unknown concurrent_session_mode should fail");

    assert_invalid_request(&error);
}

#[tokio::test]
async fn test_preparse_whitelist_blocks_without_serde() {
    let error = parse_project("canonical_id: proj_agent_memory\nunknown_key: nope\n")
        .await
        .expect_err("unknown top-level project key should fail");

    assert_invalid_request(&error);
    assert!(
        error.to_string().contains("unknown .memory-project.yaml field: unknown_key"),
        "expected preparse whitelist rejection, got {error:?}"
    );
}

async fn parse_project(yaml: &str) -> Result<memoryd::recall::ProjectBinding, RecallError> {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join(".memory-project.yaml"), yaml).expect("project yaml");

    let binding = validate_startup_request(request(temp.path().to_string_lossy())).await?;
    binding.project.ok_or_else(|| RecallError::invalid_request("missing project binding"))
}

fn request(cwd: impl AsRef<str>) -> StartupRequest {
    StartupRequest {
        cwd: cwd.as_ref().to_owned(),
        session_id: "sess".to_owned(),
        harness: "codex".to_owned(),
        harness_version: None,
        include_recent: true,
        since_event_id: None,
        budget_tokens: None,
        passive: false,
    }
}

fn assert_invalid_request(error: &RecallError) {
    assert!(matches!(error, RecallError::InvalidRequest { .. }), "expected invalid_request, got {error:?}");
}
