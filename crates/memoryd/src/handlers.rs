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
use memory_privacy::{
    safe_plaintext_fragment, CallerSensitivity, DeterministicPrivacyClassifier, EncryptedPayload, FileKeyProvider,
    PrivacyClassifier, PrivacyDecision, PrivacyEncryptor, PrivacyNamespace, PrivacyStorageAction, SafeFragmentDecision,
};
use memory_substrate::{
    Author, AuthorKind, ChunkQuery, ClassificationOutcome, EncryptedSubstrateDescriptor, EncryptedWriteRequest,
    EventContext, Frontmatter, IndexProjection, Memory, MemoryContent, MemoryId, MemoryStatus, MemoryType, ObserveKind,
    PrivacySpanRecord, RepoPath, RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, Substrate,
    SubstrateFragmentAppendRequest, SubstrateFragmentEncryption, SubstrateFragmentPayload,
    SupersedeRequest as SubstrateSupersedeRequest, TombstoneRequest, TrustLevel, WriteMode, WritePolicy,
    WriteRequest as SubstrateWriteRequest,
};
use serde::Deserialize;
use serde_json::Value;

use crate::dream::rehydration;
use crate::protocol::{
    DoctorFinding, DoctorResponse, GetResponse, GovernanceForgetResponse, GovernanceStatus,
    GovernanceSupersedeResponse, GovernanceWriteResponse, ObserveResponse, ObserveTarget, RequestEnvelope,
    RequestPayload, ResponseEnvelope, ResponsePayload, RevealResponse, ReviewDecisionResponse, ReviewQueueItemResponse,
    ReviewQueueResponse, SearchHit, SearchResponse, StatusResponse, WriteNoteResponse, MAX_FRAME_BYTES,
};
use crate::recall::{
    build_delta_response, build_startup_response, OmissionReason, RecallError, SharedRecallCounters, StartupResponse,
};

const SEARCH_LIMIT_DEFAULT: usize = 10;
const SEARCH_LIMIT_MAX: usize = 20;
const SEARCH_SNIPPET_MAX: usize = 240;
const GET_BODY_MAX: usize = 4_096;
const OBSERVE_TEXT_MAX_BYTES: usize = 16 * 1024;
const OBSERVE_ENTITIES_MAX: usize = 32;
const OBSERVE_ENTITY_MAX_BYTES: usize = 128;
const OBSERVE_ENTITY_BODY_MAX_BYTES: usize = 124;
const OBSERVE_BINDING_FIELD_MAX_BYTES: usize = 128;
const REVIEW_QUEUE_LIMIT_DEFAULT: usize = 50;
const REVIEW_QUEUE_LIMIT_MAX: usize = 100;
const REVIEW_QUEUE_SUMMARY_MAX: usize = 512;
const REVIEW_QUEUE_POLICY_MAX: usize = 128;
const REVIEW_QUEUE_REASON_MAX: usize = 512;
const REVIEW_QUEUE_ACTION_MAX: usize = 96;
const REVIEW_DECISION_SUMMARY_MAX: usize = 512;
const REVIEW_RESPONSE_FRAME_BUDGET: usize = MAX_FRAME_BYTES - 1024;
const DEFAULT_PROJECT_NAMESPACE: &str = "agent-memory";

#[derive(Debug, Clone, Default)]
pub struct HandlerState {
    recall: SharedRecallCounters,
}

impl HandlerState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub async fn handle_request(substrate: &Substrate, envelope: RequestEnvelope) -> ResponseEnvelope {
    handle_request_with_state(substrate, &HandlerState::new(), envelope).await
}

pub async fn handle_request_with_state(
    substrate: &Substrate,
    state: &HandlerState,
    envelope: RequestEnvelope,
) -> ResponseEnvelope {
    let id = envelope.id;
    match dispatch(substrate, state, envelope.request).await {
        Ok(payload) => ResponseEnvelope::success(id, payload),
        Err(error) => ResponseEnvelope::error(id, error.code, error.message, error.retryable),
    }
}

