//! Memory CRUD and observe request handlers: search/get/reveal/write-note, the
//! observe pipeline with its field validators and encrypted-payload helpers, and
//! the delta/startup recall responses.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::time::{Duration, Instant};

use super::governance::{classify_privacy, write_privacy_memory};
use super::*;
use crate::recall::config::DEFAULT_VECTOR_RECALL_RRF_K;
use crate::recall::fusion::reciprocal_rank_score;
use crate::util::serialized_enum_value;

const SEARCH_LIMIT_DEFAULT: usize = 10;
const SEARCH_LIMIT_MAX: usize = 20;
const SEARCH_SNIPPET_MAX: usize = 240;
const GET_BODY_MAX: usize = 4_096;
const OBSERVE_TEXT_MAX_BYTES: usize = 16 * 1024;
const OBSERVE_ENTITIES_MAX: usize = 32;
const OBSERVE_ENTITY_MAX_BYTES: usize = 128;
const OBSERVE_ENTITY_BODY_MAX_BYTES: usize = 124;
const OBSERVE_BINDING_FIELD_MAX_BYTES: usize = 128;
const CLAIM_LOCK_IDENTITY_MAX_BYTES: usize = 128;

pub(crate) struct SearchResponseRequest<'a> {
    pub query: &'a str,
    pub limit: Option<usize>,
    pub include_body: bool,
}

pub(crate) async fn delta_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::DeltaRequest,
) -> Result<ResponsePayload, HandlerError> {
    let started = Instant::now();
    let coordination = DeltaCoordinationContext {
        config: state.coordination_config(),
        presence: state.presence(),
        claim_locks: state.claim_locks(),
        delivery_recorder: Some(state),
        peer_cooldown: Some(state),
    };
    let config = load_vector_recall_config(substrate);
    let mode = if active_embedding_is_api(substrate) {
        crate::recall::FusionMode::Legacy
    } else {
        crate::recall::FusionMode::FourLaneHook
    };
    let vector_recall =
        crate::recall::VectorRecallContext::from_lifecycle(state.embedding_provider_slot(), config).with_mode(mode);
    let result = match crate::recall::build_delta_response_with_vector_recall_and_coordination(
        substrate,
        request,
        coordination,
        vector_recall,
    )
    .await
    {
        Ok(response) => {
            state.recall.record_delta_success();
            Ok(ResponsePayload::Delta(response))
        }
        Err(error) => {
            state.recall.record_delta_failure(error.protocol_code());
            Err(HandlerError::from_recall(error))
        }
    };
    state.recall.record_latency(prompt_latency_surface(substrate), started.elapsed());
    result
}

pub(crate) async fn startup_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::StartupRequest,
) -> Result<ResponsePayload, HandlerError> {
    let started = Instant::now();
    let result = match build_startup_response_with_coordination_config(
        substrate,
        request,
        state.coordination_config().clone(),
        state.recall_dedup(),
    )
    .await
    {
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
    };
    if !active_embedding_is_api(substrate) {
        state.recall.record_latency("desk_cue_local", started.elapsed());
    }
    result
}

fn record_budget_exhaustions(state: &HandlerState, response: &StartupResponse) {
    for omission in &response.recall_explanation.omitted {
        if omission.reason == OmissionReason::BudgetExhausted {
            state.recall.record_budget_exhausted(omission.section.as_str());
        }
    }
}

