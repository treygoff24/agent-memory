//! Reality Check request handlers: the read-side run response and the mutating
//! confirm/correct/forget/not-relevant flows with their shared respond plumbing.

use super::*;

pub(crate) async fn reality_check_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: RealityCheckRequest,
) -> Result<ResponsePayload, HandlerError> {
    match request {
        RealityCheckRequest::List { namespace, limit } => {
            let handler = RcSessionHandler::new(substrate);
            let now = chrono::Utc::now();
            let response = handler.list(namespace, limit, now).await.map_err(HandlerError::substrate)?;
            Ok(ResponsePayload::RealityCheck(response))
        }
        RealityCheckRequest::History { limit } => {
            let history = crate::state::RcHistoryStore::new(&substrate.roots().runtime)
                .load(chrono::Utc::now(), limit)
                .map_err(HandlerError::substrate)?;
            Ok(ResponsePayload::RealityCheck(RealityCheckResponse::History {
                sessions: history
                    .sessions
                    .into_iter()
                    .map(|entry| RealityCheckHistorySession {
                        session_id: entry.session_id,
                        started_at: entry.started_at,
                        completed_at: entry.completed_at,
                        items_total: entry.items_total,
                        reviewed: entry.reviewed,
                        confirmed: entry.confirmed,
                        corrected: entry.corrected,
                        forgotten: entry.forgotten,
                        not_relevant: entry.not_relevant,
                        deferred: entry.deferred,
                        remaining: entry.remaining,
                    })
                    .collect(),
            }))
        }
        mutating_request => {
            let _guard = state.reality_check_lock.lock().await;
            reality_check_mutating_response(substrate, mutating_request).await
        }
    }
}

async fn reality_check_mutating_response(
    substrate: &Substrate,
    request: RealityCheckRequest,
) -> Result<ResponsePayload, HandlerError> {
    let handler = RcSessionHandler::new(substrate);
    let now = chrono::Utc::now();
    let response = match request {
        RealityCheckRequest::List { .. } | RealityCheckRequest::History { .. } => {
            unreachable!("read-only requests are handled without the mutation lock")
        }
        RealityCheckRequest::Run { session_id, namespace, limit } => handler
            .run(RcRunRequest { requested_session_id: session_id, namespace, limit, now })
            .await
            .map_err(HandlerError::substrate)?,
        RealityCheckRequest::Respond { session_id, memory_id, action } => {
            reality_check_respond(RealityCheckRespondRequest {
                substrate,
                handler: &handler,
                session_id,
                memory_id,
                action,
                now,
            })
            .await?
        }
        RealityCheckRequest::Skip => {
            let skipped_until = now + chrono::Duration::days(7);
            let mut state = crate::state::DaemonState::load(&substrate.roots().runtime);
            state.reality_check.snooze_until = Some(skipped_until);
            state.save(&substrate.roots().runtime).map_err(HandlerError::substrate)?;
            RealityCheckResponse::Skipped { skipped_until }
        }
        RealityCheckRequest::Snooze { until } => {
            let snooze_until = until.unwrap_or_else(|| now + chrono::Duration::days(7));
            let mut state = crate::state::DaemonState::load(&substrate.roots().runtime);
            state.reality_check.snooze_until = Some(snooze_until);
            state.save(&substrate.roots().runtime).map_err(HandlerError::substrate)?;
            RealityCheckResponse::Snoozed { snooze_until }
        }
        RealityCheckRequest::Reset => {
            let cleared_session = crate::state::RcSessionStore::new(&substrate.roots().runtime)
                .load_if_recent(now)
                .ok()
                .flatten()
                .is_some();
            crate::state::RcSessionStore::new(&substrate.roots().runtime).delete().map_err(HandlerError::substrate)?;
            crate::state::RcPendingCache::delete(&substrate.roots().runtime).map_err(HandlerError::substrate)?;
            RealityCheckResponse::Reset { cleared_pending: 0, cleared_session }
        }
    };
    Ok(ResponsePayload::RealityCheck(response))
}

