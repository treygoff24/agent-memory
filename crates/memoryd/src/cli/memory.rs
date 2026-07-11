use std::path::Path;

use crate::cli::exit::{EXIT_CLIENT_GATE, EXIT_INVALID_INPUT};
use crate::cli::output::{
    emit_and_exit, emit_client_error_and_exit, emit_transport_error_and_exit, governance_write_response_promoted_id,
    maybe_emit_first_write_banner, write_note_response_id,
};
use crate::cli::{
    ForgetArgs, GetArgs, ObserveArgs, RevealArgs, SearchArgs, SupersedeArgs, WriteMemoryArgs, WriteNoteArgs,
};
use crate::mcp::meta_with_current_cwd_if_missing;
use crate::paths::resolve_socket_arg;
use crate::protocol::{RequestPayload, ResponseEnvelope};

const META_EXAMPLE: &str =
    r#"--meta must be a JSON object, e.g. --meta '{"namespace":"me","type":"preference","confidence":0.8}'"#;

/// Issue a covered-command request and hand the response to the agent-envelope
/// emitter, or synthesize a transport-failure envelope if the socket request
/// itself fails. Never returns: both arms exit with a contract exit code.
async fn request_and_emit(socket: &Path, request_id: &str, payload: RequestPayload) -> ! {
    match crate::client::request(socket, request_id, payload).await {
        Ok(response) => emit_and_exit(response),
        Err(error) => emit_transport_error_and_exit(error, socket),
    }
}

pub async fn run_search(args: SearchArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    request_and_emit(
        &socket,
        "cli-search",
        RequestPayload::Search { query: args.query, limit: Some(args.limit), include_body: args.include_body },
    )
    .await
}

pub async fn run_get(args: GetArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    request_and_emit(
        &socket,
        "cli-get",
        RequestPayload::Get { id: args.id, include_provenance: args.include_provenance, full_body: false },
    )
    .await
}

pub async fn run_write_note(args: WriteNoteArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    let response = match crate::client::request(
        &socket,
        "cli-write-note",
        RequestPayload::WriteNote { text: args.text, meta: resolve_meta(args.meta) },
    )
    .await
    {
        Ok(response) => response,
        Err(error) => emit_transport_error_and_exit(error, &socket),
    };
    emit_write_with_banner(response, &socket, write_note_response_id).await
}

pub async fn run_write(args: WriteMemoryArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    let payload = RequestPayload::WriteMemory {
        body: args.body,
        title: args.title,
        tags: args.tags,
        meta: resolve_meta(args.meta),
    };
    let response = match crate::client::request(&socket, "cli-write", payload).await {
        Ok(response) => response,
        Err(error) => emit_transport_error_and_exit(error, &socket),
    };
    emit_write_with_banner(response, &socket, governance_write_response_promoted_id).await
}

pub async fn run_supersede(args: SupersedeArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    request_and_emit(
        &socket,
        "cli-supersede",
        RequestPayload::Supersede {
            old_id: args.old_id,
            content: args.content,
            reason: args.reason,
            meta: resolve_meta(args.meta),
        },
    )
    .await
}

pub async fn run_forget(args: ForgetArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    request_and_emit(&socket, "cli-forget", RequestPayload::Forget { id: args.id, reason: args.reason }).await
}

pub async fn run_reveal(args: RevealArgs) -> anyhow::Result<()> {
    // Client-side gate: refuse before touching the socket. The daemon Reveal
    // handler has no gate of its own (it always audits), so the CLI reimplements
    // the MCP bridge's `allow_reveal` guard here — the refusal must precede any
    // connection so an agent without reveal authority never reaches the daemon.
    if !args.allow_reveal {
        emit_client_error_and_exit(
            "reveal_not_allowed",
            "reveal decrypts protected content and writes an EncryptedContentRevealed audit event".to_string(),
            EXIT_CLIENT_GATE,
            Some("re-run with --allow-reveal once you have user-directed authority to unmask this memory".to_string()),
        );
    }
    let socket = resolve_socket_arg(&args.socket);
    request_and_emit(&socket, "cli-reveal", RequestPayload::Reveal { id: args.id, reason: args.reason }).await
}

pub async fn run_observe(args: ObserveArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    let session_id =
        args.session_id.or_else(|| std::env::var("MEMORUM_SESSION_ID").ok()).unwrap_or_else(|| "cli".to_string());
    let harness = args.harness.or_else(|| std::env::var("MEMORUM_HARNESS").ok()).unwrap_or_else(|| "cli".to_string());
    let cwd =
        std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".to_string());
    request_and_emit(
        &socket,
        "cli-observe",
        RequestPayload::Observe {
            text: args.text,
            kind: args.kind.to_protocol(),
            entities: args.entities,
            cwd,
            session_id,
            harness,
            harness_version: None,
        },
    )
    .await
}

/// Emit a write response through the envelope layer, first firing the
/// stderr-only first-write banner if this write minted the user's first live
/// memory. The banner precedes the envelope emission because `emit_and_exit`
/// diverges.
async fn emit_write_with_banner(
    response: ResponseEnvelope,
    socket: &Path,
    promoted_id: fn(&ResponseEnvelope) -> Option<String>,
) -> ! {
    if let Some(id) = promoted_id(&response) {
        maybe_emit_first_write_banner(socket, &id).await;
    }
    emit_and_exit(response)
}

/// Parse `--meta` JSON and inject the current cwd if absent. On any parse or
/// injection failure, emit a 65-class validation envelope with a minimal valid
/// example and exit — never a bare anyhow error to the user.
fn resolve_meta(meta: Option<String>) -> serde_json::Value {
    let parsed = match meta {
        Some(meta) => match serde_json::from_str::<serde_json::Value>(&meta) {
            Ok(value) => value,
            Err(error) => emit_client_error_and_exit(
                "invalid_request",
                format!("--meta is not valid JSON: {error}"),
                EXIT_INVALID_INPUT,
                Some(META_EXAMPLE.to_string()),
            ),
        },
        None => serde_json::Value::Null,
    };
    match meta_with_current_cwd_if_missing(parsed) {
        Ok(value) => value,
        Err(error) => emit_client_error_and_exit(
            "invalid_request",
            format!("--meta could not be prepared: {error}"),
            EXIT_INVALID_INPUT,
            Some(META_EXAMPLE.to_string()),
        ),
    }
}
