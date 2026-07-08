use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value};

use crate::cli::exit::{agent_exit_code, EXIT_INTERNAL, EXIT_INVALID_INPUT, EXIT_TRANSIENT};
use crate::client;
use crate::protocol::{
    GovernanceRefusalReason, GovernanceStatus, ProtocolError, RequestPayload, ResponseEnvelope, ResponsePayload,
    ResponseResult,
};
use crate::util::serialized_enum_value;

pub(crate) fn print_response(response: ResponseEnvelope) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

/// Schema version emitted in every agent envelope's `meta`. Pinned by the CLI
/// contract (`docs/api/memoryd-cli-contract-v1.md`).
pub(crate) const SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Serialize)]
pub struct AgentEnvelope {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AgentError>,
    meta: AgentMeta,
}

#[derive(Debug, Serialize)]
struct AgentMeta {
    schema_version: &'static str,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_fix: Option<String>,
}

impl AgentEnvelope {
    fn success(data: Value, warnings: Vec<String>) -> Self {
        Self { ok: true, data: Some(data), error: None, meta: AgentMeta { schema_version: SCHEMA_VERSION, warnings } }
    }

    fn failure(error: AgentError) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error),
            meta: AgentMeta { schema_version: SCHEMA_VERSION, warnings: Vec::new() },
        }
    }
}

/// The rendered agent envelope plus how to deliver it: which exit code and which
/// stream. Pure and inspectable so `tests/cli_agent_envelope.rs` can assert the
/// full matrix without spawning a process.
#[derive(Debug)]
pub struct AgentRender {
    pub envelope: AgentEnvelope,
    pub exit_code: i32,
    pub to_stderr: bool,
}

/// Serialize a covered payload's inner DTO to a JSON value for the envelope
/// `data`. These are plain serializable DTOs, so failure is a programmer error.
fn payload_value<T: Serialize>(payload: &T) -> Value {
    serde_json::to_value(payload).expect("covered response payload serializes to JSON")
}

/// Render a daemon response as the agent envelope. Errors and governance
/// refusals go to stderr with a nonzero exit; successes (including queued writes
/// and empty results) go to stdout with exit 0.
pub fn render_agent_response(response: &ResponseEnvelope) -> AgentRender {
    match &response.result {
        ResponseResult::Error(error) => render_daemon_error(error),
        ResponseResult::Success(payload) => render_success(payload),
    }
}

fn render_daemon_error(error: &ProtocolError) -> AgentRender {
    AgentRender {
        envelope: AgentEnvelope::failure(AgentError {
            code: error.code.clone(),
            message: error.message.clone(),
            details: None,
            retryable: error.retryable,
            suggested_fix: suggested_fix_for(&error.code),
        }),
        exit_code: agent_exit_code(&error.code),
        to_stderr: true,
    }
}

fn render_success(payload: &ResponsePayload) -> AgentRender {
    if let Some(render) = render_governance(payload) {
        return render;
    }
    match covered_payload(payload) {
        Some((data, warnings)) => {
            AgentRender { envelope: AgentEnvelope::success(data, warnings), exit_code: 0, to_stderr: false }
        }
        None => AgentRender {
            envelope: AgentEnvelope::failure(AgentError {
                code: "unexpected_payload".to_string(),
                message: "daemon returned a payload this command does not cover".to_string(),
                details: None,
                retryable: false,
                suggested_fix: None,
            }),
            exit_code: EXIT_INTERNAL,
            to_stderr: true,
        },
    }
}

/// Extract the inner DTO and any advisory warnings for a covered read/write
/// payload. Governance write/supersede/forget are handled separately by
/// [`render_governance`]. Returns `None` for payloads no covered command emits.
fn covered_payload(payload: &ResponsePayload) -> Option<(Value, Vec<String>)> {
    let value = match payload {
        ResponsePayload::Search(response) => {
            let warnings = if response.hits.is_empty() {
                vec!["no matches; broaden the query or drop filters, or this topic may be unrecorded".to_string()]
            } else {
                Vec::new()
            };
            return Some((payload_value(response), warnings));
        }
        ResponsePayload::Get(response) => payload_value(response),
        ResponsePayload::WriteNote(response) => payload_value(response),
        ResponsePayload::CaptureSource(response) => payload_value(response),
        ResponsePayload::Reveal(response) => payload_value(response),
        ResponsePayload::Observe(response) => payload_value(response),
        ResponsePayload::Status(response) => payload_value(response),
        _ => return None,
    };
    Some((value, Vec::new()))
}

