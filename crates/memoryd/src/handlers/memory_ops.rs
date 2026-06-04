//! Memory CRUD and observe request handlers: search/get/reveal/write-note, the
//! observe pipeline with its field validators and encrypted-payload helpers, and
//! the delta/startup recall responses.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

use super::governance::{classify_privacy, write_privacy_memory};
use super::*;

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

pub(crate) async fn delta_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::DeltaRequest,
) -> Result<ResponsePayload, HandlerError> {
    let coordination = DeltaCoordinationContext {
        config: state.coordination_config(),
        presence: state.presence(),
        claim_locks: state.claim_locks(),
        delivery_recorder: Some(state),
        peer_cooldown: Some(state),
    };
    match build_delta_response_with_coordination(substrate, request, coordination).await {
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

pub(crate) async fn startup_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::StartupRequest,
) -> Result<ResponsePayload, HandlerError> {
    match build_startup_response_with_coordination_config(substrate, request, state.coordination_config().clone()).await
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
    }
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
    let mut hits = Vec::new();
    for chunk in chunks.into_iter().take(limit) {
        let body = if include_body {
            match substrate.read_memory_envelope(&chunk.memory_id).await {
                Ok(envelope) => match envelope.content {
                    // Bound the body to the same cap memory_get applies; search must return a
                    // bounded preview, never a bulk dump of full plaintext bodies (SEC-03).
                    MemoryContent::Plaintext(body) => Some(bounded(&body, GET_BODY_MAX)),
                    MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
                },
                Err(memory_substrate::ReadError::NotACanonicalMemory { .. }) => None,
                Err(err) => {
                    tracing::warn!(memory_id = %chunk.memory_id, "search read failed: {err}");
                    None
                }
            }
        } else {
            None
        };
        hits.push(SearchHit {
            id: chunk.memory_id.as_str().to_string(),
            summary: bounded(&chunk.text, SEARCH_SNIPPET_MAX),
            snippet: bounded(&chunk.text, SEARCH_SNIPPET_MAX),
            body,
            score: chunk.score,
        });
    }

    let guidance = if include_body {
        "Search returns bounded matching chunks; call memory_get for the bounded record preview.".to_string()
    } else {
        "Bounded snippets only; call memory_get for full body access when policy allows.".to_string()
    };
    Ok(ResponsePayload::Search(SearchResponse { hits, total, guidance }))
}

pub(crate) async fn get_response(
    substrate: &Substrate,
    id: &str,
    include_provenance: bool,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let provenance = include_provenance.then(|| get_provenance(&envelope.metadata));
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
        provenance,
        guidance: "Returned a bounded Memorum record preview.".to_string(),
    }))
}

fn get_provenance(memory: &Memory) -> GetProvenance {
    GetProvenance {
        path: memory.path.as_ref().map(|path| path.as_str().to_string()),
        source_kind: serialized_enum_value(&memory.frontmatter.source.kind),
        source_ref: memory.frontmatter.source.reference.clone(),
        author_kind: serialized_enum_value(&memory.frontmatter.author.kind),
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

pub(crate) fn serialized_enum_value<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_value(value)
        .expect("invariant: caller passes a unit-variant enum that serde always serializes infallibly");
    json.as_str().expect("invariant: callers pass unit-variant enums that serialize to JSON strings").to_string()
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
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
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

pub(crate) async fn write_note_response(substrate: &Substrate, text: &str) -> Result<ResponsePayload, HandlerError> {
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