pub(crate) async fn search_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: SearchResponseRequest<'_>,
) -> Result<ResponsePayload, HandlerError> {
    let query = request.query.trim();
    if query.is_empty() {
        return Err(HandlerError::invalid_request("search query must not be empty"));
    }

    let limit = request.limit.unwrap_or(SEARCH_LIMIT_DEFAULT).min(SEARCH_LIMIT_MAX);
    let started = Instant::now();
    let config = load_vector_recall_config(substrate);
    let search_timeout = Duration::from_millis(config.search_timeout_ms);
    let vector_recall = crate::recall::VectorRecallContext::from_lifecycle(state.embedding_provider_slot(), config)
        .with_mode(crate::recall::FusionMode::FourLaneSearch);
    let decision = tokio::time::timeout(
        search_timeout,
        crate::recall::hybrid::collect_hybrid_recall(substrate, query, Some(&vector_recall)),
    )
    .await;
    let (total, mut hits, vector_recall_degraded) = match decision {
        Ok(crate::recall::hybrid::HybridRecallDecision::Fused { candidates, degraded }) => {
            if let Some(marker) = degraded {
                tracing::warn!(marker, "memory_search four-lane recall partially degraded");
            }
            let total = candidates.len();
            let hits = candidates
                .into_iter()
                .take(limit)
                .map(|candidate| SearchHit {
                    id: candidate.id,
                    summary: bounded(&candidate.text, SEARCH_SNIPPET_MAX),
                    snippet: bounded(&candidate.text, SEARCH_SNIPPET_MAX),
                    body: None,
                    score: candidate.final_score,
                })
                .collect::<Vec<_>>();
            (total, hits, degraded.map(str::to_owned))
        }
        Ok(crate::recall::hybrid::HybridRecallDecision::FtsOnly { degraded }) => {
            if let Some(marker) = degraded {
                tracing::warn!(marker, "memory_search vector recall degraded; falling back to FTS-only");
            }
            let remaining = search_timeout.saturating_sub(started.elapsed());
            match tokio::time::timeout(remaining, fts_search_hits(substrate, query, limit)).await {
                Ok(Ok((total, hits))) => (total, hits, degraded.map(str::to_owned)),
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    tracing::warn!("memory_search fallback FTS timed out within the search envelope");
                    (0, Vec::new(), Some(crate::recall::hybrid::DEGRADED_FOUR_LANE_TIMEOUT.to_owned()))
                }
            }
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = search_timeout.as_millis(),
                "memory_search timed out; falling back to FTS-only"
            );
            let remaining = search_timeout.saturating_sub(started.elapsed());
            match tokio::time::timeout(remaining, fts_search_hits(substrate, query, limit)).await {
                Ok(Ok((total, hits))) => {
                    (total, hits, Some(crate::recall::hybrid::DEGRADED_FOUR_LANE_TIMEOUT.to_owned()))
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    tracing::warn!("memory_search fallback FTS timed out within the search envelope");
                    (0, Vec::new(), Some(crate::recall::hybrid::DEGRADED_FOUR_LANE_TIMEOUT.to_owned()))
                }
            }
        }
    };

    if request.include_body {
        attach_search_bodies(substrate, &mut hits).await?;
    }

    let guidance = if request.include_body {
        "Search returns bounded matching chunks; call memory_get for the bounded record preview.".to_string()
    } else {
        "Bounded snippets only; call memory_get for full body access when policy allows.".to_string()
    };
    let response = ResponsePayload::Search(SearchResponse { hits, total, guidance, vector_recall_degraded });
    state.recall.record_latency(search_latency_surface(substrate), started.elapsed());
    Ok(response)
}

fn active_embedding_is_api(substrate: &Substrate) -> bool {
    substrate.active_embedding_triple().is_ok_and(|triple| crate::embedding::is_api_embedding_lane(&triple))
}

fn prompt_latency_surface(substrate: &Substrate) -> &'static str {
    if active_embedding_is_api(substrate) {
        "prompt_cue_api"
    } else {
        "prompt_cue_local"
    }
}

fn search_latency_surface(substrate: &Substrate) -> &'static str {
    if active_embedding_is_api(substrate) {
        "search_api"
    } else {
        "search_local"
    }
}

fn load_vector_recall_config(substrate: &Substrate) -> crate::recall::VectorRecallConfig {
    crate::recall::load_recall_config(substrate.roots().repo.as_path())
        .map(|config| config.vector_recall)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "recall: failed to load config; vector recall defaults applied");
            crate::recall::VectorRecallConfig::default()
        })
}

async fn fts_search_hits(
    substrate: &Substrate,
    query: &str,
    limit: usize,
) -> Result<(usize, Vec<SearchHit>), HandlerError> {
    // Reuse the hybrid BM25 helper so FTS-only search gets the same two-stage
    // strict-AND -> relaxed-OR fallback and per-memory collapse as the fused
    // lane, without duplicating the query sanitizer.
    let candidates = substrate.query_hybrid_chunks(query, None, limit).await.map_err(HandlerError::substrate)?;
    let k = f64::from(DEFAULT_VECTOR_RECALL_RRF_K);
    let total = candidates.len();
    let hits = candidates
        .into_iter()
        .map(|candidate| SearchHit {
            id: candidate.memory_id.as_str().to_string(),
            summary: bounded(&candidate.text, SEARCH_SNIPPET_MAX),
            snippet: bounded(&candidate.text, SEARCH_SNIPPET_MAX),
            body: None,
            score: candidate.score_breakdown.bm25_rank.map_or(0.0, |rank| reciprocal_rank_score(k, rank)),
        })
        .collect::<Vec<_>>();
    Ok((total, hits))
}