/// Apply the DECISION-4 governance write-status mapping. `Refused` becomes an
/// error envelope (exit 65); `Candidate`/`Quarantined` stay successes but carry a
/// mandatory `data.status` and a "not yet active" warning; `Promoted`/
/// `Tombstoned` are plain successes. Returns `None` for non-governance payloads.
fn render_governance(payload: &ResponsePayload) -> Option<AgentRender> {
    let (status, reason, next_actions, data) = match payload {
        ResponsePayload::GovernanceWrite(response) => {
            (response.status.clone(), response.reason, response.next_actions.clone(), payload_value(response))
        }
        ResponsePayload::GovernanceSupersede(response) => {
            (response.status.clone(), response.reason, Vec::new(), payload_value(response))
        }
        ResponsePayload::GovernanceForget(response) => {
            (response.status.clone(), response.reason, Vec::new(), payload_value(response))
        }
        _ => return None,
    };
    let render = match status {
        GovernanceStatus::Promoted | GovernanceStatus::Tombstoned => {
            AgentRender { envelope: AgentEnvelope::success(data, Vec::new()), exit_code: 0, to_stderr: false }
        }
        GovernanceStatus::Candidate => AgentRender {
            envelope: AgentEnvelope::success(
                data,
                vec!["accepted into the review queue; not yet active — check `memoryd review queue`".to_string()],
            ),
            exit_code: 0,
            to_stderr: false,
        },
        GovernanceStatus::Quarantined => AgentRender {
            envelope: AgentEnvelope::success(
                data,
                vec!["quarantined for review; not yet active — check `memoryd review queue`".to_string()],
            ),
            exit_code: 0,
            to_stderr: false,
        },
        GovernanceStatus::Refused => {
            let code = reason.map(|reason| serialized_enum_value(&reason)).unwrap_or_else(|| "refused".to_string());
            let details = (!next_actions.is_empty()).then(|| json!({ "next_actions": next_actions }));
            AgentRender {
                envelope: AgentEnvelope::failure(AgentError {
                    code,
                    message: refusal_message(reason),
                    details,
                    retryable: false,
                    suggested_fix: refusal_suggested_fix(reason),
                }),
                exit_code: EXIT_INVALID_INPUT,
                to_stderr: true,
            }
        }
    };
    Some(render)
}

fn refusal_message(reason: Option<GovernanceRefusalReason>) -> String {
    match reason {
        Some(GovernanceRefusalReason::Contradiction) => {
            "governance refused the write: it contradicts an existing memory".to_string()
        }
        Some(GovernanceRefusalReason::Tombstone) => {
            "governance refused the write: this content was previously forgotten".to_string()
        }
        Some(GovernanceRefusalReason::Superseded) => {
            "governance refused the write: the target was already superseded".to_string()
        }
        Some(GovernanceRefusalReason::Policy) => "governance refused the write: policy disallows it".to_string(),
        Some(GovernanceRefusalReason::Grounding) => {
            "governance refused the write: grounding evidence is missing or insufficient".to_string()
        }
        Some(GovernanceRefusalReason::Privacy) => {
            "governance refused the write: privacy classification disallows it".to_string()
        }
        Some(GovernanceRefusalReason::ReviewRequired) => {
            "governance refused the write: human review is required first".to_string()
        }
        None => "governance refused the write".to_string(),
    }
}

fn refusal_suggested_fix(reason: Option<GovernanceRefusalReason>) -> Option<String> {
    let fix = match reason {
        Some(GovernanceRefusalReason::Contradiction) => {
            "run `memoryd search <topic>` to find the conflicting memory, then `memoryd supersede <old-id> ...` instead of a fresh write"
        }
        Some(GovernanceRefusalReason::Tombstone) => {
            "search for the tombstone with `memoryd search <topic>` before rewriting forgotten content"
        }
        Some(GovernanceRefusalReason::Superseded) => {
            "run `memoryd get <existing-id>` for the current version, then supersede that instead"
        }
        Some(GovernanceRefusalReason::Grounding) => {
            "capture supporting evidence with `memoryd source capture` (e.g. --url or --file), then cite the returned ref in `--meta` as `source_ref` (a single string)"
        }
        Some(GovernanceRefusalReason::Privacy) => {
            "the content — or a cited source artifact — classified as protected; drop the sensitive material or cite a non-sensitive source, since a plaintext governed write cannot carry a privacy-flagged descriptor"
        }
        Some(GovernanceRefusalReason::Policy) => {
            "the write violates a governance policy named in the message (often a confidence floor for the namespace); adjust the offending `--meta` field to satisfy it"
        }
        Some(GovernanceRefusalReason::ReviewRequired) => {
            "this namespace requires human review; there is no agent-side override — surface it to the user and check `memoryd review queue`"
        }
        None => return None,
    };
    Some(fix.to_string())
}

