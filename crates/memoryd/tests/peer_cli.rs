use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use clap::Parser as _;
use memorum_coordination::claim_lock::ClaimLockAcquireRequest;
use memorum_coordination::PresenceRecord;
use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::cli::Cli;
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::mcp::{forward_payload_to_daemon, manifest, ToolName};
use memoryd::protocol::{
    render_peer_activity_human, render_peer_status_human, PeerActivityFormat, PeerDeliveryAuditEntry,
    PeerReleaseLockExpectedHolder, PeerReleaseLockStatus, RequestEnvelope, RequestPayload, ResponsePayload,
    ResponseResult,
};
use tempfile::TempDir;

#[test]
fn peer_cli_parser_exposes_status_activity_and_release_lock() {
    Cli::try_parse_from(["memoryd", "peer", "status"]).expect("peer status parses");
    Cli::try_parse_from(["memoryd", "peer", "activity", "--limit", "2"]).expect("peer activity parses");
    Cli::try_parse_from(["memoryd", "peer", "activity", "--session", "sess_a", "--since", "2026-05-01"])
        .expect("peer activity filters parse");
    Cli::try_parse_from(["memoryd", "peer", "activity", "--format", "json"]).expect("peer activity json parses");
    Cli::try_parse_from(["memoryd", "peer", "release-lock", "mem_x", "--yes"]).expect("release-lock --yes parses");
}

#[tokio::test]
async fn test_peer_status_shows_coordination_level() {
    let fixture = Fixture::new(2).await;

    let status = fixture.peer_status().await;
    let output = render_peer_status_human(&status);

    assert!(output.contains("Coordination level: 2"), "{output}");
}

#[tokio::test]
async fn test_peer_status_shows_active_sessions() {
    let fixture = Fixture::new(3).await;
    fixture.state.presence().upsert(presence_record("codex", "sess_abcdef123456"));

    let status = fixture.peer_status().await;
    let output = render_peer_status_human(&status);

    assert!(output.contains("codex:sess_a"), "{output}");
    assert!(output.contains("project:agent-memory"), "{output}");
    assert!(output.contains("ent_stream_i"), "{output}");
}

#[tokio::test]
async fn test_peer_status_shows_claim_locks() {
    let fixture = Fixture::new(2).await;
    fixture.acquire_lock("mem_20260501_a1b2c3d4e5f60718_000021", "claude-code", "sess_def567");

    let status = fixture.peer_status().await;
    let output = render_peer_status_human(&status);

    assert!(output.contains("mem_20260501_a1b2c3d4e5f60718_000021"), "{output}");
    assert!(output.contains("claude-code:sess_def567"), "{output}");
    assert!(output.contains("expires in"), "{output}");
}

#[tokio::test]
async fn test_peer_activity_shows_deliveries() {
    let fixture = Fixture::new(2).await;
    fixture.state.record_peer_delivery(delivery("mem_a", "Migrated users.email to CITEXT."));
    fixture.state.record_peer_delivery(delivery("mem_b", "Added ent_stripe_webhook entity."));

    let activity = fixture.peer_activity(Some(2)).await;
    let output = render_peer_activity_human(&activity);

    assert!(output.contains("mem_a"), "{output}");
    assert!(output.contains("Migrated users.email"), "{output}");
    assert!(output.contains("mem_b"), "{output}");
    assert!(output.contains("ent_stripe_webhook"), "{output}");
}

#[tokio::test]
async fn test_peer_release_lock_no_lock_found() {
    let fixture = Fixture::new(2).await;

    let release = fixture.release_lock("mem_not_locked").await;

    assert_eq!(release.status, PeerReleaseLockStatus::NoLockFound);
    assert!(release.released.is_none());
}

#[tokio::test]
async fn test_peer_release_lock_forced_succeeds() {
    let fixture = Fixture::new(2).await;
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000099";
    fixture.acquire_lock(memory_id, "codex", "sess_lock");

    let release = fixture.release_lock(memory_id).await;

    assert_eq!(release.status, PeerReleaseLockStatus::Released);
    assert!(release.released.is_some());
    assert!(fixture.state.claim_locks().get(memory_id).is_none());
}

#[tokio::test]
async fn test_peer_release_lock_expected_holder_rejects_changed_lock() {
    let fixture = Fixture::new(2).await;
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000100";
    fixture.acquire_lock(memory_id, "codex", "sess_original");

    let release = fixture
        .release_lock_expected(
            memory_id,
            PeerReleaseLockExpectedHolder {
                holder_harness: "claude-code".to_string(),
                holder_session_id: "sess_other".to_string(),
            },
        )
        .await;

    assert_eq!(release.status, PeerReleaseLockStatus::LockChanged);
    assert!(release.released.is_none());
    let current = fixture.state.claim_locks().get(memory_id).expect("lock remains held");
    assert_eq!(current.holder_harness, "codex");
    assert_eq!(current.holder_session_id, "sess_original");
}