async fn dispatch(
    substrate: &Substrate,
    state: &HandlerState,
    request: RequestPayload,
) -> Result<ResponsePayload, HandlerError> {
    match request {
        RequestPayload::Status => Ok(ResponsePayload::Status(status_response(state))),
        RequestPayload::Doctor => Ok(ResponsePayload::Doctor(doctor_response(substrate).await)),
        RequestPayload::Search { query, limit, include_body } => {
            search_response(substrate, &query, limit, include_body).await
        }
        RequestPayload::Get { id, include_provenance } => get_response(substrate, &id, include_provenance).await,
        RequestPayload::Reveal { id, reason } => reveal_response(substrate, &id, &reason).await,
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
        RequestPayload::Startup(request) => startup_response(substrate, state, request).await,
        RequestPayload::Delta(request) => delta_response(substrate, state, request).await,
        RequestPayload::Observe { text, kind, entities, cwd, session_id, harness, harness_version } => {
            observe_response(
                substrate,
                ObserveRequestFields { text, kind, entities, cwd, session_id, harness, harness_version },
            )
            .await
        }
        RequestPayload::DreamNow { scope, force, cli_override } => {
            dream_now_response(substrate, scope, force, cli_override).await
        }
        RequestPayload::DreamStatus {} => dream_status_response(substrate).await,
    }
}

fn status_response(state: &HandlerState) -> StatusResponse {
    StatusResponse {
        state: "ready".to_string(),
        guidance: "memoryd handlers are backed by the Stream A substrate.".to_string(),
        recall: state.recall.snapshot(),
        dreams: Default::default(),
    }
}

async fn dream_status_response(substrate: &Substrate) -> Result<ResponsePayload, HandlerError> {
    crate::dream::status::build_dream_status_report(&substrate.roots().repo, &substrate.roots().runtime)
        .await
        .map(|report| ResponsePayload::DreamStatus(Box::new(report)))
        .map_err(HandlerError::substrate)
}

async fn dream_now_response(
    substrate: &Substrate,
    scope: String,
    force: bool,
    cli_override: Option<String>,
) -> Result<ResponsePayload, HandlerError> {
    let config = memory_substrate::config::load_config(&substrate.roots().repo, &substrate.roots().runtime, None)
        .map_err(HandlerError::invalid_request)?;
    if !config.synced.dreams.enabled
        || crate::dream::status::disabled_sentinel_path(&substrate.roots().runtime).exists()
    {
        return Err(HandlerError::dream_disabled("dreaming is disabled on this device"));
    }
    let scope = crate::dream::scope::DreamScope::parse(&scope).map_err(HandlerError::from_dream)?;
    validate_dream_cli_override(cli_override.as_deref())?;
    let now = chrono::Utc::now();
    let acquired = crate::dream::lease::acquire_manual_lease(crate::dream::lease::LeaseAcquireRequest {
        repo: substrate.roots().repo.clone(),
        runtime: substrate.roots().runtime.clone(),
        scope: scope.as_str(),
        force,
        now,
        lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
        cli_used: cli_override.clone(),
    })
    .map_err(HandlerError::from_lease)?;

    let result = async {
        let build = crate::dream::orchestration::build_dream_run(
            substrate,
            crate::dream::orchestration::DreamRunBuildRequest {
                scope: scope.clone(),
                run_id: acquired.record.run_id,
                run_date: now.date_naive(),
                pass_timeout: std::time::Duration::from_secs(u64::from(config.synced.dreams.per_pass_timeout_seconds)),
                pass_2_max_candidates: config.synced.dreams.pass_2_max_candidates as usize,
                pass_1_window_days: config.synced.dreams.pass_1_window_days,
            },
        )
        .await
        .map_err(HandlerError::from_dream)?;
        let harness = crate::dream::orchestration::select_harness(
            cli_override.as_deref(),
            &config.synced.dreams.default_cli_priority,
            &build.options,
        )
        .await
        .map_err(dream_error_to_handler)?;
        crate::dream::run::DreamRunner::new(build.options.with_harness(harness), build.writer)
            .run()
            .await
            .map(|report| ResponsePayload::DreamNow(Box::new(report)))
            .map_err(HandlerError::from_dream)
    }
    .await;

    if result.is_err() {
        let _ = crate::dream::lease::release_manual_lease(crate::dream::lease::LeaseAcquireRequest {
            repo: substrate.roots().repo.clone(),
            runtime: substrate.roots().runtime.clone(),
            scope: scope.as_str(),
            force: false,
            now: chrono::Utc::now(),
            lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
            cli_used: cli_override,
        });
    }

    result
}

fn dream_error_to_handler(error: crate::dream::types::DreamError) -> HandlerError {
    let message = error.to_string();
    if let Some(rest) = message.strip_prefix("invalid_request: dream_unavailable: ") {
        HandlerError::dream_unavailable(rest.to_string())
    } else {
        HandlerError::from_dream(error)
    }
}

fn validate_dream_cli_override(cli_override: Option<&str>) -> Result<(), HandlerError> {
    let Some(name) = cli_override else {
        return Ok(());
    };
    if name == "echo" {
        return Ok(());
    }
    let registry = crate::dream::registry::HarnessCliRegistry::builtin_v0_2();
    if registry.get(name).is_some() || registry.disabled_adapters().any(|adapter| adapter.name == name) {
        Ok(())
    } else {
        Err(HandlerError::invalid_request(format!("unknown harness CLI override `{name}`")))
    }
}