/// Populate the bounded body preview for each search hit, overlapping the
/// per-hit canonical-file reads instead of awaiting them one at a time.
///
/// Each read is a synchronous disk-read + Markdown parse; run serially they
/// serialize end to end (and, on a single-threaded runtime, stall the executor).
/// Cloning `Substrate` is cheap — all state is behind `Arc` — so we fan the
/// bounded reads (capped at `SEARCH_LIMIT_MAX`) onto the blocking pool via
/// `spawn_blocking` + `read_memory_envelope_blocking`, keeping the disk work off
/// the async worker threads exactly as the sibling governance active-memory fan
/// out does, then assign bodies back by hit index to preserve output order and
/// content. The bounding cap and the plaintext-only/ciphertext-skip rules are
/// unchanged from the prior serial path (SEC-03: never a bulk dump of full
/// plaintext bodies).
async fn attach_search_bodies(substrate: &Substrate, hits: &mut [SearchHit]) -> Result<(), HandlerError> {
    let mut reads = tokio::task::JoinSet::new();
    for (position, hit) in hits.iter().enumerate() {
        let substrate = substrate.clone();
        let memory_id = MemoryId::new(hit.id.clone());
        // One blocking-pool task per hit (capped at SEARCH_LIMIT_MAX): the read is
        // a synchronous disk-read + Markdown parse, so `spawn_blocking` keeps it
        // off the async worker threads, matching the governance active-memory
        // fan-out. The whole closure is synchronous, so `memory_id` stays in scope
        // for the failure log.
        reads.spawn_blocking(move || {
            let body = match substrate.read_memory_envelope_blocking(&memory_id) {
                Ok(envelope) => match envelope.content {
                    MemoryContent::Plaintext(body) => Some(bounded(&body, GET_BODY_MAX)),
                    MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
                },
                Err(memory_substrate::ReadError::NotACanonicalMemory { .. }) => None,
                Err(err) => {
                    tracing::warn!(memory_id = %memory_id, "search read failed: {err}");
                    None
                }
            };
            (position, body)
        });
    }

    while let Some(joined) = reads.join_next().await {
        // A panic in a read task is a daemon-internal fault, not a search miss;
        // surface it (retryable) rather than silently dropping the hit's body.
        let (position, body) =
            joined.map_err(|err| HandlerError::substrate(format!("search body read task: {err}")))?;
        if let Some(hit) = hits.get_mut(position) {
            hit.body = body;
        }
    }
    Ok(())
}

pub(crate) async fn get_response(
    substrate: &Substrate,
    id: &str,
    include_provenance: bool,
    full_body: bool,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = HandlerError::parse_memory_id(id)?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::read_memory)?;
    let provenance = include_provenance.then(|| get_provenance(&envelope.metadata));
    let encrypted = matches!(envelope.content, MemoryContent::Ciphertext { .. });
    let body = match envelope.content {
        MemoryContent::Plaintext(body) => body,
        MemoryContent::MetadataOnly => String::new(),
        MemoryContent::Ciphertext { .. } => crate::protocol::ENCRYPTED_BODY_SENTINEL.to_string(),
    };
    let (body, truncated) = if full_body { (body, false) } else { bounded_with_truncation(&body, GET_BODY_MAX) };
    Ok(ResponsePayload::Get(GetResponse {
        id: envelope.metadata.frontmatter.id.as_str().to_string(),
        summary: envelope.metadata.frontmatter.summary,
        body,
        truncated,
        provenance,
        sensitivity: Some(envelope.metadata.frontmatter.sensitivity),
        status: Some(envelope.metadata.frontmatter.status),
        encrypted,
        guidance: if full_body {
            "Returned the full Memorum record body.".to_string()
        } else {
            "Returned a bounded Memorum record preview.".to_string()
        },
    }))
}

fn get_provenance(memory: &Memory) -> GetProvenance {
    GetProvenance {
        path: memory.path.as_ref().map(|path| path.as_str().to_string()),
        source_kind: memory.frontmatter.source.kind.as_db_str().to_string(),
        source_ref: memory.frontmatter.source.reference.clone(),
        author_kind: memory.frontmatter.author.kind.as_db_str().to_string(),
        harness: memory.frontmatter.author.harness.clone().or_else(|| memory.frontmatter.source.harness.clone()),
        session_id: memory
            .frontmatter
            .author
            .session_id
            .clone()
            .or_else(|| memory.frontmatter.source.session_id.clone()),
        evidence_refs: memory.frontmatter.evidence.iter().map(|evidence| evidence.reference.clone()).collect(),
    }
}

