use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::{estimated_tokens, DeltaRequest, RecallSectionName, StartupRequest};

#[tokio::test]
async fn memory_startup_returns_recall_block_and_increments_success_counter() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup", RequestPayload::Startup(startup_request(repo.to_string_lossy().as_ref()))),
    )
    .await;

    match response.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => {
            assert_eq!(startup.session_binding.session_id, "sess_startup");
            assert!(startup.recall_block.starts_with("<memory-recall version=\"stream-e-v0.5\""));
            assert!(startup.recall_block.contains("<identity>"));
            assert!(startup.recall_block.contains("<pending-attention>"));
            assert_eq!(startup.recall_explanation.policy, "stream-e-v0.5");
        }
        other => panic!("expected startup success, got {other:?}"),
    }

    let status =
        handle_request_with_state(&substrate, &state, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.startup_invoked_total, 1);
            assert!(status.recall.startup_failed_total.is_empty());
        }
        other => panic!("expected status success, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_startup_response_shape_sections_and_budget_match_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new(
            "req-startup-shape",
            RequestPayload::Startup(startup_request(repo.to_string_lossy().as_ref())),
        ),
    )
    .await;
    let line = response.to_json_line().expect("startup response serializes");
    assert!(line.contains("\"startup\""), "response payload is the startup variant");

    let ResponseResult::Success(ResponsePayload::Startup(startup)) = response.result else {
        panic!("expected startup response");
    };

    assert!(startup.recall_block.starts_with("<memory-recall version=\"stream-e-v0.5\" harness=\"codex\""));
    assert_ordered(
        &startup.recall_block,
        &[
            "<identity>",
            "<project-state>",
            "<entity-recall",
            "<recent-memory>",
            "<pending-attention>",
            "<recall-explanation",
        ],
    );
    assert_eq!(startup.budget_used_tokens, estimated_tokens(&startup.recall_block));
    assert_eq!(startup.recall_explanation.budget_used_tokens, startup.budget_used_tokens);
    assert_eq!(
        startup.recall_explanation.sections.iter().map(|section| section.name).collect::<Vec<_>>(),
        RecallSectionName::STARTUP_ORDER
    );
    assert!(startup.recall_explanation.sections.iter().all(|section| section.matched_entities.is_empty()));
}

#[tokio::test]
async fn memory_startup_validation_failure_increments_failure_counter_by_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = Substrate::init(
        Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();
    let mut request = startup_request("relative/path");
    request.since_event_id = Some("evt_future".to_owned());

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup-invalid", RequestPayload::Startup(request)),
    )
    .await;

    match response.result {
        ResponseResult::Error(error) => {
            assert_eq!(error.code, "invalid_request", "cwd validation must run before since_event_id");
            assert!(!error.retryable);
        }
        other => panic!("expected invalid_request, got {other:?}"),
    }

    let status =
        handle_request_with_state(&substrate, &state, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.startup_invoked_total, 0);
            assert_eq!(status.recall.startup_failed_total.get("invalid_request"), Some(&1));
        }
        other => panic!("expected status success, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_startup_since_event_id_is_only_not_implemented_startup_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let substrate = Substrate::init(
        Roots::new(&repo, temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();
    let mut request = startup_request(repo.to_string_lossy().as_ref());
    request.since_event_id = Some("evt_future".to_owned());

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup-delta", RequestPayload::Startup(request)),
    )
    .await;

    match response.result {
        ResponseResult::Error(error) => {
            assert_eq!(error.code, "not_implemented");
            assert!(!error.retryable);
        }
        other => panic!("expected not_implemented, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_delta_validation_failure_increments_failure_counter_by_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let substrate = Substrate::init(
        Roots::new(&repo, temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new(
            "req-delta-invalid",
            RequestPayload::Delta(DeltaRequest {
                cwd: repo.to_string_lossy().into_owned(),
                session_id: "sess_delta".to_owned(),
                harness: "codex".to_owned(),
                message: " ".to_owned(),
                budget_tokens: Some(512),
            }),
        ),
    )
    .await;

    match response.result {
        ResponseResult::Error(error) => assert_eq!(error.code, "invalid_request"),
        other => panic!("expected invalid_request, got {other:?}"),
    }

    let status =
        handle_request_with_state(&substrate, &state, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.delta_invoked_total, 0);
            assert_eq!(status.recall.delta_failed_total.get("invalid_request"), Some(&1));
        }
        other => panic!("expected status success, got {other:?}"),
    }
}

fn startup_request(cwd: &str) -> StartupRequest {
    StartupRequest {
        cwd: cwd.to_owned(),
        session_id: "sess_startup".to_owned(),
        harness: "codex".to_owned(),
        harness_version: Some("0.0.0".to_owned()),
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(512),
    }
}

fn assert_ordered(haystack: &str, needles: &[&str]) {
    let mut previous = 0usize;
    for needle in needles {
        let index = haystack.find(needle).unwrap_or_else(|| panic!("missing section marker {needle}"));
        assert!(index >= previous, "{needle} appeared out of order");
        previous = index;
    }
}
