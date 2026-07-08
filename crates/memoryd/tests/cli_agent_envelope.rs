//! Task 2 matrix: the agent envelope + exit-code contract
//! (`docs/api/memoryd-cli-contract-v1.md`).
//!
//! The synthetic cells drive the pure renderer `render_agent_response` with
//! hand-built daemon responses (real serde types), asserting envelope shape,
//! exit code, and output stream for every cell the plan enumerates: success,
//! daemon-error, empty-result, and the DECISION-4 refused / candidate writes.
//! The live cells drive the built binary for daemon-down (exit 75) and stdout
//! byte-stability across identical invocations.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use memoryd::cli::output::{render_agent_response, AgentRender};
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, GovernanceWriteResponse, ResponseEnvelope, ResponsePayload, SearchHit,
    SearchResponse, WriteNoteResponse,
};
use serde_json::Value;

fn envelope_json(render: &AgentRender) -> Value {
    serde_json::to_value(&render.envelope).expect("agent envelope serializes")
}

fn success(payload: ResponsePayload) -> AgentRender {
    render_agent_response(&ResponseEnvelope::success("cli-test", payload))
}

// --- daemon-error cells (command-agnostic error rendering) ---------------------

#[test]
fn daemon_error_not_found_maps_to_66_on_stderr() {
    let render = render_agent_response(&ResponseEnvelope::error("cli-get", "not_found", "no memory here", false));
    assert_eq!(render.exit_code, 66);
    assert!(render.to_stderr, "errors go to stderr");
    let json = envelope_json(&render);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "not_found");
    assert_eq!(json["error"]["retryable"], false);
    assert!(json["error"]["suggested_fix"].as_str().unwrap().contains("search"));
    assert_eq!(json["meta"]["schema_version"], "1.0");
}

#[test]
fn daemon_error_substrate_maps_to_75_and_preserves_retryable() {
    let render = render_agent_response(&ResponseEnvelope::error("cli-get", "substrate_error", "transient", true));
    assert_eq!(render.exit_code, 75);
    assert!(render.to_stderr);
    assert_eq!(envelope_json(&render)["error"]["retryable"], true);
}

#[test]
fn daemon_error_invalid_request_maps_to_65() {
    let render = render_agent_response(&ResponseEnvelope::error("cli-get", "invalid_request", "bad id", false));
    assert_eq!(render.exit_code, 65);
    assert!(render.to_stderr);
}

// --- success + empty-result cells ---------------------------------------------

#[test]
fn write_note_success_is_ok_true_on_stdout_exit_0() {
    let render = success(ResponsePayload::WriteNote(WriteNoteResponse {
        id: "mem_20260708_a1b2c3d4e5f60718_000001".to_string(),
        summary: "a note".to_string(),
    }));
    assert_eq!(render.exit_code, 0);
    assert!(!render.to_stderr, "successes go to stdout");
    let json = envelope_json(&render);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["id"], "mem_20260708_a1b2c3d4e5f60718_000001");
    assert!(json["error"].is_null(), "success carries no error key");
    assert_eq!(json["meta"]["warnings"].as_array().unwrap().len(), 0);
}

#[test]
fn empty_search_is_success_with_broadening_warning() {
    let render =
        success(ResponsePayload::Search(SearchResponse { hits: Vec::new(), total: 0, guidance: "none".to_string() }));
    assert_eq!(render.exit_code, 0);
    assert!(!render.to_stderr);
    let json = envelope_json(&render);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["hits"].as_array().unwrap().len(), 0);
    let warnings = json["meta"]["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "empty search carries a broadening hint");
    assert!(warnings[0].as_str().unwrap().contains("broaden"));
}

#[test]
fn nonempty_search_has_no_warning() {
    let render = success(ResponsePayload::Search(SearchResponse {
        hits: vec![SearchHit {
            id: "mem_x".to_string(),
            summary: "s".to_string(),
            snippet: "snip".to_string(),
            body: None,
            score: 0.9,
        }],
        total: 1,
        guidance: "ok".to_string(),
    }));
    assert_eq!(render.exit_code, 0);
    assert_eq!(envelope_json(&render)["meta"]["warnings"].as_array().unwrap().len(), 0);
}

// --- DECISION-4 governance write-status cells ---------------------------------

fn governance_write(status: GovernanceStatus, reason: Option<GovernanceRefusalReason>) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status,
        id: Some("mem_20260708_a1b2c3d4e5f60718_000009".to_string()),
        namespace: Some("me".to_string()),
        reason,
        next_actions: vec!["do the thing".to_string()],
        policy_applied: None,
        policy_source: None,
        existing_id: None,
        similarity_degraded: None,
    }
}

#[test]
fn promoted_write_is_plain_success() {
    let render = success(ResponsePayload::GovernanceWrite(governance_write(GovernanceStatus::Promoted, None)));
    assert_eq!(render.exit_code, 0);
    assert!(!render.to_stderr);
    let json = envelope_json(&render);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "promoted");
    assert_eq!(json["meta"]["warnings"].as_array().unwrap().len(), 0);
}

#[test]
fn candidate_write_is_success_but_warns_not_active() {
    let render = success(ResponsePayload::GovernanceWrite(governance_write(GovernanceStatus::Candidate, None)));
    assert_eq!(render.exit_code, 0, "queued writes are accepted, not failed");
    assert!(!render.to_stderr);
    let json = envelope_json(&render);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "candidate", "data.status is mandatory");
    let warnings = json["meta"]["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].as_str().unwrap().contains("not yet active"));
}

