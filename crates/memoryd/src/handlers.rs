use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use memory_governance::{
    CandidateMemory, ContradictionTiebreaker, ExistingMemorySummary, FileSourceResolver, GovernanceEngine,
    GovernanceProviders, GovernanceRefusalReason, GovernanceWriteDecision, GroundingVerifier, PolicySet, PolicySource,
    ReviewMemoryEnvelope, ReviewQueue, SessionSpawnResolver, SimilaritySearch, Source as GovernanceSource,
    SourceKind as GovernanceSourceKind, TiebreakOutcome, TombstoneIndex, TombstoneKind, TombstoneRule,
};
use memory_substrate::{
    Author, AuthorKind, ChunkQuery, ClassificationOutcome, EventContext, Frontmatter, Memory, MemoryContent, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, Substrate,
    SupersedeRequest as SubstrateSupersedeRequest, TombstoneRequest, TrustLevel, WriteMode, WritePolicy,
    WriteRequest as SubstrateWriteRequest,
};
use serde::Deserialize;
use serde_json::Value;

use crate::protocol::{
    DoctorFinding, DoctorResponse, GetResponse, GovernanceForgetResponse, GovernanceStatus,
    GovernanceSupersedeResponse, GovernanceWriteResponse, RequestEnvelope, RequestPayload, ResponseEnvelope,
    ResponsePayload, ReviewDecisionResponse, ReviewQueueItemResponse, ReviewQueueResponse, SearchHit, SearchResponse,
    StatusResponse, WriteNoteResponse, MAX_FRAME_BYTES,
};

const SEARCH_LIMIT_DEFAULT: usize = 10;
const SEARCH_LIMIT_MAX: usize = 20;
const SEARCH_SNIPPET_MAX: usize = 240;
const GET_BODY_MAX: usize = 4_096;
const REVIEW_QUEUE_LIMIT_DEFAULT: usize = 50;
const REVIEW_QUEUE_LIMIT_MAX: usize = 100;
const REVIEW_QUEUE_SUMMARY_MAX: usize = 512;
const REVIEW_QUEUE_POLICY_MAX: usize = 128;
const REVIEW_QUEUE_REASON_MAX: usize = 512;
const REVIEW_QUEUE_ACTION_MAX: usize = 96;
const REVIEW_DECISION_SUMMARY_MAX: usize = 512;
const REVIEW_RESPONSE_FRAME_BUDGET: usize = MAX_FRAME_BYTES - 1024;
const DEFAULT_PROJECT_NAMESPACE: &str = "agent-memory";

pub async fn handle_request(substrate: &Substrate, envelope: RequestEnvelope) -> ResponseEnvelope {
    let id = envelope.id;
    match dispatch(substrate, envelope.request).await {
        Ok(payload) => ResponseEnvelope::success(id, payload),
        Err(error) => ResponseEnvelope::error(id, error.code, error.message, error.retryable),
    }
}

async fn dispatch(substrate: &Substrate, request: RequestPayload) -> Result<ResponsePayload, HandlerError> {
    match request {
        RequestPayload::Status => Ok(ResponsePayload::Status(status_response())),
        RequestPayload::Doctor => Ok(ResponsePayload::Doctor(doctor_response(substrate).await)),
        RequestPayload::Search { query, limit, include_body } => {
            search_response(substrate, &query, limit, include_body).await
        }
        RequestPayload::Get { id, include_provenance } => get_response(substrate, &id, include_provenance).await,
        RequestPayload::WriteNote { text } => write_note_response(substrate, &text).await,
        RequestPayload::WriteMemory { body, title, tags, meta } => {
            governance_write_response(substrate, GovernanceWriteRequest { body, title, tags, meta }).await
        }
        RequestPayload::Supersede { old_id, content, reason, meta } => {
            governance_supersede_response(substrate, GovernanceSupersedeRequest { old_id, content, reason, meta }).await
        }
        RequestPayload::Forget { id, reason } => governance_forget_response(substrate, id, reason).await,
        RequestPayload::ReviewQueue { limit } => review_queue_response(substrate, limit).await,
        RequestPayload::ReviewApprove { id } => review_decision_response(substrate, &id, ReviewDecision::Approve).await,
        RequestPayload::ReviewReject { id, reason } => {
            review_decision_response(substrate, &id, ReviewDecision::Reject { reason }).await
        }
    }
}

fn status_response() -> StatusResponse {
    StatusResponse {
        state: "ready".to_string(),
        guidance: "memoryd handlers are backed by the Stream A substrate.".to_string(),
    }
}

