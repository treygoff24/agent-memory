use crate::cli::output::{
    governance_write_response_promoted_id, maybe_emit_first_write_banner, print_response, write_note_response_id,
};
use crate::cli::{ForgetArgs, GetArgs, SearchArgs, SupersedeArgs, WriteMemoryArgs, WriteNoteArgs};
use crate::mcp::meta_with_current_cwd_if_missing;
use crate::paths::resolve_socket_arg;
use crate::protocol::RequestPayload;

pub async fn run_search(args: SearchArgs) -> anyhow::Result<()> {
    print_response(
        crate::client::request(
            resolve_socket_arg(&args.socket),
            "cli-search",
            RequestPayload::Search { query: args.query, limit: Some(args.limit), include_body: args.include_body },
        )
        .await?,
    )
}

pub async fn run_get(args: GetArgs) -> anyhow::Result<()> {
    print_response(
        crate::client::request(
            resolve_socket_arg(&args.socket),
            "cli-get",
            RequestPayload::Get { id: args.id, include_provenance: args.include_provenance },
        )
        .await?,
    )
}

pub async fn run_write_note(args: WriteNoteArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    let response =
        crate::client::request(socket.clone(), "cli-write-note", RequestPayload::WriteNote { text: args.text }).await?;
    let written_id = write_note_response_id(&response);
    print_response(response)?;
    if let Some(id) = written_id {
        maybe_emit_first_write_banner(&socket, &id).await;
    }
    Ok(())
}

pub async fn run_write(args: WriteMemoryArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    let response = crate::client::request(
        socket.clone(),
        "cli-write",
        RequestPayload::WriteMemory {
            body: args.body,
            title: args.title,
            tags: args.tags,
            meta: meta_with_current_cwd_if_missing(parse_meta(args.meta)?)?,
        },
    )
    .await?;
    let written_id = governance_write_response_promoted_id(&response);
    print_response(response)?;
    if let Some(id) = written_id {
        maybe_emit_first_write_banner(&socket, &id).await;
    }
    Ok(())
}

pub async fn run_supersede(args: SupersedeArgs) -> anyhow::Result<()> {
    print_response(
        crate::client::request(
            resolve_socket_arg(&args.socket),
            "cli-supersede",
            RequestPayload::Supersede {
                old_id: args.old_id,
                content: args.content,
                reason: args.reason,
                meta: meta_with_current_cwd_if_missing(parse_meta(args.meta)?)?,
            },
        )
        .await?,
    )
}

pub async fn run_forget(args: ForgetArgs) -> anyhow::Result<()> {
    print_response(
        crate::client::request(
            resolve_socket_arg(&args.socket),
            "cli-forget",
            RequestPayload::Forget { id: args.id, reason: args.reason },
        )
        .await?,
    )
}

fn parse_meta(meta: Option<String>) -> anyhow::Result<serde_json::Value> {
    match meta {
        Some(meta) => Ok(serde_json::from_str(&meta)?),
        None => Ok(serde_json::Value::Null),
    }
}