async fn reality_check_respond(request: RealityCheckRespondRequest<'_>) -> Result<RealityCheckResponse, HandlerError> {
    let RealityCheckRespondRequest { substrate, handler, session_id, memory_id, action, now } = request;
    if let Some(response) = handler
        .try_finalize_completed_session_response(&session_id, &memory_id, now)
        .map_err(HandlerError::substrate)?
    {
        return Ok(response);
    }
    let session = match handler.load_session_for_response(&session_id, &memory_id, now) {
        Ok(session) => session,
        Err(response) => return Ok(*response),
    };

    let advance = match action {
        RealityCheckAction::Confirm => {
            confirm_reality_check_item(substrate, &session_id, &memory_id, now).await?;
            RcSessionAdvance::Confirmed
        }
        RealityCheckAction::Correct { new_body } => {
            match correct_reality_check_item(substrate, &session_id, &memory_id, new_body).await? {
                None => {
                    return Ok(reality_check_refused(
                        &session_id,
                        &memory_id,
                        "correction refused",
                        RespondRefusalKind::GovernanceRefused,
                    ))
                }
                Some(response) => {
                    if let RealityCheckResponse::RespondRefused { .. } = response {
                        return Ok(response);
                    }
                }
            }
            RcSessionAdvance::Corrected
        }
        RealityCheckAction::Forget { reason } => {
            if reason.trim().len() < 3 {
                return Ok(reality_check_refused(
                    &session_id,
                    &memory_id,
                    "reason too short",
                    RespondRefusalKind::InvalidAction,
                ));
            }
            forget_reality_check_item(substrate, &session_id, &memory_id, sanitize_forget_reason(&reason)).await?;
            RcSessionAdvance::Forgotten
        }
        RealityCheckAction::NotRelevant => {
            not_relevant_reality_check_item(substrate, &session_id, &memory_id).await?;
            RcSessionAdvance::NotRelevant
        }
        RealityCheckAction::SkipThisWeek => RcSessionAdvance::Deferred,
    };

    handler.advance(RcAdvanceRequest { session, memory_id, advance, now }).await.map_err(HandlerError::substrate)
}

struct RealityCheckRespondRequest<'a> {
    substrate: &'a Substrate,
    handler: &'a RcSessionHandler<'a>,
    session_id: String,
    memory_id: MemoryId,
    action: RealityCheckAction,
    now: chrono::DateTime<chrono::Utc>,
}

async fn confirm_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), HandlerError> {
    mutate_reality_check_metadata(substrate, memory_id, |memory| {
        memory.frontmatter.updated_at = now;
        memory.frontmatter.observed_at = Some(now);
        memory.frontmatter.confidence = (memory.frontmatter.confidence + 0.02).min(1.0);
    })
    .await?;
    substrate
        .record_event_best_effort(EventKind::RealityCheckConfirmed {
            id: memory_id.clone(),
            session_id: session_id.to_owned(),
        })
        .map_err(|error| HandlerError::substrate(format!("record reality check confirmation: {error}")))
}