async fn doctor_response(substrate: &Substrate) -> DoctorResponse {
    let report = substrate.doctor().await;
    let findings = report
        .warnings
        .into_iter()
        .map(|message| DoctorFinding { code: "warning".to_string(), message, repair: None })
        .chain(report.repairs_required.into_iter().map(|message| DoctorFinding {
            code: "repair_required".to_string(),
            message,
            repair: Some("Run substrate repair before relying on daemon recall.".to_string()),
        }))
        .collect::<Vec<_>>();
    DoctorResponse {
        healthy: findings.is_empty(),
        findings,
        guidance: "Doctor reflects Stream A substrate validation and repair state.".to_string(),
    }
}

async fn search_response(
    substrate: &Substrate,
    query: &str,
    limit: Option<usize>,
    include_body: bool,
) -> Result<ResponsePayload, HandlerError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(HandlerError::invalid_request("search query must not be empty"));
    }

    let limit = limit.unwrap_or(SEARCH_LIMIT_DEFAULT).min(SEARCH_LIMIT_MAX);
    let chunks = substrate
        .query_chunks(ChunkQuery { text: Some(query.to_string()), triple: None, vector: None })
        .await
        .map_err(HandlerError::substrate)?;
    let total = chunks.len();
    let hits = chunks
        .into_iter()
        .take(limit)
        .map(|chunk| SearchHit {
            id: chunk.memory_id.as_str().to_string(),
            summary: bounded(&chunk.text, SEARCH_SNIPPET_MAX),
            snippet: bounded(&chunk.text, SEARCH_SNIPPET_MAX),
            score: chunk.score,
        })
        .collect();

    let guidance = if include_body {
        "Search returns bounded matching chunks; call memory_get for the bounded record preview.".to_string()
    } else {
        "Bounded snippets only; call memory_get for full body access when policy allows.".to_string()
    };
    Ok(ResponsePayload::Search(SearchResponse { hits, total, guidance }))
}

async fn get_response(
    substrate: &Substrate,
    id: &str,
    _include_provenance: bool,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let body = match envelope.content {
        MemoryContent::Plaintext(body) => body,
        MemoryContent::MetadataOnly => String::new(),
        MemoryContent::Ciphertext { .. } => "[encrypted content omitted]".to_string(),
    };
    let (body, truncated) = bounded_with_truncation(&body, GET_BODY_MAX);
    Ok(ResponsePayload::Get(GetResponse {
        id: envelope.metadata.frontmatter.id.as_str().to_string(),
        summary: envelope.metadata.frontmatter.summary,
        body,
        truncated,
        guidance: "Returned a bounded Stream A record preview.".to_string(),
    }))
}

async fn write_note_response(substrate: &Substrate, text: &str) -> Result<ResponsePayload, HandlerError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(HandlerError::invalid_request("note text must not be empty"));
    }

    let memory_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let memory = candidate_memory(memory_id, text);
    let id = memory.frontmatter.id.as_str().to_string();
    let summary = memory.frontmatter.summary.clone();
    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(HandlerError::substrate)?;
    Ok(ResponsePayload::WriteNote(WriteNoteResponse { id, summary }))
}

async fn governance_write_response(
    substrate: &Substrate,
    request: GovernanceWriteRequest,
) -> Result<ResponsePayload, HandlerError> {
    let input = GovernanceWriteInput::parse(request.body, request.title, request.tags, request.meta)?;
    if let Some(response) = input.privacy_refusal() {
        return Ok(ResponsePayload::GovernanceWrite(response));
    }

    let id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let candidate = input.candidate(id.as_str());
    let (policies, policy_source) = match load_policy_set(substrate.roots().repo.as_path()) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceWrite(policy_refusal(input.response_namespace(), error.message)))
        }
    };
    let tombstones = match load_tombstone_index(substrate.roots().repo.as_path()) {
        Ok(index) => index,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceWrite(tombstone_refusal(
                input.response_namespace(),
                error.message,
                policy_source,
            )));
        }
    };
    let active = active_memory_summaries(substrate).await?;
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode: TiebreakMode::Unclear,
        allow_top_k: false,
    });
    let decision = engine.evaluate_write(&candidate);
    let response = execute_write_decision(substrate, WriteExecution { input, id, decision, policy_source }).await?;
    Ok(ResponsePayload::GovernanceWrite(response))
}

