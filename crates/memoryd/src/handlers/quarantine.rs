//! Quarantine admin handlers.

use std::collections::BTreeSet;

use super::*;
use crate::notifications::dispatcher::blocking_merge_conflict_dedup_key;

pub(crate) async fn quarantine_resolve_response(
    substrate: &Substrate,
    state: &HandlerState,
    id: String,
    mode: QuarantineResolutionMode,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = HandlerError::parse_memory_id(id)?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let MemoryContent::Plaintext(body) = &envelope.content else {
        return Err(HandlerError::invalid_request(
            "encrypted quarantine resolution requires an encrypted lifecycle update API",
        ));
    };
    // Refuse to certify a body that still carries git conflict markers as Active/Trusted.
    // The CLI's --accept-* flags record operator intent in the audit trail but do NOT
    // auto-select a side (the substrate has no side-swap API yet); the operator must
    // resolve the file first, so a marker-bearing body means the conflict is unresolved.
    if has_git_conflict_markers(body) {
        return Err(HandlerError::invalid_request(
            "quarantined memory still contains git conflict markers (<<<<<<< / >>>>>>>); \
             resolve the conflict in the file before running `quarantine resolve`",
        ));
    }

    let mut memory = envelope.metadata;
    if memory.frontmatter.schema_version > memory_substrate::merge::MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION {
        return Err(HandlerError::invalid_request(format!(
            "memory schema_version {} exceeds merge-driver supported schema_version {}",
            memory.frontmatter.schema_version,
            memory_substrate::merge::MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION
        )));
    }
    if !matches!(memory.frontmatter.status, MemoryStatus::Quarantined)
        && !matches!(memory.frontmatter.trust_level, TrustLevel::Quarantined)
    {
        return Err(HandlerError::invalid_request("memory is not quarantined"));
    }

    let path = memory
        .path
        .as_ref()
        .map(|path| path.as_str().to_owned())
        .ok_or_else(|| HandlerError::invalid_request("quarantined memory has no repository path"))?;

    memory.frontmatter.status = MemoryStatus::Active;
    memory.frontmatter.trust_level = TrustLevel::Trusted;
    memory.frontmatter.requires_user_confirmation = false;
    memory.frontmatter.review_state = None;
    memory.frontmatter.write_policy.human_review_required = false;
    memory.frontmatter.updated_at = chrono::Utc::now();

    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-quarantine".to_string()),
                reason: Some(format!("quarantine resolve {}", mode.as_str())),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(HandlerError::substrate)?;

    let remaining_blocking_conflicts = current_blocking_conflict_paths(substrate).await?;
    prune_resolved_blocking_notifications(state, &remaining_blocking_conflicts);

    Ok(ResponsePayload::QuarantineResolve(QuarantineResolveResponse {
        id: memory_id.as_str().to_owned(),
        path,
        mode,
        remaining_blocking_conflicts,
    }))
}

async fn current_blocking_conflict_paths(substrate: &Substrate) -> Result<Vec<String>, HandlerError> {
    let mut paths = substrate
        .query_recall_index_including_metadata_only(RecallIndexQuery {
            statuses: vec![MemoryStatus::Quarantined],
            hydrate: AuxScope::None,
            source_identity: false,
            ..RecallIndexQuery::default()
        })
        .await
        .map_err(HandlerError::substrate)?
        .into_iter()
        .map(|row| row.path.to_string())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Whether `body` still contains git conflict open/close markers — an unresolved
/// merge. Only the unambiguous 7-char open (`<<<<<<<`) and close (`>>>>>>>`) markers
/// are matched (no markdown line starts with seven `<`/`>`); the `=======` divider is
/// deliberately not matched because a 7-`=` line is a legitimate markdown construct.
fn has_git_conflict_markers(body: &str) -> bool {
    body.lines().any(|line| line.starts_with("<<<<<<<") || line.starts_with(">>>>>>>"))
}

fn prune_resolved_blocking_notifications(state: &HandlerState, current_paths: &[String]) {
    const PREFIX: &str = "blocking_merge_conflict:";

    let current_keys =
        current_paths.iter().map(|path| blocking_merge_conflict_dedup_key(path)).collect::<BTreeSet<_>>();
    let passive = state.passive_notifications();
    for key in passive.dedup_keys() {
        if key.starts_with(PREFIX) && !current_keys.contains(&key) {
            passive.clear_by_key(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::has_git_conflict_markers;

    #[test]
    fn flags_unresolved_conflicts_but_not_markdown() {
        assert!(has_git_conflict_markers("a\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> theirs\nb\n"));
        assert!(has_git_conflict_markers(">>>>>>> theirs\n"));
        // A 7-`=` line is a legitimate markdown setext underline / divider — it must NOT
        // be read as an unresolved conflict.
        assert!(!has_git_conflict_markers("Title\n=======\nresolved body\n"));
        assert!(!has_git_conflict_markers("clean resolved content\n"));
    }
}