async fn correct_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
    new_body: String,
) -> Result<Option<RealityCheckResponse>, HandlerError> {
    if new_body.trim().is_empty() {
        return Ok(Some(reality_check_refused(
            session_id,
            memory_id,
            "correction body must not be empty",
            RespondRefusalKind::InvalidAction,
        )));
    }
    let old = substrate.read_memory(memory_id).await.map_err(HandlerError::substrate)?;
    let response = match governance_supersede_response(
        substrate,
        None,
        GovernanceSupersedeRequest {
            old_id: memory_id.as_str().to_owned(),
            content: new_body,
            reason: "reality check correction".to_owned(),
            meta: serde_json::json!({
                "namespace": governance_namespace_meta(&old.frontmatter),
                "type": governance_type_meta(old.frontmatter.memory_type),
                "summary": old.frontmatter.summary,
                "confidence": old.frontmatter.confidence,
                "sensitivity": memory_ops::serialized_enum_value(&old.frontmatter.sensitivity),
                "source_kind": "user",
                "explicit_user_context": true
            }),
        },
    )
    .await
    {
        Ok(response) => response,
        Err(error) if error.code == "privacy_error" => {
            return Ok(Some(reality_check_refused(
                session_id,
                memory_id,
                format!("governance refused correction: {}", error.message),
                RespondRefusalKind::GovernanceRefused,
            )));
        }
        Err(error) => return Err(error),
    };
    let ResponsePayload::GovernanceSupersede(supersede) = response else {
        return Ok(Some(reality_check_refused(
            session_id,
            memory_id,
            "unexpected correction response",
            RespondRefusalKind::GovernanceRefused,
        )));
    };
    if supersede.status == GovernanceStatus::Promoted {
        return Ok(Some(RealityCheckResponse::Pending {
            session_id: Some(session_id.to_owned()),
            items: Vec::new(),
            total_scored: 0,
            last_completed_at: None,
        }));
    }

    let kind = if supersede.reason == Some(GovernanceRefusalReason::Tombstone) {
        RespondRefusalKind::TombstoneMatch
    } else {
        RespondRefusalKind::GovernanceRefused
    };
    Ok(Some(reality_check_refused(
        session_id,
        memory_id,
        format!("governance refused correction: {:?}", supersede.reason),
        kind,
    )))
}

async fn forget_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
    reason: String,
) -> Result<(), HandlerError> {
    let response = governance_forget_response(substrate, memory_id.as_str().to_owned(), reason.clone()).await?;
    let ResponsePayload::GovernanceForget(forget) = response else {
        return Err(HandlerError::substrate("unexpected forget response"));
    };
    if forget.status != GovernanceStatus::Tombstoned {
        return Err(HandlerError::substrate("governance did not tombstone memory"));
    }
    substrate
        .record_event_best_effort(EventKind::RealityCheckForgotten {
            id: memory_id.clone(),
            session_id: session_id.to_owned(),
            reason,
        })
        .map_err(|error| HandlerError::substrate(format!("record reality check forgotten: {error}")))
}

async fn not_relevant_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
) -> Result<(), HandlerError> {
    mutate_reality_check_metadata(substrate, memory_id, |memory| {
        memory.frontmatter.updated_at = chrono::Utc::now();
        memory.frontmatter.retrieval_policy.passive_recall = false;
        if !memory.frontmatter.tags.iter().any(|tag| tag == "reality_check_not_relevant") {
            memory.frontmatter.tags.push("reality_check_not_relevant".to_owned());
        }
    })
    .await?;
    substrate
        .record_event_best_effort(EventKind::RealityCheckNotRelevant {
            id: memory_id.clone(),
            session_id: session_id.to_owned(),
        })
        .map_err(|error| HandlerError::substrate(format!("record reality check not relevant: {error}")))
}

async fn mutate_reality_check_metadata(
    substrate: &Substrate,
    memory_id: &MemoryId,
    mutate: impl FnOnce(&mut Memory),
) -> Result<(), HandlerError> {
    let envelope = substrate.read_memory_envelope(memory_id).await.map_err(HandlerError::substrate)?;
    if !matches!(envelope.content, MemoryContent::Plaintext(_)) {
        return substrate.update_encrypted_memory_metadata(memory_id, mutate).await.map_err(HandlerError::substrate);
    }
    let mut memory = envelope.metadata;
    mutate(&mut memory);
    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::AdminRepair,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-reality-check".to_owned()),
                reason: Some("reality check metadata update".to_owned()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map(|_| ())
        .map_err(HandlerError::substrate)
}

fn reality_check_refused(
    session_id: &str,
    memory_id: &MemoryId,
    reason: impl Into<String>,
    kind: RespondRefusalKind,
) -> RealityCheckResponse {
    RealityCheckResponse::RespondRefused {
        session_id: session_id.to_owned(),
        memory_id: memory_id.clone(),
        reason: reason.into(),
        kind,
    }
}