async fn delta_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::DeltaRequest,
) -> Result<ResponsePayload, HandlerError> {
    match build_delta_response(substrate, request).await {
        Ok(response) => {
            state.recall.record_delta_success();
            Ok(ResponsePayload::Delta(response))
        }
        Err(error) => {
            state.recall.record_delta_failure(error.protocol_code());
            Err(HandlerError::from_recall(error))
        }
    }
}

async fn startup_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::StartupRequest,
) -> Result<ResponsePayload, HandlerError> {
    match build_startup_response(substrate, request).await {
        Ok(response) => {
            record_budget_exhaustions(state, &response);
            state.recall.record_dream_question_omissions(&response.dream_question_omissions);
            state.recall.record_startup_success();
            Ok(ResponsePayload::Startup(Box::new(response)))
        }
        Err(error) => {
            state.recall.record_startup_failure(error.protocol_code());
            Err(HandlerError::from_recall(error))
        }
    }
}

fn record_budget_exhaustions(state: &HandlerState, response: &StartupResponse) {
    for omission in &response.recall_explanation.omitted {
        if omission.reason == OmissionReason::BudgetExhausted {
            state.recall.record_budget_exhausted(omission.section.as_str());
        }
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

async fn reveal_response(substrate: &Substrate, id: &str, reason: &str) -> Result<ResponsePayload, HandlerError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(HandlerError::invalid_request("reveal reason must not be empty"));
    }
    if !is_safe_plaintext_for_indexing(reason) {
        return Err(HandlerError::invalid_request("reveal reason must not contain sensitive material"));
    }
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let MemoryContent::Ciphertext { bytes, .. } = envelope.content else {
        return Err(HandlerError::invalid_request("memory_reveal requires an encrypted memory"));
    };
    let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
    let body = encryptor
        .decrypt(&EncryptedPayload { ciphertext: bytes, envelope: serde_json::Value::Null })
        .map_err(HandlerError::privacy)?;
    substrate
        .record_encrypted_content_revealed(memory_id, bounded(reason, 240))
        .map_err(|err| HandlerError::substrate(format!("record encrypted reveal audit event: {err}")))?;
    let (body, truncated) = bounded_with_truncation(&body, GET_BODY_MAX);
    Ok(ResponsePayload::Reveal(RevealResponse {
        id: envelope.metadata.frontmatter.id.as_str().to_string(),
        summary: envelope.metadata.frontmatter.summary,
        body,
        truncated,
        guidance: "Returned decrypted content through explicit memory_reveal; plaintext was not re-indexed."
            .to_string(),
    }))
}

async fn write_note_response(substrate: &Substrate, text: &str) -> Result<ResponsePayload, HandlerError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(HandlerError::invalid_request("note text must not be empty"));
    }
    let privacy = classify_privacy(text, PrivacyNamespace::Agent, None)?;
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret note before disk effects"));
    }

    let memory_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let memory = candidate_memory(memory_id, text, privacy.storage_action);
    let id = memory.frontmatter.id.as_str().to_string();
    let summary = memory.frontmatter.summary.clone();
    write_privacy_memory(
        substrate,
        memory,
        &privacy,
        EventContext { actor: Some("memoryd-note".to_string()), reason: Some("privacy-mediated note".to_string()) },
    )
    .await?;
    Ok(ResponsePayload::WriteNote(WriteNoteResponse { id, summary }))
}

#[derive(Debug)]
struct ObserveRequestFields {
    text: String,
    kind: ObserveKind,
    entities: Vec<String>,
    cwd: String,
    session_id: String,
    harness: String,
    harness_version: Option<String>,
}