#[tokio::test]
async fn test_peer_release_lock_expected_holder_releases_matching_lock() {
    let fixture = Fixture::new(2).await;
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000101";
    fixture.acquire_lock(memory_id, "codex", "sess_lock");

    let release = fixture
        .release_lock_expected(
            memory_id,
            PeerReleaseLockExpectedHolder {
                holder_harness: "codex".to_string(),
                holder_session_id: "sess_lock".to_string(),
            },
        )
        .await;

    assert_eq!(release.status, PeerReleaseLockStatus::Released);
    assert!(release.released.is_some());
    assert!(fixture.state.claim_locks().get(memory_id).is_none());
}

#[tokio::test]
async fn test_peer_commands_not_in_mcp() {
    for name in ["peer_status", "peer_activity", "peer_release_lock", "memory_peer_status"] {
        assert!(ToolName::try_from(name).is_err(), "{name} must not parse as an MCP tool");
        assert!(manifest().tools.iter().all(|tool| tool.name != name), "{name} leaked into manifest");
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let response =
        forward_payload_to_daemon(temp.path().join("missing.sock").as_path(), "peer-mcp", RequestPayload::PeerStatus)
            .await
            .expect("MCP rejection is local");
    let ResponseResult::Error(error) = response.result else {
        panic!("expected MCP rejection, got {:?}", response.result);
    };
    assert_eq!(error.code, "method_not_allowed_on_mcp");
}

struct Fixture {
    _temp: TempDir,
    substrate: Substrate,
    state: HandlerState,
}

impl Fixture {
    async fn new(level: u8) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots,
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_peercli01".to_string()) },
        )
        .await
        .expect("init substrate");
        Self { _temp: temp, substrate, state: HandlerState::with_coordination_level(level) }
    }

    async fn peer_status(&self) -> memoryd::protocol::PeerStatusResponse {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new("peer-status", RequestPayload::PeerStatus),
        )
        .await;
        let ResponseResult::Success(ResponsePayload::PeerStatus(status)) = response.result else {
            panic!("expected peer status, got {:?}", response.result);
        };
        status
    }

    async fn peer_activity(&self, limit: Option<usize>) -> memoryd::protocol::PeerActivityResponse {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new(
                "peer-activity",
                RequestPayload::PeerActivity { session: None, since: None, limit, format: PeerActivityFormat::Human },
            ),
        )
        .await;
        let ResponseResult::Success(ResponsePayload::PeerActivity(activity)) = response.result else {
            panic!("expected peer activity, got {:?}", response.result);
        };
        activity
    }

    async fn release_lock(&self, memory_id: &str) -> memoryd::protocol::PeerReleaseLockResponse {
        self.release_lock_payload(RequestPayload::PeerReleaseLock {
            memory_id: memory_id.to_string(),
            expected_holder: None,
        })
        .await
    }

    async fn release_lock_expected(
        &self,
        memory_id: &str,
        expected_holder: PeerReleaseLockExpectedHolder,
    ) -> memoryd::protocol::PeerReleaseLockResponse {
        self.release_lock_payload(RequestPayload::PeerReleaseLock {
            memory_id: memory_id.to_string(),
            expected_holder: Some(expected_holder),
        })
        .await
    }

    async fn release_lock_payload(&self, payload: RequestPayload) -> memoryd::protocol::PeerReleaseLockResponse {
        let response =
            handle_request_with_state(&self.substrate, &self.state, RequestEnvelope::new("peer-release-lock", payload))
                .await;
        let ResponseResult::Success(ResponsePayload::PeerReleaseLock(release)) = response.result else {
            panic!("expected peer release-lock, got {:?}", response.result);
        };
        release
    }

    fn acquire_lock(&self, memory_id: &str, harness: &str, session_id: &str) {
        self.state.claim_locks().acquire(ClaimLockAcquireRequest::new(
            memory_id,
            session_id,
            harness,
            Duration::from_secs(300),
        ));
    }
}

fn presence_record(harness: &str, session_id: &str) -> PresenceRecord {
    PresenceRecord {
        session_id: session_id.to_string(),
        device_id: Some("dev_peercli01".to_string()),
        harness: harness.to_string(),
        project_binding: None,
        namespace: "project:agent-memory".to_string(),
        salient_entities: vec![
            "ent_stream_i".to_string(),
            "ent_cli".to_string(),
            "ent_claim_lock".to_string(),
            "ent_presence".to_string(),
            "ent_extra".to_string(),
            "ent_hidden".to_string(),
        ],
        salient_paths: Vec::new(),
        capabilities: Vec::new(),
        started_at: Some(Utc.with_ymd_and_hms(2026, 5, 1, 14, 0, 0).unwrap()),
        last_heartbeat_at: Instant::now(),
        claim_locks_held: Vec::new(),
    }
}

fn delivery(memory_id: &str, summary: &str) -> PeerDeliveryAuditEntry {
    PeerDeliveryAuditEntry {
        delivered_at: Utc.with_ymd_and_hms(2026, 5, 1, 15, 23, 0).unwrap(),
        from_harness: "codex".to_string(),
        from_session_id: "abc1234".to_string(),
        to_harness: "claude-code".to_string(),
        to_session_id: "def567".to_string(),
        memory_id: memory_id.to_string(),
        relevance: 0.84,
        summary: summary.to_string(),
    }
}