pub(crate) async fn reveal_response(
    substrate: &Substrate,
    id: &str,
    reason: &str,
) -> Result<ResponsePayload, HandlerError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(HandlerError::invalid_request("reveal reason must not be empty"));
    }
    if reason.chars().count() > REVEAL_REASON_MAX_CHARS {
        return Err(HandlerError::invalid_request("reveal reason must be at most 512 characters"));
    }
    let memory_id = HandlerError::parse_memory_id(id)?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::read_memory)?;
    let MemoryContent::Ciphertext { bytes, encryption } = envelope.content else {
        return Err(HandlerError::invalid_request("memory_reveal requires an encrypted memory"));
    };
    let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
    let body = encryptor
        .decrypt(&EncryptedPayload {
            ciphertext: bytes,
            envelope: encryption.metadata.unwrap_or_else(|| {
                serde_json::json!({
                    "scheme": encryption.scheme,
                    "recipient": encryption.recipient,
                })
            }),
        })
        .map_err(HandlerError::privacy)?;
    // The reveal reason is caller-supplied free text persisted verbatim into the canonical
    // event log (EncryptedContentRevealed) and surfaced in event summaries. Redact any
    // secret/PII content to "[redacted]" before persistence — same policy as a forget
    // reason — so the audit trail can never leak a secret to disk (invariant 1).
    substrate
        .record_encrypted_content_revealed(memory_id, super::sanitize_reason(reason, REVEAL_REASON_MAX_CHARS))
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

pub(crate) async fn write_note_response(
    substrate: &Substrate,
    text: &str,
    meta: &serde_json::Value,
) -> Result<ResponsePayload, HandlerError> {
    #[derive(Default, serde::Deserialize)]
    #[serde(default, deny_unknown_fields)]
    struct NoteMeta {
        abstraction: Option<String>,
        cues: Vec<String>,
        #[serde(rename = "cwd")]
        _cwd: Option<String>,
    }

    let text = text.trim();
    if text.is_empty() {
        return Err(HandlerError::invalid_request("note text must not be empty"));
    }
    let meta = if meta.is_null() {
        NoteMeta::default()
    } else {
        serde_json::from_value::<NoteMeta>(meta.clone())
            .map_err(|error| HandlerError::invalid_request(format!("invalid note meta: {error}")))?
    };
    let abstraction = memory_substrate::frontmatter::normalize_abstraction_value(meta.abstraction)
        .map_err(|error| HandlerError::invalid_request(error.to_string()))?;
    let cues = memory_substrate::frontmatter::normalize_cue_values(meta.cues)
        .map_err(|error| HandlerError::invalid_request(error.to_string()))?;
    let combined = std::iter::once(text)
        .chain(abstraction.as_deref())
        .chain(cues.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let privacy = classify_privacy(&combined, PrivacyNamespace::Agent, None)?;
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret note before disk effects"));
    }

    let memory_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let mut memory = candidate_memory(memory_id, text, privacy.storage_action);
    memory.frontmatter.abstraction = abstraction;
    memory.frontmatter.cues = cues;
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
pub(crate) struct ObserveRequestFields {
    pub(crate) text: String,
    pub(crate) kind: ObserveKind,
    pub(crate) entities: Vec<String>,
    pub(crate) cwd: String,
    pub(crate) session_id: String,
    pub(crate) harness: String,
    pub(crate) harness_version: Option<String>,
}

pub(crate) async fn observe_response(
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

pub(crate) fn validated_claim_lock_identity_field(name: &str, value: String) -> Result<String, HandlerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(HandlerError::invalid_request(format!("{name} must be non-empty")));
    }
    if trimmed.len() > CLAIM_LOCK_IDENTITY_MAX_BYTES {
        return Err(HandlerError::invalid_request(format!("{name} must be at most 128 bytes")));
    }
    if !trimmed.bytes().all(is_observe_binding_byte) {
        return Err(HandlerError::invalid_request(format!("{name} must contain only safe id characters")));
    }
    validate_observe_metadata_is_safe(name, trimmed)?;
    if contains_secret_or_pii_marker(trimmed) {
        return Err(HandlerError::invalid_request(format!("{name} must not contain sensitive material")));
    }
    Ok(trimmed.to_string())
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
            ciphertext_b64: BASE64_STANDARD.encode(&encrypted.ciphertext),
        },
        descriptor: content_aware_encrypted_observe_descriptor(text, kind),
    })
}

fn content_aware_encrypted_observe_descriptor(text: &str, kind: ObserveKind) -> EncryptedSubstrateDescriptor {
    let tag = observe_kind_tag(kind);
    let fallback_tags = vec![tag.to_string()];
    let fallback_summary = format!("encrypted {tag} substrate fragment");
    let projection =
        safe_descriptor_projection(&DeterministicPrivacyClassifier::new(), text, &fallback_summary, &fallback_tags);
    EncryptedSubstrateDescriptor { summary_safe: projection.summary_safe, tag_safe: projection.tag_safe }
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
        .map(|span| PrivacySpanRecord { label: serialized_enum_value(&span.label), start: span.start, end: span.end })
        .collect()
}
