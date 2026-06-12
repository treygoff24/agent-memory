use memorum_coordination::claim_lock::ClaimLockAcquireRequest;
use memory_substrate::{
    events::{read_events, EventKind},
    InitOptions, Roots, Substrate,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn level1_supersede_skips_claim_lock_acquire() {
    let fixture = Fixture::new(1).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-minimal",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-minimal", "codex", "sess_minimal"),
        ))
        .await;

    assert_promoted_without_warning(response);
    assert_eq!(fixture.state.claim_locks().get(&old_id), None);
}

#[tokio::test]
async fn project_minimal_mode_skips_claim_lock_despite_level2_config() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-project-minimal",
            &old_id,
            "The deployment target is production.",
            project_meta_with_mode("supersede-project-minimal", "codex", "sess_b", "minimal"),
        ))
        .await;

    assert_promoted_without_warning(response);
    let lock = fixture.state.claim_locks().get(&old_id).expect("existing lock remains untouched");
    assert_eq!(lock.holder_harness, "claude-code");
    assert_eq!(lock.holder_session_id, "sess_a");
    assert!(fixture.jsonl_events().iter().all(|event| !matches!(event.kind, EventKind::ClaimLockContention { .. })));
}

#[tokio::test]
async fn project_default_mode_acquires_claim_lock_despite_level1_config() {
    let fixture = Fixture::new(1).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-project-default",
            &old_id,
            "The deployment target is production.",
            project_meta_with_mode("supersede-project-default", "codex", "sess_b", "default"),
        ))
        .await;

    let supersede = assert_promoted(response);
    let warning = supersede.warning.expect("project default mode should acquire and contend");
    assert_eq!(warning.code, "claim_lock_contention");
    assert_eq!(warning.holder, "claude-code:sess_a");
}

#[tokio::test]
async fn level2_supersede_releases_claim_lock_on_success() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-default",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-default", "codex", "sess_b"),
        ))
        .await;

    assert_promoted_without_warning(response);
    assert_eq!(fixture.state.claim_locks().get(&old_id), None);
}

#[tokio::test]
async fn contention_proceeds_with_warning() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-contended",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-contended", "codex", "sess_b"),
        ))
        .await;

    let supersede = assert_promoted(response);
    let warning = supersede.warning.expect("contention warning");
    assert_eq!(warning.code, "claim_lock_contention");
    assert_eq!(warning.holder, "claude-code:sess_a");
    assert!(warning.message.contains("claude-code:sess_a"));
}

#[tokio::test]
async fn contention_emits_jsonl_and_sqlite_event() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-contended",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-contended", "codex", "sess_b"),
        ))
        .await;
    assert_promoted(response);

    assert!(fixture.jsonl_events().iter().any(|event| matches!(
        &event.kind,
        EventKind::ClaimLockContention { memory_id, holder, contender }
            if memory_id.as_str() == old_id && holder == "claude-code:sess_a" && contender == "codex:sess_b"
    )));
    assert_eq!(fixture.sqlite_contention_count(&old_id, "claude-code:sess_a", "codex:sess_b"), 1);
}

#[tokio::test]
async fn governance_rejected_supersede_does_not_return_claim_lock_warning() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-rejected",
            &old_id,
            "An agent claim without local evidence must fail closed.",
            json!({
                "namespace": "agent",
                "type": "claim",
                "summary": "Ungrounded replacement",
                "confidence": 0.50,
                "sensitivity": "internal",
                "source_kind": "agent_primary",
                "session_id": "sess_b",
                "harness": "codex"
            }),
        ))
        .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected structured governance supersede response");
    };
    assert_eq!(supersede.status, GovernanceStatus::Refused);
    assert!(supersede.warning.is_none(), "governance refusal must not be replaced by claim-lock warning");
}

#[tokio::test]
async fn supersede_rejects_secret_claim_lock_session_identity_before_contention_log() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-secret-session",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-secret-session", "codex", "AKIA1234567890ABCDEF"),
        ))
        .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid_request, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("session_id"));
    assert!(fixture.jsonl_events().iter().all(|event| !matches!(event.kind, EventKind::ClaimLockContention { .. })));
}

#[tokio::test]
async fn supersede_rejects_oversized_claim_lock_harness_identity() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    let oversized_harness = "a".repeat(129);

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-oversized-harness",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-oversized-harness", &oversized_harness, "sess_b"),
        ))
        .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid_request, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("harness"));
}

#[tokio::test]
async fn post_acquire_supersede_failure_restores_previous_claim_lock_holder() {
    let fixture = Fixture::new(2).await;
    let old_id = fixture.write_project_memory("old", "The deployment target is staging.").await;
    fixture.acquire_lock(&old_id, "claude-code", "sess_a");
    let blocker = fixture.create_directory_at_next_memory_path(&old_id);

    let response = fixture
        .supersede(SupersedeInput::new(
            "supersede-write-fails",
            &old_id,
            "The deployment target is production.",
            project_meta("supersede-write-fails", "codex", "sess_b"),
        ))
        .await;
    std::fs::remove_dir(blocker).expect("remove blocker directory");

    let ResponseResult::Error(error) = response.result else {
        panic!("expected substrate error, got {:?}", response.result);
    };
    assert_eq!(error.code, "substrate_error");

    let lock = fixture.state.claim_locks().get(&old_id).expect("previous holder restored");
    assert_eq!(lock.holder_harness, "claude-code");
    assert_eq!(lock.holder_session_id, "sess_a");
}