async fn observe_response(
    substrate: &Substrate,
    request: ObserveRequestFields,
) -> Result<ResponsePayload, HandlerError> {
    let text = validated_observe_text(request.text)?;
    let entities = validated_observe_entities(request.entities)?;
    let session_id = validated_observe_binding_field("session_id", request.session_id)?;
    let harness = validated_observe_binding_field("harness", request.harness)?;
    let harness_version = request
        .harness_version
        .map(|version| validated_observe_binding_field("harness_version", version))
        .transpose()?;
    let mut binding = crate::recall::binding::validate_session_fields(&request.cwd, &session_id, &harness)
        .await
        .map_err(HandlerError::from_recall)?;
    binding.harness_version = harness_version;
    let privacy = classify_privacy(&text, PrivacyNamespace::Agent, None)?;
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::privacy("secret refused before substrate fragment write"));
    }

    let kind = request.kind;
    let (payload, classification, target) = if privacy.storage_action.requires_encryption() {
        (
            encrypted_observe_payload(substrate, &text, kind)?,
            ClassificationOutcome::RequiresEncryption,
            ObserveTarget::EncryptedSubstrate,
        )
    } else {
        (
            SubstrateFragmentPayload::Plaintext { text },
            ClassificationOutcome::Trusted,
            ObserveTarget::PlaintextSubstrate,
        )
    };
    let outcome = substrate
        .append_substrate_fragment(SubstrateFragmentAppendRequest {
            id: None,
            at: chrono::Utc::now(),
            session: Some(binding.session_id.clone()),
            harness: Some(binding.harness.clone()),
            scope: observe_scope(&binding),
            entities,
            kind,
            source_ref: Some(observe_source_ref(&binding)),
            privacy_spans: privacy_span_records(&privacy),
            payload,
            classification,
            operation_id: None,
        })
        .await
        .map_err(HandlerError::substrate)?;

    Ok(ResponsePayload::Observe(ObserveResponse { fragment_id: outcome.id, target }))
}

fn validated_observe_text(text: String) -> Result<String, HandlerError> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(HandlerError::invalid_request("observe text must not be empty"));
    }
    if text.len() > OBSERVE_TEXT_MAX_BYTES {
        return Err(HandlerError::invalid_request("observe text exceeds 16 KiB"));
    }
    Ok(text)
}

fn validated_observe_entities(entities: Vec<String>) -> Result<Vec<String>, HandlerError> {
    if entities.len() > OBSERVE_ENTITIES_MAX {
        return Err(HandlerError::invalid_request("observe entities exceeds 32 entries"));
    }
    for entity in &entities {
        validate_observe_entity_id(entity)?;
    }
    Ok(entities)
}

fn validate_observe_entity_id(entity: &str) -> Result<(), HandlerError> {
    if entity.trim() != entity {
        return Err(HandlerError::invalid_request(
            "observe entity ids must not include leading or trailing whitespace",
        ));
    }
    if entity.len() > OBSERVE_ENTITY_MAX_BYTES {
        return Err(HandlerError::invalid_request("observe entity exceeds 128 UTF-8 bytes"));
    }
    let Some(body) = entity.strip_prefix("ent_") else {
        return Err(HandlerError::invalid_request("observe entities must be canonical ent_ ids"));
    };
    if body.is_empty() || body.len() > OBSERVE_ENTITY_BODY_MAX_BYTES {
        return Err(HandlerError::invalid_request("observe entities must be canonical ent_ ids"));
    }
    if !body.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')) {
        return Err(HandlerError::invalid_request("observe entities must be canonical ent_ ids"));
    }
    validate_observe_metadata_is_safe("observe entity", entity)?;
    Ok(())
}

fn validated_observe_binding_field(name: &str, value: String) -> Result<String, HandlerError> {
    if value.trim() != value {
        return Err(HandlerError::invalid_request(format!("{name} must not include leading or trailing whitespace")));
    }
    if value.is_empty() {
        return Err(HandlerError::invalid_request(format!("{name} must be non-empty")));
    }
    if value.len() > OBSERVE_BINDING_FIELD_MAX_BYTES {
        return Err(HandlerError::invalid_request(format!("{name} must be at most 128 bytes")));
    }
    if !value.bytes().all(is_observe_binding_byte) {
        return Err(HandlerError::invalid_request(format!("{name} must contain only safe id characters")));
    }
    validate_observe_metadata_is_safe(name, &value)?;
    Ok(value)
}

fn is_observe_binding_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')
}

fn validate_observe_metadata_is_safe(name: &str, value: &str) -> Result<(), HandlerError> {
    if !is_safe_plaintext_for_indexing(value) || contains_observe_metadata_canary(value) {
        return Err(HandlerError::invalid_request(format!("{name} must not contain sensitive material")));
    }
    Ok(())
}

fn contains_observe_metadata_canary(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value.contains('@')
        || contains_aws_access_key(value)
        || contains_us_phone_number(value)
        || contains_phone_like_digit_sequence(value)
        || lower.contains("ghp_")
        || lower.contains("sk_live_")
}

fn contains_aws_access_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(4).enumerate().any(|(index, window)| {
        window == b"AKIA"
            && bytes.get(index + 4..index + 20).is_some_and(|suffix| suffix.iter().all(u8::is_ascii_alphanumeric))
    })
}

fn contains_us_phone_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(12).any(|window| {
        window[0..3].iter().all(u8::is_ascii_digit)
            && window[3] == b'-'
            && window[4..7].iter().all(u8::is_ascii_digit)
            && window[7] == b'-'
            && window[8..12].iter().all(u8::is_ascii_digit)
    })
}

