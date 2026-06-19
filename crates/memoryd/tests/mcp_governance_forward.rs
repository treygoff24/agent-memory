use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use memoryd::mcp::{
    forward_to_daemon, ForgetRequest, RevealRequest, StartupRequest, SupersedeRequest, ToolRequest, WriteRequest,
};
use memoryd::protocol::{
    GovernanceForgetResponse, GovernanceStatus, GovernanceSupersedeResponse, GovernanceWriteResponse, RequestEnvelope,
    RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult, RevealResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task::JoinHandle;

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn mcp_governance_forward_memory_write_forwards_to_governed_daemon_payload() {
    let socket = unique_socket_path("mcp-governance-write");
    let server = spawn_single_request_daemon(&socket, |request| {
        assert_eq!(request.id, "req-write");
        let RequestPayload::WriteMemory { body, title, tags, meta } = request.request else {
            panic!("expected WriteMemory payload, got {:?}", request.request);
        };
        assert_eq!(body, "structured project fact");
        assert_eq!(title.as_deref(), Some("Project fact"));
        assert_eq!(tags, ["governed"]);
        assert_eq!(meta["namespace"], "project");
        assert_eq!(meta["cwd"], std::env::current_dir().expect("current dir").to_string_lossy().as_ref());
        ResponseEnvelope::success(
            request.id,
            ResponsePayload::GovernanceWrite(GovernanceWriteResponse {
                status: GovernanceStatus::Promoted,
                id: Some("mem_20260429_a1b2c3d4e5f60718_900001".to_string()),
                namespace: Some("project".to_string()),
                reason: None,
                next_actions: Vec::new(),
                policy_applied: Some("project-standard@v2".to_string()),
                policy_source: Some("built_in_fallback".to_string()),
                existing_id: None,
                similarity_degraded: None,
            }),
        )
    })
    .await;

    let response = forward_to_daemon(
        &socket,
        "req-write",
        ToolRequest::MemoryWrite(WriteRequest {
            body: "structured project fact".to_string(),
            title: Some("Project fact".to_string()),
            tags: vec!["governed".to_string()],
            meta: serde_json::json!({ "namespace": "project", "sensitivity": "internal" }),
        }),
    )
    .await
    .expect("write forwards to daemon");

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected GovernanceWrite success, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Promoted);
    assert_eq!(write.id.as_deref(), Some("mem_20260429_a1b2c3d4e5f60718_900001"));
    server.await.expect("server joins").expect("server ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn mcp_governance_forward_memory_supersede_forwards_to_governed_daemon_payload() {
    let socket = unique_socket_path("mcp-governance-supersede");
    let server = spawn_single_request_daemon(&socket, |request| {
        assert_eq!(request.id, "req-supersede");
        let RequestPayload::Supersede { old_id, content, reason, meta } = request.request else {
            panic!("expected Supersede payload, got {:?}", request.request);
        };
        assert_eq!(old_id, "mem_20260429_a1b2c3d4e5f60718_900001");
        assert_eq!(content, "replacement content");
        assert_eq!(reason, "old fact was stale");
        assert_eq!(meta["sensitivity"], "internal");
        assert_eq!(meta["cwd"], std::env::current_dir().expect("current dir").to_string_lossy().as_ref());
        ResponseEnvelope::success(
            request.id,
            ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_20260429_a1b2c3d4e5f60718_900002".to_string()),
                old_id: Some(old_id),
                reason: None,
                chain: Some(serde_json::json!({
                    "supersedes": ["mem_20260429_a1b2c3d4e5f60718_900001"]
                })),
                policy_applied: Some("project-standard@v2".to_string()),
                policy_source: Some("built_in_fallback".to_string()),
                warning: None,
            }),
        )
    })
    .await;

    let response = forward_to_daemon(
        &socket,
        "req-supersede",
        ToolRequest::MemorySupersede(SupersedeRequest {
            old_id: "mem_20260429_a1b2c3d4e5f60718_900001".to_string(),
            new_body: "replacement content".to_string(),
            reason: "old fact was stale".to_string(),
            meta: serde_json::json!({
                "namespace": "project",
                "type": "project",
                "summary": "replacement content",
                "confidence": 0.95,
                "sensitivity": "internal",
                "source_kind": "user",
                "explicit_user_context": true
            }),
        }),
    )
    .await
    .expect("supersede forwards to daemon");

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected GovernanceSupersede success, got {:?}", response.result);
    };
    assert_eq!(supersede.status, GovernanceStatus::Promoted);
    assert_eq!(supersede.new_id.as_deref(), Some("mem_20260429_a1b2c3d4e5f60718_900002"));
    server.await.expect("server joins").expect("server ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn mcp_governance_forward_memory_forget_forwards_to_governed_daemon_payload() {
    let socket = unique_socket_path("mcp-governance-forget");
    let server = spawn_single_request_daemon(&socket, |request| {
        assert_eq!(request.id, "req-forget");
        let RequestPayload::Forget { id, reason } = request.request else {
            panic!("expected Forget payload, got {:?}", request.request);
        };
        assert_eq!(id, "mem_20260429_a1b2c3d4e5f60718_900001");
        assert_eq!(reason, "user requested removal");
        ResponseEnvelope::success(
            request.id,
            ResponsePayload::GovernanceForget(GovernanceForgetResponse {
                status: GovernanceStatus::Tombstoned,
                id: id.clone(),
                tombstone_ref: Some("tombstone:stream-a".to_string()),
                reason: None,
            }),
        )
    })
    .await;

    let response = forward_to_daemon(
        &socket,
        "req-forget",
        ToolRequest::MemoryForget(ForgetRequest {
            id: "mem_20260429_a1b2c3d4e5f60718_900001".to_string(),
            reason: "user requested removal".to_string(),
        }),
    )
    .await
    .expect("forget forwards to daemon");

    let ResponseResult::Success(ResponsePayload::GovernanceForget(forget)) = response.result else {
        panic!("expected GovernanceForget success, got {:?}", response.result);
    };
    assert_eq!(forget.status, GovernanceStatus::Tombstoned);
    assert_eq!(forget.id, "mem_20260429_a1b2c3d4e5f60718_900001");
    server.await.expect("server joins").expect("server ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn mcp_governance_forward_memory_reveal_forwards_to_privacy_reveal_payload() {
    let socket = unique_socket_path("mcp-reveal");
    let server = spawn_single_request_daemon(&socket, |request| {
        assert_eq!(request.id, "req-reveal");
        let RequestPayload::Reveal { id, reason } = request.request else {
            panic!("expected Reveal payload, got {:?}", request.request);
        };
        assert_eq!(id, "mem_20260429_a1b2c3d4e5f60718_900003");
        assert_eq!(reason, "user asked for contact cell");
        ResponseEnvelope::success(
            request.id,
            ResponsePayload::Reveal(RevealResponse {
                id,
                summary: "contact cell".to_string(),
                body: "cell is 202-555-0198".to_string(),
                truncated: false,
                guidance: "revealed".to_string(),
            }),
        )
    })
    .await;

    let response = forward_to_daemon(
        &socket,
        "req-reveal",
        ToolRequest::MemoryReveal(RevealRequest {
            id: "mem_20260429_a1b2c3d4e5f60718_900003".to_string(),
            reason: "user asked for contact cell".to_string(),
        }),
    )
    .await
    .expect("reveal forwards to daemon");

    let ResponseResult::Success(ResponsePayload::Reveal(reveal)) = response.result else {
        panic!("expected Reveal success, got {:?}", response.result);
    };
    assert!(reveal.body.contains("202-555-0198"));
    server.await.expect("server joins").expect("server ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn mcp_governance_forward_memory_startup_uses_daemon_path() {
    let socket = unique_socket_path("startup");
    let server = spawn_single_request_daemon(&socket, |request| {
        assert!(matches!(request.request, RequestPayload::Startup(_)));
        ResponseEnvelope::error(request.id, "not_implemented", "fixture saw startup", false)
    })
    .await;

    let response = forward_to_daemon(
        &socket,
        "req-startup",
        ToolRequest::MemoryStartup(StartupRequest {
            cwd: "/tmp/project".to_owned(),
            session_id: "sess_mcp".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            include_recent: true,
            since_event_id: None,
            budget_tokens: Some(3_600),
            passive: false,
        }),
    )
    .await
    .expect("startup forwards");

    let ResponseResult::Error(error) = response.result else {
        panic!("expected startup error, got {:?}", response.result);
    };
    assert_eq!(error.code, "not_implemented");
    assert!(error.message.contains("fixture"));
    server.await.expect("server joins").expect("server ok");
    let _ = std::fs::remove_file(socket);
}

async fn spawn_single_request_daemon<F>(socket: &Path, assert_and_respond: F) -> JoinHandle<anyhow::Result<()>>
where
    F: FnOnce(RequestEnvelope) -> ResponseEnvelope + Send + 'static,
{
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("bind fake daemon socket");
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let request = RequestEnvelope::from_json_line(&line)?;
        let response = assert_and_respond(request);
        reader.get_mut().write_all(response.to_json_line()?.as_bytes()).await?;
        Ok(())
    });
    task
}

fn unique_socket_path(test_name: &str) -> PathBuf {
    let counter = SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(format!("/tmp/memd-mcpgov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create short socket directory");
    dir.join(format!("{}-{counter}.sock", &test_name.chars().take(8).collect::<String>()))
}
