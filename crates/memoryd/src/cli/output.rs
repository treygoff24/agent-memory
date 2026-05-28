use std::path::Path;

use crate::client;
use crate::protocol::{GovernanceStatus, RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};

pub(crate) fn print_response(response: ResponseEnvelope) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
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