fn contains_phone_like_digit_sequence(value: &str) -> bool {
    let mut digit_count = 0usize;
    for byte in value.bytes() {
        if byte.is_ascii_digit() {
            digit_count += 1;
            if digit_count >= 10 {
                return true;
            }
            continue;
        }
        if matches!(byte, b'-' | b'.' | b'_' | b' ') {
            continue;
        }
        digit_count = 0;
    }
    false
}

fn observe_scope(binding: &crate::recall::SessionBinding) -> String {
    binding
        .project
        .as_ref()
        .map(|project| format!("project:{}", project.canonical_id))
        .unwrap_or_else(|| "agent".to_string())
}

fn observe_source_ref(binding: &crate::recall::SessionBinding) -> String {
    format!("session:{}:memory_observe", binding.session_id)
}

fn encrypted_observe_payload(
    substrate: &Substrate,
    text: &str,
    kind: ObserveKind,
) -> Result<SubstrateFragmentPayload, HandlerError> {
    let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
    let encrypted = encryptor.encrypt(text).map_err(HandlerError::privacy)?;
    Ok(SubstrateFragmentPayload::Encrypted {
        encryption: SubstrateFragmentEncryption {
            recipient: encrypted.envelope.get("recipient").and_then(Value::as_str).unwrap_or("age-x25519").to_string(),
            ciphertext_b64: base64_encode(&encrypted.ciphertext),
        },
        descriptor: encrypted_observe_descriptor(kind),
    })
}

fn encrypted_observe_descriptor(kind: ObserveKind) -> EncryptedSubstrateDescriptor {
    let tag = observe_kind_tag(kind);
    EncryptedSubstrateDescriptor {
        summary_safe: format!("encrypted {tag} substrate fragment"),
        tag_safe: vec![tag.to_string()],
    }
}

fn observe_kind_tag(kind: ObserveKind) -> &'static str {
    match kind {
        ObserveKind::Observation => "observation",
        ObserveKind::Pattern => "pattern",
        ObserveKind::Signal => "signal",
    }
}

fn privacy_span_records(privacy: &PrivacyDecision) -> Vec<PrivacySpanRecord> {
    privacy
        .spans
        .iter()
        .map(|span| PrivacySpanRecord {
            label: serde_json::to_value(span.label)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{:?}", span.label)),
            start: span.start,
            end: span.end,
        })
        .collect()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(b0 >> 2) as usize] as char);
        encoded.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