async fn governance_supersede_response(
    substrate: &Substrate,
    request: GovernanceSupersedeRequest,
) -> Result<ResponsePayload, HandlerError> {
    let GovernanceSupersedeRequest { old_id, content, reason, meta } = request;
    let old_memory_id =
        MemoryId::try_new(old_id.clone()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let input = GovernanceWriteInput::parse(content, None, Vec::new(), meta)?;
    if let Some(refusal) = input.privacy_refusal() {
        return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
            status: GovernanceStatus::Refused,
            new_id: None,
            old_id: Some(old_id),
            reason: refusal.reason,
            chain: None,
            policy_applied: refusal.policy_applied,
            policy_source: refusal.policy_source,
        }));
    }

    let new_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let candidate = input.candidate(new_id.as_str());
    let (policies, policy_source) = match load_policy_set(substrate.roots().repo.as_path()) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
                status: GovernanceStatus::Refused,
                new_id: None,
                old_id: Some(old_id),
                reason: Some(GovernanceRefusalReason::Policy),
                chain: None,
                policy_applied: None,
                policy_source: Some(error.message),
            }));
        }
    };
    let tombstones = match load_tombstone_index(substrate.roots().repo.as_path()) {
        Ok(index) => index,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
                status: GovernanceStatus::Refused,
                new_id: None,
                old_id: Some(old_id),
                reason: Some(GovernanceRefusalReason::Tombstone),
                chain: None,
                policy_applied: None,
                policy_source: Some(error.message),
            }));
        }
    };
    let old_memory = substrate.read_memory(&old_memory_id).await.map_err(HandlerError::substrate)?;
    let active = vec![existing_summary_from_memory(old_memory)];
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode: TiebreakMode::Contradiction { existing_id: old_id.clone() },
        allow_top_k: true,
    });
    let decision = engine.evaluate_write(&candidate);
    let GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. } = decision else {
        return Ok(ResponsePayload::GovernanceSupersede(supersede_refusal(old_id, decision, policy_source)));
    };
    if existing_id != old_id {
        return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
            status: GovernanceStatus::Refused,
            new_id: None,
            old_id: Some(old_id),
            reason: Some(GovernanceRefusalReason::Contradiction),
            chain: None,
            policy_applied: Some(policy_applied),
            policy_source: Some(policy_source_string(policy_source)),
        }));
    }

    let mut replacement = input.to_memory(
        new_id.clone(),
        GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
    );
    replacement.frontmatter.supersedes.push(old_memory_id.clone());
    substrate
        .supersede_memory(SubstrateSupersedeRequest {
            old_id: old_memory_id,
            replacement,
            reason,
            classification: ClassificationOutcome::Trusted,
            allow_best_effort_durability: true,
        })
        .await
        .map_err(HandlerError::substrate)?;

    Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
        status: GovernanceStatus::Promoted,
        new_id: Some(new_id.as_str().to_string()),
        old_id: Some(old_id.clone()),
        reason: None,
        chain: Some(serde_json::json!({ "supersedes": [old_id] })),
        policy_applied: Some(policy_applied),
        policy_source: Some(policy_source_string(policy_source)),
    }))
}

async fn governance_forget_response(
    substrate: &Substrate,
    id: String,
    reason: String,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.clone()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let memory = substrate.read_memory(&memory_id).await.map_err(HandlerError::substrate)?;
    substrate
        .tombstone_memory(TombstoneRequest { id: memory_id, reason: reason.clone() })
        .await
        .map_err(HandlerError::substrate)?;
    write_tombstone_rule(substrate.roots().repo.as_path(), &memory, &reason)?;
    Ok(ResponsePayload::GovernanceForget(GovernanceForgetResponse {
        status: GovernanceStatus::Tombstoned,
        id,
        tombstone_ref: Some("tombstone:stream-a".to_string()),
        reason: None,
    }))
}

async fn execute_write_decision(
    substrate: &Substrate,
    execution: WriteExecution,
) -> Result<GovernanceWriteResponse, HandlerError> {
    let WriteExecution { input, id, decision, policy_source } = execution;
    match decision {
        GovernanceWriteDecision::Promoted { namespace, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
            );
            write_governed_memory(substrate, memory).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Promoted,
                id: Some(id.as_str().to_string()),
                namespace: Some(namespace),
                reason: None,
                next_actions: Vec::new(),
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
            })
        }
        GovernanceWriteDecision::Candidate { reason, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Candidate, TrustLevel::Candidate, policy_applied.clone()),
            );
            write_governed_memory(substrate, memory).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Candidate,
                id: Some(id.as_str().to_string()),
                namespace: Some(input.response_namespace()),
                reason: None,
                next_actions: vec![reason],
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
            })
        }
        GovernanceWriteDecision::Quarantined { reason, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Quarantined, TrustLevel::Quarantined, policy_applied.clone()),
            );
            write_governed_memory(substrate, memory).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Quarantined,
                id: Some(id.as_str().to_string()),
                namespace: Some(input.response_namespace()),
                reason: None,
                next_actions: vec![reason],
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
            })
        }
        GovernanceWriteDecision::Duplicate { existing_id, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some(existing_id.clone()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: Vec::new(),
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
        }),
        GovernanceWriteDecision::Refinement { existing_id, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some(existing_id.clone()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["merge_evidence".to_string()],
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
        }),
        GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Candidate,
            id: Some(id.as_str().to_string()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["memory_supersede".to_string()],
            policy_applied: Some(policy_applied),
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
        }),
        GovernanceWriteDecision::Refused { reason, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Refused,
            id: None,
            namespace: Some(input.response_namespace()),
            reason: Some(reason),
            next_actions: Vec::new(),
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: None,
        }),
    }
}