#[test]
fn quarantined_write_is_success_but_warns_not_active() {
    let render = success(ResponsePayload::GovernanceWrite(governance_write(GovernanceStatus::Quarantined, None)));
    assert_eq!(render.exit_code, 0);
    let json = envelope_json(&render);
    assert_eq!(json["data"]["status"], "quarantined");
    assert!(json["meta"]["warnings"][0].as_str().unwrap().contains("not yet active"));
}

#[test]
fn refused_write_is_error_exit_65_with_reason_code_and_fix() {
    let render = success(ResponsePayload::GovernanceWrite(governance_write(
        GovernanceStatus::Refused,
        Some(GovernanceRefusalReason::Contradiction),
    )));
    assert_eq!(render.exit_code, 65);
    assert!(render.to_stderr, "a refused write is a failure, on stderr");
    let json = envelope_json(&render);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "contradiction", "refusal code is the governance reason");
    assert_eq!(json["error"]["retryable"], false);
    assert!(json["error"]["suggested_fix"].as_str().unwrap().contains("supersede"));
    // Daemon next_actions surface in details so the agent keeps the guidance.
    assert!(json["error"]["details"]["next_actions"].is_array());
}

#[test]
fn refused_write_tombstone_reason_points_at_search() {
    let render = success(ResponsePayload::GovernanceWrite(governance_write(
        GovernanceStatus::Refused,
        Some(GovernanceRefusalReason::Tombstone),
    )));
    assert_eq!(render.exit_code, 65);
    assert_eq!(envelope_json(&render)["error"]["code"], "tombstone");
}

// --- live binary cells --------------------------------------------------------

#[test]
fn daemon_down_is_exit_75_error_on_stderr_nothing_on_stdout() {
    let missing = std::env::temp_dir().join("memoryd-agent-envelope-absent.sock");
    let _ = std::fs::remove_file(&missing);
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["search", "anything", "--socket"])
        .arg(&missing)
        .output()
        .expect("run search against a dead socket");
    assert_eq!(output.status.code(), Some(75), "daemon-unreachable is exit 75");
    assert!(output.stdout.is_empty(), "no success frame on stdout when the daemon is down");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    let json: Value = serde_json::from_str(stderr.trim()).expect("stderr is one JSON error envelope");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["retryable"], true);
}

struct ServeGuard {
    child: Child,
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon(repo: &Path, runtime: &Path, socket: &Path) -> ServeGuard {
    let child = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["serve", "--init", "--force-unsafe-durability", "--repo"])
        .arg(repo)
        .arg("--runtime")
        .arg(runtime)
        .arg("--socket")
        .arg(socket)
        .spawn()
        .expect("spawn memoryd serve");
    let guard = ServeGuard { child };
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let ready = Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(["status", "--socket"])
            .arg(socket)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if ready {
            return guard;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("daemon did not become ready within 30s");
}

#[test]
fn identical_search_invocations_are_byte_identical_on_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let socket = temp.path().join("memoryd.sock");
    let _daemon = start_daemon(&repo, &runtime, &socket);

    let run = || {
        let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(["search", "a stable query with no matches", "--socket"])
            .arg(&socket)
            .output()
            .expect("run search");
        assert_eq!(output.status.code(), Some(0), "empty search is exit 0");
        output.stdout
    };
    let first = run();
    let second = run();
    assert_eq!(first, second, "identical search invocations must be byte-identical on stdout");

    // And the empty-result envelope shape holds end-to-end.
    let json: Value = serde_json::from_slice(&first).expect("stdout is one JSON success envelope");
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["hits"].as_array().unwrap().len(), 0);
    assert_eq!(json["meta"]["schema_version"], "1.0");
}

// --- Task 5: error pedagogy -------------------------------------------------

#[test]
fn daemon_error_suggested_fixes_name_the_corrective_move() {
    // Each code's suggested_fix names the exact next move, not just "an error".
    let cases = [
        ("not_found", "search"),
        ("invalid_request", "schema"),
        ("substrate_error", "doctor"),
        ("privacy_error", "encrypted"),
        ("unsupported", "http-static"),
        ("source_capture_failed", "retry"),
        ("embedding_backlog", "retry"),
    ];
    for (code, needle) in cases {
        let render = render_agent_response(&ResponseEnvelope::error("cli", code, "the daemon message", false));
        let json = envelope_json(&render);
        let fix = json["error"]["suggested_fix"].as_str().unwrap_or("");
        assert!(fix.contains(needle), "suggested_fix for `{code}` should mention `{needle}`, got: {fix}");
    }
}

#[test]
fn write_note_stdout_is_pure_json_with_diagnostics_on_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let socket = temp.path().join("memoryd.sock");
    let _daemon = start_daemon(&repo, &runtime, &socket);

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["write-note", "a durable dev note for the pedagogy test", "--socket"])
        .arg(&socket)
        .output()
        .expect("run write-note");
    assert_eq!(output.status.code(), Some(0), "write-note should succeed: {}", String::from_utf8_lossy(&output.stderr));
    // The entire stdout must parse as exactly one JSON object — proving no banner
    // or prose leaked onto the success stream (diagnostics/banner go to stderr).
    let json: Value = serde_json::from_slice(&output.stdout).expect("write-note stdout is pure JSON, no prose");
    assert_eq!(json["ok"], true);
    assert!(json["data"]["id"].is_string(), "write-note returns the new memory id");
}