async fn governance_write_response(
    substrate: &Substrate,
    request: GovernanceWriteRequest,
) -> Result<ResponsePayload, HandlerError> {
    let input = GovernanceWriteInput::parse(request.body, request.title, request.tags, request.meta)?;
    let privacy = classify_input_privacy(&input)?;
    if let Some(response) = input.privacy_refusal(&privacy) {
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
    let response =
        execute_write_decision(substrate, WriteExecution { input, id, decision, policy_source, privacy }).await?;
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
    let privacy = classify_input_privacy(&input)?;
    if let Some(refusal) = input.privacy_refusal(&privacy) {
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
    let old_envelope = substrate.read_memory_envelope(&old_memory_id).await.map_err(HandlerError::substrate)?;
    let MemoryContent::Plaintext(old_body) = old_envelope.content else {
        return Ok(ResponsePayload::GovernanceSupersede(supersede_privacy_refusal(
            old_id,
            None,
            "encrypted memories cannot be superseded until Stream A exposes an encrypted supersession API",
        )));
    };
    let active = vec![existing_summary_from_memory(old_envelope.metadata, old_body)];
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
        &privacy,
    );
    replacement.frontmatter.supersedes.push(old_memory_id.clone());
    if privacy.storage_action.requires_encryption() {
        return Ok(ResponsePayload::GovernanceSupersede(supersede_privacy_refusal(
            old_id,
            Some(policy_applied),
            "encrypted supersession replacements require Stream A encrypted supersession atomicity",
        )));
    }
    substrate
        .supersede_memory(SubstrateSupersedeRequest {
            old_id: old_memory_id,
            replacement,
            reason,
            classification: privacy.tier.classification(),
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
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let tombstone_claim = match &envelope.content {
        MemoryContent::Plaintext(body) if !body.is_empty() => body.clone(),
        MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly | MemoryContent::Plaintext(_) => {
            envelope.metadata.frontmatter.summary.clone()
        }
    };
    substrate
        .tombstone_memory(TombstoneRequest { id: memory_id, reason: reason.clone() })
        .await
        .map_err(HandlerError::substrate)?;
    write_tombstone_rule(substrate.roots().repo.as_path(), &envelope.metadata, &tombstone_claim, &reason)?;
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
    let WriteExecution { input, id, decision, policy_source, privacy } = execution;
    match decision {
        GovernanceWriteDecision::Promoted { namespace, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
                &privacy,
            );
            write_governed_memory(substrate, memory, &privacy).await?;
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
                &privacy,
            );
            write_governed_memory(substrate, memory, &privacy).await?;
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
                &privacy,
            );
            write_governed_memory(substrate, memory, &privacy).await?;
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

async fn write_governed_memory(
    substrate: &Substrate,
    memory: Memory,
    privacy: &PrivacyDecision,
) -> Result<(), HandlerError> {
    write_privacy_memory(
        substrate,
        memory,
        privacy,
        EventContext {
            actor: Some("memoryd-governance".to_string()),
            reason: Some("governed privacy-mediated write".to_string()),
        },
    )
    .await
}

async fn write_privacy_memory(
    substrate: &Substrate,
    mut memory: Memory,
    privacy: &PrivacyDecision,
    event_context: EventContext,
) -> Result<(), HandlerError> {
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret before disk effects"));
    }
    attach_privacy_scan(&mut memory, privacy);
    if privacy.storage_action.requires_encryption() {
        let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
        let encrypted = encryptor.encrypt(&memory.body).map_err(HandlerError::privacy)?;
        memory.frontmatter.extras.insert("encryption".to_string(), encrypted.envelope);
        let safe_index_projection = safe_index_projection(&memory);
        substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: encrypted.ciphertext,
                // Stream D: encrypted records index only descriptors already proven safe.
                // Do NOT project raw or masked body text here; see stream-d-security-review P0.
                safe_index_projection,
                event_context,
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .map(|_| ())
            .map_err(HandlerError::substrate)
    } else {
        substrate
            .write_memory(SubstrateWriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context,
                allow_best_effort_durability: true,
                classification: privacy.tier.classification(),
            })
            .await
            .map(|_| ())
            .map_err(HandlerError::substrate)
    }
}

fn classify_input_privacy(input: &GovernanceWriteInput) -> Result<PrivacyDecision, HandlerError> {
    classify_privacy(&input.privacy_scan_text(), input.privacy_namespace(), input.caller_sensitivity())
}

fn classify_privacy(
    text: &str,
    namespace: PrivacyNamespace,
    caller: Option<CallerSensitivity>,
) -> Result<PrivacyDecision, HandlerError> {
    DeterministicPrivacyClassifier::new().classify(text, namespace, caller).map_err(HandlerError::privacy)
}

fn attach_privacy_scan(memory: &mut Memory, privacy: &PrivacyDecision) {
    memory.frontmatter.extras.insert(
        "privacy_scan".to_string(),
        serde_json::to_value(&privacy.scan).expect("privacy scan always serializes"),
    );
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

fn supersede_privacy_refusal(
    old_id: String,
    policy_applied: Option<String>,
    policy_source: impl Into<String>,
) -> GovernanceSupersedeResponse {
    GovernanceSupersedeResponse {
        status: GovernanceStatus::Refused,
        new_id: None,
        old_id: Some(old_id),
        reason: Some(GovernanceRefusalReason::Privacy),
        chain: None,
        policy_applied,
        policy_source: Some(policy_source.into()),
    }
}

fn existing_summary_from_memory(memory: Memory, body: String) -> ExistingMemorySummary {
    ExistingMemorySummary::new(
        memory.frontmatter.id.as_str().to_string(),
        namespace_for_frontmatter(&memory.frontmatter),
        body,
        1.0,
    )
    .with_entity_ids(entity_ids(&memory.frontmatter))
}

fn write_tombstone_rule(repo: &Path, memory: &Memory, claim: &str, reason: &str) -> Result<(), HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    std::fs::create_dir_all(&tombstone_dir)
        .map_err(|error| HandlerError::substrate(format!("create tombstone dir: {error}")))?;
    let key = memory_governance::CandidateTombstoneKey::from_claim(claim, entity_ids(&memory.frontmatter))
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
    privacy: PrivacyDecision,
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
    privacy_descriptors: Option<PrivacyDescriptors>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PrivacyDescriptors {
    subject: Option<String>,
    role: Option<String>,
    organization: Option<String>,
    office: Option<String>,
    value_kind: Option<String>,
    lookup_hints: Vec<String>,
}

impl PrivacyDescriptors {
    fn values(&self) -> Vec<String> {
        let mut values = [
            self.subject.clone(),
            self.role.clone(),
            self.organization.clone(),
            self.office.clone(),
            self.value_kind.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        values.extend(self.lookup_hints.iter().cloned());
        values
    }
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
            privacy_descriptors: None,
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

    fn privacy_scan_text(&self) -> String {
        let mut fields = vec![self.body.as_str()];
        if let Some(title) = &self.title {
            fields.push(title.as_str());
        }
        if let Some(summary) = &self.meta.summary {
            fields.push(summary.as_str());
        }
        if let Some(source_ref) = &self.meta.source_ref {
            fields.push(source_ref.as_str());
        }
        fields.extend(self.tags.iter().map(String::as_str));
        let mut text = fields.join("\n");
        if let Some(descriptors) = &self.meta.privacy_descriptors {
            for value in descriptors.values() {
                text.push('\n');
                text.push_str(&value);
            }
        }
        text
    }

    fn privacy_refusal(&self, privacy: &PrivacyDecision) -> Option<GovernanceWriteResponse> {
        match privacy.storage_action {
            PrivacyStorageAction::Refuse => Some(GovernanceWriteResponse {
                status: GovernanceStatus::Refused,
                id: None,
                namespace: Some(self.response_namespace()),
                reason: Some(GovernanceRefusalReason::Privacy),
                next_actions: vec!["remove_secret_material".to_string()],
                policy_applied: None,
                policy_source: None,
                existing_id: None,
            }),
            PrivacyStorageAction::Plaintext | PrivacyStorageAction::EncryptAtRest => None,
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

    fn to_memory(&self, id: MemoryId, lifecycle: GovernedLifecycle, privacy: &PrivacyDecision) -> Memory {
        let now = chrono::Utc::now();
        let summary = self.summary(privacy.storage_action);
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

        let sensitivity = privacy.tier.persisted_sensitivity().unwrap_or(Sensitivity::Internal);
        let encrypted = privacy.storage_action.requires_encryption();
        let indexable = !encrypted && !matches!(lifecycle.status, MemoryStatus::Quarantined);
        if let Some(descriptors) = self.safe_privacy_descriptors_value() {
            extras.insert("privacy_descriptors".to_string(), descriptors);
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
                sensitivity,
                status: lifecycle.status,
                created_at: now,
                updated_at: now,
                author: self.author(),
                namespace: self.substrate_namespace(),
                canonical_namespace_id: self.substrate_namespace(),
                tags: self.persisted_tags(privacy.storage_action),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: self.substrate_source(privacy.storage_action),
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
                    mask_personal_for_synthesis: encrypted,
                    index_body: indexable,
                    index_embeddings: indexable,
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

    fn summary(&self, storage_action: PrivacyStorageAction) -> String {
        let candidate = self.meta.summary.clone().or_else(|| self.title.clone());
        if storage_action.requires_encryption() {
            return candidate
                .filter(|value| is_safe_plaintext_for_indexing(value))
                .unwrap_or_else(|| "encrypted memory".to_string());
        }
        candidate.unwrap_or_else(|| bounded(&self.body, 120))
    }

    fn persisted_tags(&self, storage_action: PrivacyStorageAction) -> Vec<String> {
        if storage_action.requires_encryption() {
            self.tags.iter().filter(|tag| is_safe_plaintext_for_indexing(tag)).cloned().collect()
        } else {
            self.tags.clone()
        }
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

    fn privacy_namespace(&self) -> PrivacyNamespace {
        match self.meta.namespace {
            GovernanceNamespace::Me => PrivacyNamespace::Me,
            GovernanceNamespace::Project => PrivacyNamespace::Project,
            GovernanceNamespace::Agent => PrivacyNamespace::Agent,
        }
    }

    fn caller_sensitivity(&self) -> Option<CallerSensitivity> {
        self.meta.sensitivity.map(|sensitivity| match sensitivity {
            GovernanceSensitivity::Public => CallerSensitivity::Public,
            GovernanceSensitivity::Internal => CallerSensitivity::Internal,
            GovernanceSensitivity::Confidential => CallerSensitivity::Confidential,
            GovernanceSensitivity::Personal => CallerSensitivity::Personal,
            GovernanceSensitivity::Sensitive => CallerSensitivity::Sensitive,
            GovernanceSensitivity::Secret => CallerSensitivity::Secret,
        })
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

    fn substrate_source(&self, storage_action: PrivacyStorageAction) -> Source {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => SourceKind::User,
            GovernanceSourceKindMeta::Subagent => SourceKind::AgentSubagent,
            GovernanceSourceKindMeta::File => SourceKind::File,
            GovernanceSourceKindMeta::AgentPrimary => SourceKind::AgentPrimary,
        };
        Source {
            kind,
            reference: if storage_action.requires_encryption() {
                self.meta
                    .source_ref
                    .clone()
                    .filter(|reference| is_safe_plaintext_for_indexing(reference))
                    .or_else(|| Some("memoryd.governance".to_string()))
            } else {
                self.meta.source_ref.clone().or_else(|| Some("memoryd.governance".to_string()))
            },
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        }
    }

    fn safe_privacy_descriptors_value(&self) -> Option<Value> {
        let descriptors = self.meta.privacy_descriptors.as_ref()?;
        let mut object = serde_json::Map::new();
        insert_safe_descriptor(&mut object, "subject", descriptors.subject.as_deref());
        insert_safe_descriptor(&mut object, "role", descriptors.role.as_deref());
        insert_safe_descriptor(&mut object, "organization", descriptors.organization.as_deref());
        insert_safe_descriptor(&mut object, "office", descriptors.office.as_deref());
        insert_safe_descriptor(&mut object, "value_kind", descriptors.value_kind.as_deref());
        let hints = descriptors
            .lookup_hints
            .iter()
            .filter(|hint| is_safe_plaintext_for_indexing(hint))
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        if !hints.is_empty() {
            object.insert("lookup_hints".to_string(), Value::Array(hints));
        }
        (!object.is_empty()).then_some(Value::Object(object))
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

fn candidate_memory(id: MemoryId, text: &str, storage_action: PrivacyStorageAction) -> Memory {
    let now = chrono::Utc::now();
    let sensitivity = Sensitivity::Internal;
    let encrypted = storage_action.requires_encryption();
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: if encrypted { "encrypted note".to_string() } else { bounded(text, 120) },
            confidence: 0.5,
            trust_level: TrustLevel::Candidate,
            sensitivity,
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
            tags: if encrypted { Vec::new() } else { vec!["candidate".to_string(), "memoryd-note".to_string()] },
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
                mask_personal_for_synthesis: encrypted,
                index_body: !encrypted,
                index_embeddings: !encrypted,
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

fn insert_safe_descriptor(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| is_safe_plaintext_for_indexing(value)) {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn is_safe_plaintext_for_indexing(text: &str) -> bool {
    matches!(safe_plaintext_fragment(&DeterministicPrivacyClassifier::new(), text), SafeFragmentDecision::Allow)
}

fn safe_index_projection(memory: &Memory) -> Option<IndexProjection> {
    let mut fragments = Vec::new();
    if !memory.frontmatter.summary.starts_with("encrypted ") {
        fragments.push(memory.frontmatter.summary.clone());
    }
    fragments.extend(memory.frontmatter.tags.iter().cloned());
    if let Some(reference) = &memory.frontmatter.source.reference {
        if reference != "memoryd.governance" && reference != "memoryd.write_note" {
            fragments.push(reference.clone());
        }
    }
    if let Some(descriptors) = memory.frontmatter.extras.get("privacy_descriptors") {
        collect_descriptor_strings(descriptors, &mut fragments);
    }
    let safe_body = fragments
        .into_iter()
        .map(|fragment| fragment.trim().to_string())
        .filter(|fragment| !fragment.is_empty() && is_safe_plaintext_for_indexing(fragment))
        .collect::<Vec<_>>()
        .join("\n");
    (!safe_body.is_empty()).then_some(IndexProjection { safe_body: Some(safe_body) })
}

fn collect_descriptor_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => values.iter().for_each(|value| collect_descriptor_strings(value, output)),
        Value::Object(values) => values.values().for_each(|value| collect_descriptor_strings(value, output)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
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

    fn dream_unavailable(message: impl Into<String>) -> Self {
        Self { code: "dream_unavailable".to_string(), message: message.into(), retryable: true }
    }

    fn dream_disabled(message: impl Into<String>) -> Self {
        Self { code: "dream_disabled".to_string(), message: message.into(), retryable: false }
    }

    fn substrate(error: impl std::fmt::Display) -> Self {
        Self { code: "substrate_error".to_string(), message: error.to_string(), retryable: true }
    }

    fn privacy(error: impl std::fmt::Display) -> Self {
        Self { code: "privacy_error".to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    fn from_recall(error: RecallError) -> Self {
        Self {
            code: error.protocol_code().to_owned(),
            message: bounded(error.message(), 240),
            retryable: error.retryable(),
        }
    }

    fn from_dream(error: crate::dream::types::DreamError) -> Self {
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    fn from_lease(error: crate::dream::lease::LeaseError) -> Self {
        let retryable = matches!(
            error,
            crate::dream::lease::LeaseError::Held { .. } | crate::dream::lease::LeaseError::Unavailable { .. }
        );
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable }
    }
}