/// The `suggested_fix` for a daemon error code: the exact corrective move. The
/// daemon `message` already names *what* was wrong and *why*; this names *how* to
/// proceed. Codes whose message is already self-correcting return `None`.
fn suggested_fix_for(code: &str) -> Option<String> {
    let fix = match code {
        "not_found" => "no memory has this id; run `memoryd search <query>` to find the right one",
        "invalid_request" => {
            "the message names the bad input; run `memoryd schema commands --json` for the exact argument shape"
        }
        "substrate_error" => "transient; retry, or run `memoryd doctor` to check daemon health",
        "privacy_error" => {
            "the content classified as protected; rephrase to drop the secret, or record it as an encrypted memory"
        }
        "unsupported" => {
            "unsupported source-capture mode; use `--url` with `--mode http-static`, or `--file` with a local mode"
        }
        "source_capture_failed" => {
            "the capture itself failed (network, integrity, or IO); retry, or verify the URL/file is reachable"
        }
        "embedding_backlog" | "embedding_worker_idle" | "embedding_retry_budget_exhausted" => {
            "the embedding worker is catching up; retry shortly (`memoryd doctor` shows worker state)"
        }
        _ => return None,
    };
    Some(fix.to_string())
}

/// Render `response` as the agent envelope, write it to the correct stream, and
/// exit with the contract exit code. Never returns.
pub(crate) fn emit_and_exit(response: ResponseEnvelope) -> ! {
    let render = render_agent_response(&response);
    emit_render_and_exit(render)
}

fn emit_render_and_exit(render: AgentRender) -> ! {
    let json = serde_json::to_string_pretty(&render.envelope).expect("agent envelope serializes to JSON");
    if render.to_stderr {
        eprintln!("{json}");
    } else {
        println!("{json}");
    }
    std::process::exit(render.exit_code);
}

/// Emit a client-synthesized validation-error envelope and exit. Used for input
/// the CLI rejects before (or instead of) a daemon request — malformed `--meta`
/// JSON, a client-side gate refusal, etc.
pub(crate) fn emit_client_error_and_exit(
    code: &str,
    message: String,
    exit_code: i32,
    suggested_fix: Option<String>,
) -> ! {
    let render = AgentRender {
        envelope: AgentEnvelope::failure(AgentError {
            code: code.to_string(),
            message,
            details: None,
            retryable: false,
            suggested_fix,
        }),
        exit_code,
        to_stderr: true,
    };
    emit_render_and_exit(render)
}

/// Emit a client-synthesized transport-failure envelope (daemon unreachable) and
/// exit. Used when the socket request itself fails before any daemon frame lands.
pub(crate) fn emit_transport_error_and_exit(error: anyhow::Error, socket: &Path) -> ! {
    let render = AgentRender {
        envelope: AgentEnvelope::failure(AgentError {
            code: "daemon_unreachable".to_string(),
            message: format!("could not reach the memoryd daemon at {}: {error:#}", socket.display()),
            details: None,
            retryable: true,
            suggested_fix: Some("start it with `memoryd serve`, or pass the correct --socket".to_string()),
        }),
        exit_code: EXIT_TRANSIENT,
        to_stderr: true,
    };
    emit_render_and_exit(render)
}

/// Pull the freshly-minted memory id out of a `WriteMemory` response if and only if
/// the daemon promoted it (status `Promoted` with a non-empty id). `Candidate`,
/// `Quarantined`, and `Refused` writes do not trigger the first-write banner —
/// the banner is a "your first memory is live" signal, not "your first attempt
/// was processed."
pub(crate) fn governance_write_response_promoted_id(response: &ResponseEnvelope) -> Option<String> {
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = &response.result else {
        return None;
    };
    if !matches!(write.status, GovernanceStatus::Promoted) {
        return None;
    }
    write.id.clone()
}

/// Pull the substrate id out of a `WriteNote` response. Notes land in the
/// substrate immediately (no governance candidate step), so any success counts.
pub(crate) fn write_note_response_id(response: &ResponseEnvelope) -> Option<String> {
    let ResponseResult::Success(ResponsePayload::WriteNote(note)) = &response.result else {
        return None;
    };
    Some(note.id.clone())
}

/// Issue a `Status` query and emit the first-write banner if this looks like the
/// user's very first memory. Failures here are non-fatal: the write already
/// succeeded; the banner is purely a UX hint and we don't want a transient
/// status-query error to mask the underlying success.
pub(crate) async fn maybe_emit_first_write_banner(socket: &Path, id: &str) {
    let envelope = match client::request(socket, "cli-first-write-status", RequestPayload::Status).await {
        Ok(envelope) => envelope,
        Err(_) => return,
    };
    let ResponseResult::Success(ResponsePayload::Status(status)) = envelope.result else {
        return;
    };
    if crate::first_write::should_emit_first_write_banner(&status) {
        let mut stderr = std::io::stderr().lock();
        let _ = crate::first_write::emit_first_write_banner(&mut stderr, id);
    }
}