async fn write_governed_memory(substrate: &Substrate, memory: Memory) -> Result<(), HandlerError> {
    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-governance".to_string()),
                reason: Some("governed write".to_string()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map(|_| ())
        .map_err(HandlerError::substrate)
}

async fn review_queue_response(substrate: &Substrate, limit: Option<usize>) -> Result<ResponsePayload, HandlerError> {
    let mut envelopes = Vec::new();
    for path in memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path()) {
        let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
        let envelope = substrate.read_path_envelope(&repo_path).await.map_err(HandlerError::substrate)?;
        envelopes.push(review_envelope_from_memory(envelope.metadata));
    }

    let mut queue = ReviewQueue::from_memory_envelopes(envelopes);
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

async fn review_decision_response(
    substrate: &Substrate,
    id: &str,
    decision: ReviewDecision,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let mut memory = substrate.read_memory(&memory_id).await.map_err(HandlerError::substrate)?;
    if !matches!(memory.frontmatter.status, MemoryStatus::Candidate | MemoryStatus::Quarantined)
        || !review_queue_contains(&memory)
    {
        return Err(HandlerError::invalid_request("memory is not eligible for the review queue"));
    }
    if matches!((&decision, memory.frontmatter.status), (ReviewDecision::Approve, MemoryStatus::Quarantined)) {
        return Err(HandlerError::invalid_request("quarantined memories must be resubmitted through governance"));
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

fn review_envelope_from_memory(memory: Memory) -> ReviewMemoryEnvelope {
    ReviewMemoryEnvelope {
        id: memory.frontmatter.id.as_str().to_string(),
        summary: memory.frontmatter.summary,
        status: serde_json::to_value(memory.frontmatter.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "active".to_string()),
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

fn serialized_payload_len(payload: &ResponsePayload) -> usize {
    serde_json::to_vec(payload).map_or(MAX_FRAME_BYTES, |bytes| bytes.len())
}

fn load_policy_set(repo: &Path) -> Result<(PolicySet, PolicySource), HandlerError> {
    let policy_dir = repo.join("policies");
    let has_yaml = std::fs::read_dir(&policy_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|extension| extension == "yaml"));

    if has_yaml {
        match PolicySet::load_from_dir(&policy_dir) {
            Ok(policies) => return Ok((policies, PolicySource::Disk)),
            Err(error) => return Err(HandlerError::invalid_request(format!("invalid governance policy: {error}"))),
        }
    }

    Ok((PolicySet::builtin(), PolicySource::BuiltInFallback))
}

fn load_tombstone_index(repo: &Path) -> Result<TombstoneIndex, HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    if !tombstone_dir.exists() {
        return Ok(TombstoneIndex::default());
    }
    TombstoneIndex::load_jsonl_dir(&tombstone_dir)
        .map_err(|error| HandlerError::invalid_request(format!("invalid tombstone rules: {error}")))
}

fn policy_refusal(namespace: String, message: String) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status: GovernanceStatus::Refused,
        id: None,
        namespace: Some(namespace),
        reason: Some(GovernanceRefusalReason::Policy),
        next_actions: vec![message],
        policy_applied: None,
        policy_source: None,
        existing_id: None,
    }
}

fn tombstone_refusal(namespace: String, message: String, policy_source: PolicySource) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status: GovernanceStatus::Refused,
        id: None,
        namespace: Some(namespace),
        reason: Some(GovernanceRefusalReason::Tombstone),
        next_actions: vec![message],
        policy_applied: None,
        policy_source: Some(policy_source_string(policy_source)),
        existing_id: None,
    }
}