struct Fixture {
    _temp: TempDir,
    roots: Roots,
    substrate: Substrate,
    state: HandlerState,
}

impl Fixture {
    async fn new(level: u8) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_claimlock01".to_string()) },
        )
        .await
        .expect("init substrate");
        Self { _temp: temp, roots, substrate, state: HandlerState::with_coordination_level(level) }
    }

    async fn write_project_memory(&self, request_id: &str, body: &str) -> String {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new(
                request_id,
                RequestPayload::WriteMemory {
                    body: body.to_string(),
                    title: Some(request_id.to_string()),
                    tags: vec!["project".to_string()],
                    meta: project_meta(request_id, "codex", "sess_seed"),
                },
            ),
        )
        .await;
        let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
            panic!("expected governed write success, got {:?}", response.result);
        };
        write.id.expect("write id")
    }

    async fn supersede(&self, input: SupersedeInput<'_>) -> memoryd::protocol::ResponseEnvelope {
        handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new(
                input.request_id,
                RequestPayload::Supersede {
                    old_id: input.old_id.to_string(),
                    content: input.content.to_string(),
                    reason: "test supersede".to_string(),
                    meta: input.meta,
                },
            ),
        )
        .await
    }

    fn acquire_lock(&self, memory_id: &str, harness: &str, session_id: &str) {
        self.state.claim_locks().acquire(ClaimLockAcquireRequest::new(
            memory_id,
            session_id,
            harness,
            self.state.claim_lock_ttl(),
        ));
    }

    fn jsonl_events(&self) -> Vec<memory_substrate::events::Event> {
        read_events(&self.roots.repo.join("events/dev_claimlock01.jsonl")).expect("read events")
    }

    fn sqlite_contention_count(&self, memory_id: &str, holder: &str, contender: &str) -> i64 {
        let conn = Connection::open(self.roots.runtime.join("index.sqlite")).expect("open sqlite");
        let holder_pattern = format!("%\"holder\":\"{holder}\"%");
        let contender_pattern = format!("%\"contender\":\"{contender}\"%");
        conn.query_row(
            "SELECT COUNT(*) FROM events_log \
             WHERE kind = 'claim_lock_contention' AND memory_id = ?1 AND payload_json LIKE ?2 AND payload_json LIKE ?3",
            (memory_id, holder_pattern, contender_pattern),
            |row| row.get(0),
        )
        .expect("count contention rows")
    }

    fn create_directory_at_next_memory_path(&self, old_id: &str) -> std::path::PathBuf {
        let next_id = next_memory_id_after(old_id);
        let path = self.roots.repo.join(format!("projects/claim-lock-test/decisions/{next_id}.md"));
        std::fs::create_dir(&path).expect("create blocker directory");
        path
    }
}

struct SupersedeInput<'a> {
    request_id: &'a str,
    old_id: &'a str,
    content: &'a str,
    meta: serde_json::Value,
}

impl<'a> SupersedeInput<'a> {
    fn new(request_id: &'a str, old_id: &'a str, content: &'a str, meta: serde_json::Value) -> Self {
        Self { request_id, old_id, content, meta }
    }
}

fn project_meta(summary: &str, harness: &str, session_id: &str) -> serde_json::Value {
    json!({
        "namespace": "project",
        "type": "project",
        "summary": summary,
        "canonical_namespace_id": "proj_claim_lock_test",
        "namespace_alias": "claim-lock-test",
        "confidence": 0.95,
        "sensitivity": "internal",
        "source_kind": "user",
        "explicit_user_context": true,
        "session_id": session_id,
        "harness": harness
    })
}

fn project_meta_with_mode(summary: &str, harness: &str, session_id: &str, mode: &str) -> serde_json::Value {
    let mut meta = project_meta(summary, harness, session_id);
    meta.as_object_mut().expect("project meta is object").insert("concurrent_session_mode".to_string(), json!(mode));
    meta
}

fn next_memory_id_after(old_id: &str) -> String {
    let (prefix, sequence) = old_id.rsplit_once('_').expect("memory id has sequence suffix");
    let next_sequence = sequence.parse::<u64>().expect("memory id sequence is numeric") + 1;
    format!("{prefix}_{next_sequence:06}")
}

fn assert_promoted(response: memoryd::protocol::ResponseEnvelope) -> memoryd::protocol::GovernanceSupersedeResponse {
    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede success, got {:?}", response.result);
    };
    assert_eq!(supersede.status, GovernanceStatus::Promoted);
    supersede
}

fn assert_promoted_without_warning(response: memoryd::protocol::ResponseEnvelope) {
    let supersede = assert_promoted(response);
    assert!(supersede.warning.is_none(), "unexpected warning: {:?}", supersede.warning);
}
