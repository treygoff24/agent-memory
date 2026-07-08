//! Review-queue request handlers: listing the queue, applying approve/reject
//! decisions (with grounding-rehydration quarantine), and the ReviewDecision model.

use super::*;
use crate::dream::calibration;
use crate::util::serialized_enum_value;

pub(crate) enum ReviewDecision {
    Approve,
    Reject { reason: String },
}

impl ReviewDecision {
    fn apply(&self, memory: &mut Memory) -> &'static str {
        let was_quarantined = matches!(memory.frontmatter.status, MemoryStatus::Quarantined);
        memory.frontmatter.updated_at = chrono::Utc::now();
        memory.frontmatter.requires_user_confirmation = false;
        memory.frontmatter.write_policy.human_review_required = false;
        match self {
            Self::Approve => {
                memory.frontmatter.status = MemoryStatus::Active;
                memory.frontmatter.trust_level = TrustLevel::Trusted;
                memory.frontmatter.review_state = None;
                if was_quarantined {
                    // Promotion out of a governance quarantine (merge quarantines are
                    // refused before apply): restore the retrieval surface the
                    // quarantine suppressed at write time (meta.rs stamps
                    // index_body/index_embeddings/passive_recall false for quarantined
                    // writes) and drop the quarantine artifacts. This path is
                    // plaintext-only, so re-enabling indexing is safe.
                    memory.frontmatter.retrieval_policy.passive_recall = true;
                    memory.frontmatter.retrieval_policy.index_body = true;
                    memory.frontmatter.retrieval_policy.index_embeddings = true;
                    memory.frontmatter.merge_diagnostics = None;
                    memory.frontmatter.extras.remove("governance_reason");
                }
                "approved"
            }
            Self::Reject { reason } => {
                memory.frontmatter.status = MemoryStatus::Archived;
                if matches!(memory.frontmatter.trust_level, TrustLevel::Quarantined) {
                    // (Archived, Quarantined) is an invalid lifecycle pair — a
                    // rejected quarantine archives as Untrusted.
                    memory.frontmatter.trust_level = TrustLevel::Untrusted;
                }
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
    // Serve the queue from the derived index: the membership predicate rides
    // `idx_memories_review` and the bounded row fetch returns only what the
    // response renders, instead of reading and re-parsing every canonical memory
    // file on each (repeatedly-polled) inbox request.
    let bounded_limit = limit.unwrap_or(REVIEW_QUEUE_LIMIT_DEFAULT).min(REVIEW_QUEUE_LIMIT_MAX);
    let page = substrate.review_queue(bounded_limit).await.map_err(HandlerError::substrate)?;

    if page.total >= REVIEW_QUEUE_DOGFOOD_THRESHOLD {
        state.emit_notification(NotificationEvent::ReviewQueueOverThreshold {
            count: page.total,
            threshold: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
        });
    }

    // Reuse the exact governance membership/classification logic so the rendered
    // items are byte-for-byte identical to the prior full-walk path. The SQL
    // predicate already restricts to qualifying rows, so each row maps to an
    // item; `from_memory_envelopes` re-applies the same filter as a safety net.
    let queue = ReviewQueue::from_memory_envelopes(page.rows.into_iter().map(review_envelope_from_row));

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
    trim_items_to_frame_budget(&mut items);

    Ok(ResponsePayload::ReviewQueue(ReviewQueueResponse { items }))
}

/// Drop review-queue items from the tail until the serialized response fits
/// within [`REVIEW_RESPONSE_FRAME_BUDGET`], preserving the prior "trim the tail"
/// semantics without re-serializing the whole payload once per popped item.
///
/// `serde_json`'s compact output joins array elements with a single `,` byte and
/// adds no whitespace, so the exact frame length for any prefix of `items`
/// equals the empty-items framing overhead plus the sum of the kept items'
/// individual lengths plus one comma between adjacent kept items. Each item is
/// serialized exactly once and the surviving prefix is found by a single linear
/// scan — O(n) total — instead of the previous clone-and-reserialize loop that
/// was O(n^2) in the number of items trimmed. The empty result (budget too small
/// for even one item) matches the prior loop's `items.pop().is_none()` break.
fn trim_items_to_frame_budget(items: &mut Vec<ReviewQueueItemResponse>) {
    // Framing overhead = the serialized envelope with an empty `items` array.
    let overhead = serialized_payload_len(&ResponsePayload::ReviewQueue(ReviewQueueResponse { items: Vec::new() }));

    // Grow the surviving prefix one item at a time, stopping before the running
    // total would exceed the budget. `running` tracks the exact frame length for
    // the current prefix: overhead, plus each kept item's length, plus one comma
    // between adjacent kept items. Each item is serialized at most once.
    let mut kept = 0usize;
    let mut running = overhead;
    for (index, item) in items.iter().enumerate() {
        let item_len = serde_json::to_vec(item).map_or(MAX_FRAME_BYTES, |bytes| bytes.len());
        let separator = usize::from(index > 0);
        let next = running + separator + item_len;
        if next > REVIEW_RESPONSE_FRAME_BUDGET {
            break;
        }
        running = next;
        kept = index + 1;
    }
    items.truncate(kept);
}

pub(crate) async fn review_decision_response(
    substrate: &Substrate,
    id: &str,
    decision: ReviewDecision,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = HandlerError::parse_memory_id(id)?;
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
    // Snapshot the calibration inputs from the candidate *before* the decision
    // mutates it (`decision.apply` flips status/trust). The candidate is
    // calibration-eligible when it was authored by dreaming or is currently
    // quarantined (dynamics-spec §6). We record only after the decision's write
    // actually lands — so a rehydration-refused approval (which returns early
    // below) never logs an `accept`.
    let calibration_inputs = CalibrationInputs::from_candidate(&memory);
    // Governance quarantines (contradiction/unclear routing, `governance_reason:
    // governance quarantine`) are review-queue items and approvable here — this is
    // their only promotion path. Merge-driver quarantines carry an unresolved git
    // merge and must go through `quarantine resolve --edited` instead, which
    // verifies the on-disk body no longer has conflict markers.
    if matches!((&decision, memory.frontmatter.status), (ReviewDecision::Approve, MemoryStatus::Quarantined))
        && quarantine::has_quarantined_merge_diagnostic(&memory.frontmatter.merge_diagnostics)
    {
        return Err(HandlerError::invalid_request(
            "merge-conflict quarantines must be resolved via `quarantine resolve --edited`",
        ));
    }
    // Approving a dream candidate that asked for grounding rehydration re-runs
    // verification against the *current* substrate before promotion. If the cited
    // evidence drifted, aged out, or vanished since capture, quarantine the memory
    // (so it leaves the candidate queue and is never written Active on this
    // approval) and refuse the approval with a typed error so the review UI shows
    // *why* — rather than silently promoting stale evidence.
    if matches!(decision, ReviewDecision::Approve) && rehydration::requires_rehydration(&memory) {
        if let Err(error) = rehydration::verify_dream_candidate(substrate, &memory).await {
            quarantine_for_grounding_rehydration(substrate, memory).await?;
            return Err(HandlerError::grounding_rehydration(&error));
        }
    }
    if let ReviewDecision::Reject { reason } = &decision {
        // The rejection reason is caller-supplied free text persisted into the canonical
        // file. Classify it so a secret/sensitive reason is refused rather than written
        // unclassified under a hardcoded Trusted outcome. Namespace picks the tier FLOOR,
        // not scanner strictness — `Me` floors at Personal/EncryptAtRest and refuses every
        // reason unconditionally (SEC-001 as originally shipped bricked reject entirely).
        // `Agent` floors at Internal/Plaintext; scanner spans still raise the action, so
        // secret spans refuse and sensitive spans reject as not plaintext-storable.
        let privacy = super::governance::classify_privacy(reason, PrivacyNamespace::Agent, None)?;
        if privacy.storage_action.refuses_storage() {
            return Err(HandlerError::invalid_request(
                "review rejection reason contains secret content and cannot be persisted",
            ));
        }
        if privacy.storage_action.requires_encryption() {
            return Err(HandlerError::invalid_request(
                "review rejection reason contains sensitive content that cannot be stored in plaintext review metadata",
            ));
        }
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

    // The decision landed: append a calibration record for eligible candidates.
    // A failed append must not fail the review (the user's decision already
    // persisted) — log and carry on.
    if let Some(inputs) = calibration_inputs {
        let calibration_decision = match decision {
            ReviewDecision::Approve => calibration::Decision::Accept,
            ReviewDecision::Reject { .. } => calibration::Decision::Reject,
        };
        if let Err(error) = inputs.append(substrate, calibration_decision) {
            tracing::warn!(memory_id = %id, %error, "failed to append review calibration record");
        }
    }

    let response = ReviewDecisionResponse { id: id.to_string(), status: status.to_string(), summary };
    match decision {
        ReviewDecision::Approve => Ok(ResponsePayload::ReviewApprove(response)),
        ReviewDecision::Reject { .. } => Ok(ResponsePayload::ReviewReject(response)),
    }
}

/// Calibration inputs snapshotted from a review candidate before its decision
/// mutates the in-memory copy. `None` when the candidate is not
/// calibration-eligible (not dream-authored and not quarantined).
struct CalibrationInputs {
    candidate_id: String,
    scope: String,
    author_kind: AuthorKind,
    self_reported_confidence: f64,
}

impl CalibrationInputs {
    fn from_candidate(memory: &Memory) -> Option<Self> {
        let fm = &memory.frontmatter;
        let eligible = matches!(fm.author.kind, AuthorKind::Dreaming) || matches!(fm.status, MemoryStatus::Quarantined);
        if !eligible {
            return None;
        }
        Some(Self {
            candidate_id: fm.id.as_str().to_string(),
            scope: calibration::scope_string(fm.scope, fm.canonical_namespace_id.as_deref()),
            author_kind: fm.author.kind,
            self_reported_confidence: fm.confidence,
        })
    }

    /// Resolve the local device id and append the calibration record. The device
    /// id is read from `local-device.yaml` under the runtime root — the same
    /// identity the event log uses (`load_device_id`), via the substrate's
    /// public config loader (no fenced substrate API needed).
    fn append(self, substrate: &Substrate, decision: calibration::Decision) -> std::io::Result<()> {
        let roots = substrate.roots();
        let local = memory_substrate::config::load_local_device_config(&roots.runtime)
            .map_err(std::io::Error::other)?
            .ok_or_else(|| std::io::Error::other("local-device.yaml missing; cannot resolve device id"))?;
        calibration::append_decision(
            &roots.repo,
            &local.device.id,
            substrate.durability_tier(),
            &calibration::DecisionRecord {
                candidate_id: self.candidate_id,
                scope: self.scope,
                author_kind: self.author_kind,
                self_reported_confidence: self.self_reported_confidence,
                decision,
                // The daemon protocol has no edit path today (only approve /
                // reject), so edit-distance is never produced here; it remains a
                // record field for a future edit-capable review surface.
                edit_distance_ratio: None,
                decided_at: chrono::Utc::now(),
                // Approve/Reject requests carry no session id on the wire.
                session_id: None,
            },
        )
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

/// Build a review envelope from an index-projected row. Mirrors
/// [`review_envelope_from_memory`] field-for-field; `status` already arrives as
/// the canonical lowercase string from the index, and `policy_applied` /
/// `governance_reason` are projected from `frontmatter_json`.
fn review_envelope_from_row(row: memory_substrate::model::ReviewQueueRow) -> ReviewMemoryEnvelope {
    ReviewMemoryEnvelope {
        id: row.id,
        summary: row.summary,
        status: row.status,
        requires_user_confirmation: row.requires_user_confirmation,
        review_state: row.review_state,
        policy_applied: row.policy_applied,
        reason: row.governance_reason,
    }
}

fn review_queue_contains(memory: &Memory) -> bool {
    let envelope = review_envelope_from_memory(memory.clone());
    ReviewQueue::from_memory_envelopes(vec![envelope])
        .items
        .iter()
        .any(|item| item.id == memory.frontmatter.id.as_str())
}

#[cfg(test)]
mod trim_tests {
    use super::*;
    use memory_governance::ReviewStatus;

    fn sample_item(index: usize, summary_chars: usize) -> ReviewQueueItemResponse {
        ReviewQueueItemResponse {
            id: format!("mem-{index:08}"),
            summary: "x".repeat(summary_chars),
            status: ReviewStatus::Candidate.as_str().to_string(),
            policy_applied: "default".to_string(),
            reason: Some("review".to_string()),
            next_actions: vec!["approve".to_string(), "reject".to_string()],
        }
    }

    /// Reference implementation: the prior clone-and-pop loop. Drops items from
    /// the tail until the full serialized payload fits the budget.
    fn reference_trim(mut items: Vec<ReviewQueueItemResponse>) -> Vec<ReviewQueueItemResponse> {
        while serialized_payload_len(&ResponsePayload::ReviewQueue(ReviewQueueResponse { items: items.clone() }))
            > REVIEW_RESPONSE_FRAME_BUDGET
        {
            if items.pop().is_none() {
                break;
            }
        }
        items
    }

    #[test]
    fn matches_reference_loop_across_sizes() {
        // Each case is sized so the serialized payload straddles the budget,
        // exercising the no-trim, partial-trim, and trim-to-empty branches.
        for (count, summary_chars) in [(0, 0), (1, 10), (10, 100), (60, 1024), (100, 1024), (1, 70_000)] {
            let items: Vec<_> = (0..count).map(|index| sample_item(index, summary_chars)).collect();
            let mut actual = items.clone();
            trim_items_to_frame_budget(&mut actual);
            let expected = reference_trim(items);
            assert_eq!(actual, expected, "count={count} summary_chars={summary_chars}");
            // The kept payload must always be within budget.
            assert!(
                serialized_payload_len(&ResponsePayload::ReviewQueue(ReviewQueueResponse { items: actual.clone() }))
                    <= REVIEW_RESPONSE_FRAME_BUDGET
                    || actual.is_empty(),
                "kept payload exceeds budget: count={count} summary_chars={summary_chars}"
            );
        }
    }

    #[test]
    fn keeps_everything_when_under_budget() {
        let items: Vec<_> = (0..5).map(|index| sample_item(index, 32)).collect();
        let mut trimmed = items.clone();
        trim_items_to_frame_budget(&mut trimmed);
        assert_eq!(trimmed, items);
    }
}
