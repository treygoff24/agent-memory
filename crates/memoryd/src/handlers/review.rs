//! Review-queue request handlers: listing the queue, applying approve/reject
//! decisions (with grounding-rehydration quarantine), and the ReviewDecision model.

use super::memory_ops::serialized_enum_value;
use super::*;

pub(crate) enum ReviewDecision {
    Approve,
    Reject { reason: String },
}

impl ReviewDecision {
    fn apply(&self, memory: &mut Memory) -> &'static str {
        memory.frontmatter.updated_at = chrono::Utc::now();
        memory.frontmatter.requires_user_confirmation = false;
        memory.frontmatter.write_policy.human_review_required = false;
        match self {
            Self::Approve => {
                memory.frontmatter.status = MemoryStatus::Active;
                memory.frontmatter.trust_level = TrustLevel::Trusted;
                memory.frontmatter.review_state = None;
                "approved"
            }
            Self::Reject { reason } => {
                memory.frontmatter.status = MemoryStatus::Archived;
                memory.frontmatter.review_state = Some("rejected".to_string());
                memory.frontmatter.retrieval_policy.index_body = false;
                memory.frontmatter.retrieval_policy.index_embeddings = false;
                memory.frontmatter.extras.insert("review_rejection_reason".to_string(), serde_json::json!(reason));
                "rejected"
            }
        }
    }
}

pub(crate) async fn review_queue_response(
    substrate: &Substrate,
    state: &HandlerState,
    limit: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    let mut envelopes = Vec::new();
    for path in memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path()) {
        let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
        let envelope = substrate.read_path_envelope(&repo_path).await.map_err(HandlerError::substrate)?;
        envelopes.push(review_envelope_from_memory(envelope.metadata));
    }

    let mut queue = ReviewQueue::from_memory_envelopes(envelopes);
    if over_threshold(&queue) {
        state.emit_notification(NotificationEvent::ReviewQueueOverThreshold {
            count: queue.items.len(),
            threshold: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
        });
    }
    queue.items.truncate(limit.unwrap_or(REVIEW_QUEUE_LIMIT_DEFAULT).min(REVIEW_QUEUE_LIMIT_MAX));

    let mut items = queue
        .items
        .into_iter()
        .map(|item| ReviewQueueItemResponse {
            id: item.id,
            summary: bounded(&item.summary, REVIEW_QUEUE_SUMMARY_MAX),
            status: item.status.as_str().to_string(),
            policy_applied: bounded(&item.policy_applied, REVIEW_QUEUE_POLICY_MAX),
            reason: item.reason.map(|reason| bounded(&reason, REVIEW_QUEUE_REASON_MAX)),
            next_actions: item
                .next_actions
                .into_iter()
                .take(4)
                .map(|action| bounded(&action, REVIEW_QUEUE_ACTION_MAX))
                .collect(),
        })
        .collect::<Vec<_>>();
    while serialized_payload_len(&ResponsePayload::ReviewQueue(ReviewQueueResponse { items: items.clone() }))
        > REVIEW_RESPONSE_FRAME_BUDGET
    {
        if items.pop().is_none() {
            break;
        }
    }

    Ok(ResponsePayload::ReviewQueue(ReviewQueueResponse { items }))
}

pub(crate) async fn review_decision_response(
    substrate: &Substrate,
    id: &str,
    decision: ReviewDecision,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    if !matches!(envelope.content, MemoryContent::Plaintext(_)) {
        return Err(HandlerError::invalid_request(
            "encrypted review decisions require an encrypted lifecycle update API",
        ));
    }
    let mut memory = envelope.metadata;
    if !matches!(memory.frontmatter.status, MemoryStatus::Candidate | MemoryStatus::Quarantined)
        || !review_queue_contains(&memory)
    {
        return Err(HandlerError::invalid_request("memory is not eligible for the review queue"));
    }
    if matches!((&decision, memory.frontmatter.status), (ReviewDecision::Approve, MemoryStatus::Quarantined)) {
        return Err(HandlerError::invalid_request("quarantined memories must be resubmitted through governance"));
    }
    if matches!(decision, ReviewDecision::Approve)
        && rehydration::requires_rehydration(&memory)
        && rehydration::verify_dream_candidate(substrate, &memory).await.is_err()
    {
        let summary = bounded(&memory.frontmatter.summary, REVIEW_DECISION_SUMMARY_MAX);
        quarantine_for_grounding_rehydration(substrate, memory).await?;
        let response = ReviewDecisionResponse { id: id.to_string(), status: "quarantined".to_string(), summary };
        return Ok(ResponsePayload::ReviewApprove(response));
    }
    let status = decision.apply(&mut memory);
    let summary = bounded(&memory.frontmatter.summary, REVIEW_DECISION_SUMMARY_MAX);

    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-review".to_string()),
                reason: Some(format!("review {status}")),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(HandlerError::substrate)?;

    let response = ReviewDecisionResponse { id: id.to_string(), status: status.to_string(), summary };
    match decision {
        ReviewDecision::Approve => Ok(ResponsePayload::ReviewApprove(response)),
        ReviewDecision::Reject { .. } => Ok(ResponsePayload::ReviewReject(response)),
    }
}

async fn quarantine_for_grounding_rehydration(substrate: &Substrate, mut memory: Memory) -> Result<(), HandlerError> {
    memory.frontmatter.updated_at = chrono::Utc::now();
    memory.frontmatter.status = MemoryStatus::Quarantined;
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.requires_user_confirmation = true;
    memory.frontmatter.review_state = Some("quarantined".to_string());
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.frontmatter.write_policy.human_review_required = true;
    memory
        .frontmatter
        .extras
        .insert("governance_reason".to_string(), serde_json::json!("grounding_rehydration_failed"));
    memory.frontmatter.merge_diagnostics = Some(serde_json::json!({
        "human_reason": "grounding_rehydration_failed",
        "preserved_sources": [],
        "lifecycle_notes": ["dream grounding rehydration failed before review approval"],
        "evidence_near_duplicates": []
    }));

    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-review".to_string()),
                reason: Some("review grounding_rehydration_failed".to_string()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(HandlerError::substrate)?;
    Ok(())
}

fn review_envelope_from_memory(memory: Memory) -> ReviewMemoryEnvelope {
    ReviewMemoryEnvelope {
        id: memory.frontmatter.id.as_str().to_string(),
        summary: memory.frontmatter.summary,
        status: serialized_enum_value(&memory.frontmatter.status),
        requires_user_confirmation: memory.frontmatter.requires_user_confirmation,
        review_state: memory.frontmatter.review_state,
        policy_applied: memory.frontmatter.write_policy.policy_applied,
        reason: memory.frontmatter.extras.get("governance_reason").and_then(|value| value.as_str()).map(str::to_string),
    }
}

fn review_queue_contains(memory: &Memory) -> bool {
    let envelope = review_envelope_from_memory(memory.clone());
    ReviewQueue::from_memory_envelopes(vec![envelope])
        .items
        .iter()
        .any(|item| item.id == memory.frontmatter.id.as_str())
}