fn supersede_refusal(
    old_id: String,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
) -> GovernanceSupersedeResponse {
    let (reason, policy_applied) = match decision {
        GovernanceWriteDecision::Refused { reason, .. } => (reason, None),
        GovernanceWriteDecision::Duplicate { .. } => (GovernanceRefusalReason::Superseded, None),
        GovernanceWriteDecision::Refinement { .. } => (GovernanceRefusalReason::Contradiction, None),
        GovernanceWriteDecision::Candidate { policy_applied, .. }
        | GovernanceWriteDecision::Quarantined { policy_applied, .. }
        | GovernanceWriteDecision::Promoted { policy_applied, .. } => {
            (GovernanceRefusalReason::Contradiction, Some(policy_applied))
        }
        GovernanceWriteDecision::Supersession { policy_applied, .. } => {
            (GovernanceRefusalReason::Contradiction, Some(policy_applied))
        }
    };
    GovernanceSupersedeResponse {
        status: GovernanceStatus::Refused,
        new_id: None,
        old_id: Some(old_id),
        reason: Some(reason),
        chain: None,
        policy_applied,
        policy_source: Some(policy_source_string(policy_source)),
    }
}

fn existing_summary_from_memory(memory: Memory) -> ExistingMemorySummary {
    ExistingMemorySummary::new(
        memory.frontmatter.id.as_str().to_string(),
        namespace_for_frontmatter(&memory.frontmatter),
        memory.body,
        1.0,
    )
    .with_entity_ids(entity_ids(&memory.frontmatter))
}

fn write_tombstone_rule(repo: &Path, memory: &Memory, reason: &str) -> Result<(), HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    std::fs::create_dir_all(&tombstone_dir)
        .map_err(|error| HandlerError::substrate(format!("create tombstone dir: {error}")))?;
    let key = memory_governance::CandidateTombstoneKey::from_claim(&memory.body, entity_ids(&memory.frontmatter))
        .with_target_memory_id(memory.frontmatter.id.as_str().to_string());
    let rule = TombstoneRule {
        id: format!("tomb_{}", memory.frontmatter.id.as_str()),
        target_memory_id: Some(memory.frontmatter.id.as_str().to_string()),
        content_hash: key.content_hash,
        entity_hash: key.entity_hash,
        reason: TombstoneKind::UserForget,
        reason_text: Some(reason.to_string()),
        active: true,
    };
    let path = tombstone_dir.join("memoryd-forget.jsonl");
    let mut file =
        OpenOptions::new().create(true).append(true).open(&path).map_err(|error| {
            HandlerError::substrate(format!("open tombstone rule file {}: {error}", path.display()))
        })?;
    let line = serde_json::to_string(&rule)
        .map_err(|error| HandlerError::substrate(format!("serialize tombstone rule: {error}")))?;
    writeln!(file, "{line}")
        .map_err(|error| HandlerError::substrate(format!("append tombstone rule file {}: {error}", path.display())))?;
    Ok(())
}

struct GovernanceEngineInput {
    policies: PolicySet,
    active: Vec<ExistingMemorySummary>,
    tombstones: TombstoneIndex,
    tiebreak_mode: TiebreakMode,
    allow_top_k: bool,
}

fn governance_engine(
    input: GovernanceEngineInput,
) -> GovernanceEngine<MemorydSimilaritySearch, MemorydTiebreaker, MemorydSessionResolver> {
    GovernanceEngine::new(
        input.policies,
        GroundingVerifier::new(FileSourceResolver, MemorydSessionResolver),
        input.tombstones,
        GovernanceProviders::new(
            MemorydSimilaritySearch { active: input.active, allow_top_k: input.allow_top_k },
            MemorydTiebreaker { tiebreak_mode: input.tiebreak_mode },
        ),
    )
}

async fn active_memory_summaries(substrate: &Substrate) -> Result<Vec<ExistingMemorySummary>, HandlerError> {
    let mut summaries = Vec::new();
    for path in memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path()) {
        let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
        let envelope = substrate.read_path_envelope(&repo_path).await.map_err(HandlerError::substrate)?;
        if !matches!(envelope.metadata.frontmatter.status, MemoryStatus::Active) {
            continue;
        }
        let MemoryContent::Plaintext(body) = envelope.content else {
            continue;
        };
        summaries.push(
            ExistingMemorySummary::new(
                envelope.metadata.frontmatter.id.as_str().to_string(),
                namespace_for_frontmatter(&envelope.metadata.frontmatter),
                body,
                1.0,
            )
            .with_entity_ids(entity_ids(&envelope.metadata.frontmatter)),
        );
    }
    Ok(summaries)
}

#[derive(Clone, Debug)]
struct MemorydSimilaritySearch {
    active: Vec<ExistingMemorySummary>,
    allow_top_k: bool,
}

impl SimilaritySearch for MemorydSimilaritySearch {
    fn find_active_by_claim_hash(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        self.active
            .iter()
            .find(|memory| {
                memory.canonical_claim_hash() == candidate.canonical_claim_hash()
                    && memory.entity_hash() == candidate.entity_hash()
                    && memory.namespace() == candidate.namespace()
            })
            .cloned()
    }

    fn top_k(&self, _candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        if !self.allow_top_k {
            return Vec::new();
        }
        self.active.iter().take(limit).cloned().collect()
    }
}

#[derive(Clone, Debug)]
struct MemorydTiebreaker {
    tiebreak_mode: TiebreakMode,
}

#[derive(Clone, Debug)]
enum TiebreakMode {
    Unclear,
    Contradiction { existing_id: String },
}

impl ContradictionTiebreaker for MemorydTiebreaker {
    fn tiebreak(&self, _candidate: &CandidateMemory, _hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        match &self.tiebreak_mode {
            TiebreakMode::Unclear => TiebreakOutcome::Unclear,
            TiebreakMode::Contradiction { existing_id } => {
                TiebreakOutcome::Contradiction { existing_id: existing_id.clone() }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MemorydSessionResolver;

impl SessionSpawnResolver for MemorydSessionResolver {
    fn spawned_in_session(&self, _spawn_id: &str) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
struct GovernanceWriteRequest {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: Value,
}

#[derive(Clone, Debug)]
struct GovernanceSupersedeRequest {
    old_id: String,
    content: String,
    reason: String,
    meta: Value,
}

#[derive(Clone, Debug)]
struct WriteExecution {
    input: GovernanceWriteInput,
    id: MemoryId,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
}

#[derive(Clone, Debug)]
struct GovernedLifecycle {
    status: MemoryStatus,
    trust_level: TrustLevel,
    policy_applied: String,
}

impl GovernedLifecycle {
    fn new(status: MemoryStatus, trust_level: TrustLevel, policy_applied: String) -> Self {
        Self { status, trust_level, policy_applied }
    }
}

#[derive(Clone, Debug)]
struct GovernanceWriteInput {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: GovernanceMeta,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct GovernanceMeta {
    namespace: GovernanceNamespace,
    #[serde(rename = "type")]
    memory_type: GovernanceMemoryType,
    summary: Option<String>,
    confidence: f64,
    sensitivity: Option<GovernanceSensitivity>,
    source_kind: GovernanceSourceKindMeta,
    source_ref: Option<String>,
    explicit_user_context: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GovernanceNamespace {
    Me,
    Project,
    Agent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceMemoryType {
    Project,
    Claim,
    Decision,
    Pattern,
    Playbook,
    Procedure,
    Artifact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceSensitivity {
    Public,
    Internal,
    Confidential,
    Personal,
    Sensitive,
    Secret,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceSourceKindMeta {
    User,
    AgentPrimary,
    Subagent,
    File,
}

impl Default for GovernanceMeta {
    fn default() -> Self {
        Self {
            namespace: GovernanceNamespace::Project,
            memory_type: GovernanceMemoryType::Project,
            summary: None,
            confidence: 0.85,
            sensitivity: None,
            source_kind: GovernanceSourceKindMeta::User,
            source_ref: None,
            explicit_user_context: false,
        }
    }
}

impl Default for GovernanceNamespace {
    fn default() -> Self {
        Self::Project
    }
}

impl<'de> Deserialize<'de> for GovernanceNamespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "me" | "user" => Ok(Self::Me),
            "project" => Ok(Self::Project),
            "agent" => Ok(Self::Agent),
            other => Err(serde::de::Error::custom(format!("unsupported namespace `{other}`"))),
        }
    }
}

impl GovernanceWriteInput {
    fn parse(body: String, title: Option<String>, tags: Vec<String>, meta: Value) -> Result<Self, HandlerError> {
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err(HandlerError::invalid_request("memory body must not be empty"));
        }
        let meta = if meta.is_null() {
            GovernanceMeta::default()
        } else {
            serde_json::from_value(meta).map_err(|err| HandlerError::invalid_request(err.to_string()))?
        };
        if !meta.confidence.is_finite() || !(0.0..=1.0).contains(&meta.confidence) {
            return Err(HandlerError::invalid_request("confidence must be finite and between 0.0 and 1.0"));
        }
        Ok(Self { body, title, tags, meta })
    }

    fn privacy_refusal(&self) -> Option<GovernanceWriteResponse> {
        match self.meta.sensitivity {
            Some(GovernanceSensitivity::Public | GovernanceSensitivity::Internal) => None,
            None
            | Some(
                GovernanceSensitivity::Confidential
                | GovernanceSensitivity::Personal
                | GovernanceSensitivity::Sensitive
                | GovernanceSensitivity::Secret,
            ) => Some(GovernanceWriteResponse {
                status: GovernanceStatus::Refused,
                id: None,
                namespace: Some(self.response_namespace()),
                reason: Some(GovernanceRefusalReason::Privacy),
                next_actions: vec!["run_stream_d_privacy_classification".to_string()],
                policy_applied: None,
                policy_source: None,
                existing_id: None,
            }),
        }
    }

    fn candidate(&self, id: &str) -> CandidateMemory {
        let mut candidate =
            CandidateMemory::new(id, self.response_namespace(), self.body.clone(), self.governance_scope())
                .with_confidence(self.meta.confidence as f32)
                .with_sources(self.governance_sources());
        if self.meta.explicit_user_context {
            candidate = candidate.with_explicit_user_context();
        }
        candidate
    }

    fn to_memory(&self, id: MemoryId, lifecycle: GovernedLifecycle) -> Memory {
        let now = chrono::Utc::now();
        let summary = self.summary();
        let requires_review = matches!(lifecycle.status, MemoryStatus::Candidate | MemoryStatus::Quarantined);
        let review_state = match lifecycle.status {
            MemoryStatus::Candidate => Some("candidate".to_string()),
            MemoryStatus::Quarantined => Some("quarantined".to_string()),
            _ => None,
        };
        let mut extras = BTreeMap::new();
        if matches!(lifecycle.status, MemoryStatus::Quarantined) {
            extras.insert("governance_reason".to_string(), serde_json::json!("governance quarantine"));
        }

        Memory {
            frontmatter: Frontmatter {
                schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
                id: id.clone(),
                memory_type: self.memory_type(),
                scope: self.substrate_scope(),
                summary,
                confidence: self.meta.confidence,
                trust_level: lifecycle.trust_level,
                sensitivity: Sensitivity::Internal,
                status: lifecycle.status,
                created_at: now,
                updated_at: now,
                author: self.author(),
                namespace: self.substrate_namespace(),
                canonical_namespace_id: self.substrate_namespace(),
                tags: self.tags.clone(),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: self.substrate_source(),
                evidence: Vec::new(),
                requires_user_confirmation: requires_review,
                review_state,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: !matches!(lifecycle.status, MemoryStatus::Quarantined),
                    max_scope: self.substrate_scope(),
                    mask_personal_for_synthesis: false,
                    index_body: !matches!(lifecycle.status, MemoryStatus::Quarantined),
                    index_embeddings: !matches!(lifecycle.status, MemoryStatus::Quarantined),
                },
                write_policy: WritePolicy {
                    human_review_required: requires_review,
                    policy_applied: lifecycle.policy_applied,
                    expected_base_hash: None,
                },
                merge_diagnostics: matches!(lifecycle.status, MemoryStatus::Quarantined).then(|| {
                    serde_json::json!({
                        "human_reason": "governance quarantine",
                        "preserved_sources": [],
                        "lifecycle_notes": [],
                        "evidence_near_duplicates": []
                    })
                }),
                extras,
            },
            body: self.body.clone(),
            path: Some(self.repo_path(id.as_str())),
        }
    }

    fn summary(&self) -> String {
        self.meta.summary.clone().or_else(|| self.title.clone()).unwrap_or_else(|| bounded(&self.body, 120))
    }

    fn response_namespace(&self) -> String {
        match self.meta.namespace {
            GovernanceNamespace::Me => "me".to_string(),
            GovernanceNamespace::Project => "project".to_string(),
            GovernanceNamespace::Agent => "agent".to_string(),
        }
    }

    fn governance_scope(&self) -> memory_governance::Scope {
        match self.meta.namespace {
            GovernanceNamespace::Me => memory_governance::Scope::Me,
            GovernanceNamespace::Project => memory_governance::Scope::Project,
            GovernanceNamespace::Agent => memory_governance::Scope::Agent,
        }
    }

    fn substrate_scope(&self) -> Scope {
        match self.meta.namespace {
            GovernanceNamespace::Me => Scope::User,
            GovernanceNamespace::Project => Scope::Project,
            GovernanceNamespace::Agent => Scope::Agent,
        }
    }

    fn substrate_namespace(&self) -> Option<String> {
        matches!(self.meta.namespace, GovernanceNamespace::Project).then(|| DEFAULT_PROJECT_NAMESPACE.to_string())
    }

    fn governance_sources(&self) -> Vec<GovernanceSource> {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => GovernanceSourceKind::User,
            GovernanceSourceKindMeta::Subagent => GovernanceSourceKind::Subagent,
            GovernanceSourceKindMeta::AgentPrimary | GovernanceSourceKindMeta::File => {
                GovernanceSourceKind::AgentPrimary
            }
        };
        vec![GovernanceSource::new(kind, self.meta.source_ref.clone())]
    }

    fn substrate_source(&self) -> Source {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => SourceKind::User,
            GovernanceSourceKindMeta::Subagent => SourceKind::AgentSubagent,
            GovernanceSourceKindMeta::File => SourceKind::File,
            GovernanceSourceKindMeta::AgentPrimary => SourceKind::AgentPrimary,
        };
        Source {
            kind,
            reference: self.meta.source_ref.clone().or_else(|| Some("memoryd.governance".to_string())),
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        }
    }

    fn author(&self) -> Author {
        match self.meta.source_kind {
            GovernanceSourceKindMeta::User => Author {
                kind: AuthorKind::User,
                user_handle: Some("memoryd-user".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::Subagent => Author {
                kind: AuthorKind::Subagent,
                user_handle: None,
                harness: Some("memoryd".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: Some("memoryd-subagent".to_string()),
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::AgentPrimary | GovernanceSourceKindMeta::File => Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("memoryd".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: None,
                phase: None,
                component: None,
            },
        }
    }

    fn memory_type(&self) -> MemoryType {
        match self.meta.memory_type {
            GovernanceMemoryType::Claim => MemoryType::Claim,
            GovernanceMemoryType::Decision => MemoryType::Decision,
            GovernanceMemoryType::Pattern => MemoryType::Pattern,
            GovernanceMemoryType::Playbook => MemoryType::Playbook,
            GovernanceMemoryType::Procedure => MemoryType::Procedure,
            GovernanceMemoryType::Artifact => MemoryType::Artifact,
            GovernanceMemoryType::Project => MemoryType::Project,
        }
    }

    fn repo_path(&self, id: &str) -> RepoPath {
        match self.meta.namespace {
            GovernanceNamespace::Me => RepoPath::new(format!("me/knowledge/{id}.md")),
            GovernanceNamespace::Project => {
                RepoPath::new(format!("projects/{DEFAULT_PROJECT_NAMESPACE}/decisions/{id}.md"))
            }
            GovernanceNamespace::Agent => RepoPath::new(format!("agent/patterns/{id}.md")),
        }
    }
}

fn policy_source_string(source: PolicySource) -> String {
    match source {
        PolicySource::Disk => "disk".to_string(),
        PolicySource::BuiltInFallback => "built_in_fallback".to_string(),
    }
}

fn namespace_for_frontmatter(frontmatter: &Frontmatter) -> String {
    match frontmatter.scope {
        Scope::Project => "project".to_string(),
        Scope::Agent | Scope::Subagent => "agent".to_string(),
        Scope::User => "me".to_string(),
        Scope::Org => "project".to_string(),
    }
}

fn entity_ids(frontmatter: &Frontmatter) -> Vec<String> {
    frontmatter.entities.iter().map(|entity| entity.id.clone()).collect()
}

enum ReviewDecision {
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

fn candidate_memory(id: MemoryId, text: &str) -> Memory {
    let now = chrono::Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: bounded(text, 120),
            confidence: 0.5,
            trust_level: TrustLevel::Candidate,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Candidate,
            created_at: now,
            updated_at: now,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("memoryd".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: vec!["candidate".to_string(), "memoryd-note".to_string()],
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: Some("memoryd.write_note".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: true,
            review_state: Some("candidate".to_string()),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: true,
                policy_applied: "memoryd-candidate-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: BTreeMap::new(),
        },
        body: text.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{}.md", id.as_str()))),
    }
}

fn bounded(text: &str, max_chars: usize) -> String {
    bounded_with_truncation(text, max_chars).0
}

fn bounded_with_truncation(text: &str, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let bounded: String = chars.by_ref().take(max_chars).collect();
    let truncated = chars.next().is_some();
    (bounded, truncated)
}

#[derive(Debug)]
struct HandlerError {
    code: String,
    message: String,
    retryable: bool,
}

impl HandlerError {
    fn invalid_request(message: impl Into<String>) -> Self {
        Self { code: "invalid_request".to_string(), message: message.into(), retryable: false }
    }

    fn substrate(error: impl std::fmt::Display) -> Self {
        Self { code: "substrate_error".to_string(), message: error.to_string(), retryable: true }
    }
}
