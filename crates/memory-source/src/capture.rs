use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use memory_privacy::{
    DeterministicPrivacyClassifier, FileKeyProvider, PrivacyClassifier, PrivacyEncryptor, PrivacyNamespace,
    PrivacyStorageAction,
};

use crate::adapters::{dispatch_capture, CaptureDispatch};
use crate::error::{SourceError, SourceResult};
use crate::excerpt::create_excerpt_records;
use crate::extract::{extract_text, raw_textual_projection};
use crate::hash::sha256_prefixed;
use crate::model::{
    CaptureMode, CaptureStatus, EncryptionEnvelope, ExtractedTextStorage, RawStorage, SourceArtifactId,
    WebCaptureManifest, WebCaptureSourceRef, WEB_CAPTURE_SCHEMA_VERSION,
};
use crate::storage::{excerpts_jsonl, ArtifactStore, WebCaptureArtifact};
use crate::url_safety::{AddressPolicy, DefaultDnsResolver, DnsResolver};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureWebSourceRequest {
    pub url: String,
    pub excerpts: Vec<String>,
    pub note: Option<String>,
    pub mode: CaptureMode,
    pub local_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
}

impl Default for CaptureWebSourceRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            excerpts: Vec::new(),
            note: None,
            mode: CaptureMode::HttpStatic,
            local_path: None,
            key_path: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureWebSourceResponse {
    pub artifact_id: String,
    pub source_refs: Vec<String>,
    pub final_url: String,
    pub captured_at: chrono::DateTime<Utc>,
    pub capture_status: CaptureStatus,
    pub warnings: Vec<String>,
}

pub async fn capture_web_source(
    repo_root: impl Into<std::path::PathBuf>,
    request: CaptureWebSourceRequest,
) -> SourceResult<CaptureWebSourceResponse> {
    capture_web_source_with_resolver(repo_root, request, &DefaultDnsResolver, AddressPolicy::PublicOnly).await
}

pub async fn capture_web_source_with_resolver(
    repo_root: impl Into<std::path::PathBuf>,
    request: CaptureWebSourceRequest,
    resolver: &dyn DnsResolver,
    policy: AddressPolicy,
) -> SourceResult<CaptureWebSourceResponse> {
    if request.excerpts.is_empty() {
        return Err(SourceError::ExcerptNotFound("at least one excerpt is required".to_string()));
    }
    let classifier = DeterministicPrivacyClassifier::new();
    for excerpt in &request.excerpts {
        if !is_plaintext_safe(&classifier, excerpt) {
            return Err(SourceError::privacy("source capture excerpts must be safe plaintext"));
        }
    }
    let store = ArtifactStore::new(repo_root);
    let fetched = dispatch_capture(CaptureDispatch {
        mode: request.mode,
        url: Some(request.url.as_str()),
        local_path: request.local_path.as_deref(),
        resolver,
        policy,
    })
    .await?;
    let key_provider = request.key_path.as_ref().map(FileKeyProvider::new);

    let captured_at = Utc::now();
    let artifact_id = SourceArtifactId::generate();
    let content_type = fetched.response.content_type.clone();
    let raw_bytes = fetched.bytes;
    let mut extracted = extract_text(content_type.as_deref(), &raw_bytes)?;
    if !extracted.is_supported() {
        return Err(SourceError::Unsupported(
            extracted.unsupported_reason.unwrap_or_else(|| "unsupported content type".to_string()),
        ));
    }

    let extracted_policy = classify_extracted_text(&classifier, &extracted.text)?;
    let excerpts = create_excerpt_records(&artifact_id, &extracted.text, &request.excerpts, captured_at)?;
    let excerpts_jsonl = excerpts_jsonl(&excerpts)?;
    let extracted_artifact = prepare_extracted_text(&extracted.text, extracted_policy, key_provider.as_ref())?;

    let raw_text = raw_textual_projection(content_type.as_deref(), &raw_bytes);
    let raw_is_safe = raw_text.as_deref().is_some_and(|text| is_plaintext_safe(&classifier, text));
    let raw_artifact = prepare_raw_artifact(&raw_bytes, raw_is_safe, key_provider.as_ref())?;

    let mut warnings = Vec::new();
    warnings.append(&mut extracted.warnings);
    if raw_artifact.omitted_reason.is_some() {
        warnings.push("raw_omitted_privacy".to_string());
    }
    if matches!(raw_artifact.storage, RawStorage::Encrypted) {
        warnings.push("raw_encrypted".to_string());
    }
    if matches!(extracted_artifact.storage, ExtractedTextStorage::Encrypted) {
        warnings.push("extracted_encrypted".to_string());
    }

    let capture_status = if matches!(raw_artifact.storage, RawStorage::OmittedPrivacy | RawStorage::OmittedUnsupported)
    {
        CaptureStatus::CompleteTextOnly
    } else {
        CaptureStatus::Complete
    };

    let manifest = WebCaptureManifest {
        schema_version: WEB_CAPTURE_SCHEMA_VERSION,
        artifact_id: artifact_id.clone(),
        kind: "web_capture".to_string(),
        original_url: fetched.original_ref,
        final_url: fetched.final_ref,
        redirect_chain: fetched.redirect_chain,
        captured_at,
        capture_method: fetched.capture_method,
        request: fetched.request,
        response: fetched.response,
        raw_sha256: raw_artifact.raw_sha256,
        raw_zstd_sha256: raw_artifact.raw_zstd_sha256,
        raw_encrypted_sha256: raw_artifact.encrypted_sha256,
        raw_storage: raw_artifact.storage,
        raw_omitted_reason: raw_artifact.omitted_reason,
        extracted_text_storage: extracted_artifact.storage,
        encryption_envelope: extracted_artifact.envelope.or(raw_artifact.envelope),
        extracted_text_sha256: extracted_artifact.plaintext_sha256,
        extracted_text_encrypted_sha256: extracted_artifact.ciphertext_sha256,
        excerpts_sha256: sha256_prefixed(excerpts_jsonl.as_bytes()),
        raw_byte_len: raw_bytes.len(),
        extracted_text_byte_len: extracted_artifact.plaintext_byte_len,
        extracted_text_encrypted_byte_len: extracted_artifact.ciphertext_byte_len,
        capture_status,
        warnings,
        merge_conflict: None,
    };
    let artifact = WebCaptureArtifact {
        manifest,
        extracted_text: extracted_artifact.plaintext,
        excerpts,
        raw_bytes: raw_artifact.raw_bytes,
        encrypted_extracted_bytes: extracted_artifact.ciphertext,
        encrypted_raw_bytes: raw_artifact.encrypted_bytes,
    };
    store.write_web_capture(&artifact)?;
    let source_refs = artifact
        .excerpts
        .iter()
        .map(|record| WebCaptureSourceRef::new(artifact_id.clone(), record.excerpt_id.clone()).to_string())
        .collect::<Vec<_>>();
    Ok(CaptureWebSourceResponse {
        artifact_id: artifact_id.to_string(),
        source_refs,
        final_url: artifact.manifest.final_url.clone(),
        captured_at,
        capture_status: artifact.manifest.capture_status,
        warnings: artifact.manifest.warnings,
    })
}

#[derive(Clone, Debug)]
struct RawArtifactStorage {
    storage: RawStorage,
    omitted_reason: Option<String>,
    raw_bytes: Option<Vec<u8>>,
    encrypted_bytes: Option<Vec<u8>>,
    raw_sha256: Option<String>,
    raw_zstd_sha256: Option<String>,
    encrypted_sha256: Option<String>,
    envelope: Option<EncryptionEnvelope>,
}

#[derive(Clone, Debug)]
struct ExtractedArtifactStorage {
    plaintext: String,
    ciphertext: Option<Vec<u8>>,
    storage: ExtractedTextStorage,
    plaintext_sha256: Option<String>,
    plaintext_byte_len: Option<usize>,
    ciphertext_sha256: Option<String>,
    ciphertext_byte_len: Option<usize>,
    envelope: Option<EncryptionEnvelope>,
}

fn classify_extracted_text(
    classifier: &DeterministicPrivacyClassifier,
    text: &str,
) -> SourceResult<PrivacyStorageAction> {
    let decision = classifier
        .classify(text, PrivacyNamespace::Project, None)
        .map_err(|err| SourceError::privacy(format!("classify extracted text: {err}")))?;
    Ok(decision.storage_action)
}

fn prepare_extracted_text(
    text: &str,
    storage_action: PrivacyStorageAction,
    key_provider: Option<&FileKeyProvider>,
) -> SourceResult<ExtractedArtifactStorage> {
    match storage_action {
        PrivacyStorageAction::Plaintext => Ok(ExtractedArtifactStorage {
            plaintext: text.to_string(),
            ciphertext: None,
            storage: ExtractedTextStorage::Plaintext,
            plaintext_sha256: Some(sha256_prefixed(text.as_bytes())),
            plaintext_byte_len: Some(text.len()),
            ciphertext_sha256: None,
            ciphertext_byte_len: None,
            envelope: None,
        }),
        PrivacyStorageAction::EncryptAtRest => {
            let key_provider = key_provider.ok_or_else(|| {
                SourceError::privacy("encrypted_source_artifacts_unsupported: key_path required".to_string())
            })?;
            let (ciphertext, envelope) = encrypt_string(text, key_provider)?;
            let ciphertext_sha256 = sha256_prefixed(&ciphertext);
            let ciphertext_byte_len = ciphertext.len();
            Ok(ExtractedArtifactStorage {
                plaintext: String::new(),
                ciphertext: Some(ciphertext),
                storage: ExtractedTextStorage::Encrypted,
                plaintext_sha256: None,
                plaintext_byte_len: None,
                ciphertext_sha256: Some(ciphertext_sha256),
                ciphertext_byte_len: Some(ciphertext_byte_len),
                envelope: Some(envelope),
            })
        }
        PrivacyStorageAction::Refuse => Err(SourceError::privacy("extracted text refused by privacy policy")),
    }
}

fn prepare_raw_artifact(
    raw_bytes: &[u8],
    raw_is_safe: bool,
    key_provider: Option<&FileKeyProvider>,
) -> SourceResult<RawArtifactStorage> {
    if raw_is_safe {
        let compressed = zstd::encode_all(raw_bytes, 0)?;
        return Ok(RawArtifactStorage {
            storage: RawStorage::Stored,
            omitted_reason: None,
            raw_bytes: Some(raw_bytes.to_vec()),
            encrypted_bytes: None,
            raw_sha256: Some(sha256_prefixed(raw_bytes)),
            raw_zstd_sha256: Some(sha256_prefixed(&compressed)),
            encrypted_sha256: None,
            envelope: None,
        });
    }

    let Some(key_provider) = key_provider else {
        return Ok(RawArtifactStorage {
            storage: RawStorage::OmittedPrivacy,
            omitted_reason: Some("raw textual projection is not safe plaintext".to_string()),
            raw_bytes: None,
            encrypted_bytes: None,
            raw_sha256: Some(sha256_prefixed(raw_bytes)),
            raw_zstd_sha256: None,
            encrypted_sha256: None,
            envelope: None,
        });
    };

    let encoded_raw = BASE64_STANDARD.encode(raw_bytes);
    let (ciphertext, envelope) = encrypt_string(&encoded_raw, key_provider)?;
    Ok(RawArtifactStorage {
        storage: RawStorage::Encrypted,
        omitted_reason: None,
        raw_bytes: None,
        encrypted_bytes: Some(ciphertext.clone()),
        raw_sha256: None,
        raw_zstd_sha256: None,
        encrypted_sha256: Some(sha256_prefixed(&ciphertext)),
        envelope: Some(envelope),
    })
}

fn encrypt_string(text: &str, key_provider: &FileKeyProvider) -> SourceResult<(Vec<u8>, EncryptionEnvelope)> {
    let encryptor = PrivacyEncryptor::new(key_provider.clone());
    let payload =
        encryptor.encrypt(text).map_err(|err| SourceError::privacy(format!("encrypt source artifact: {err}")))?;
    let envelope = encryption_envelope_from_payload(&payload.envelope)?;
    Ok((payload.ciphertext, envelope))
}

fn encryption_envelope_from_payload(envelope: &serde_json::Value) -> SourceResult<EncryptionEnvelope> {
    let scheme = envelope.get("scheme").and_then(serde_json::Value::as_str).unwrap_or("age-x25519");
    let recipient = envelope
        .get("recipient")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| SourceError::privacy("encrypted payload missing recipient"))?;
    if scheme != "age-x25519" {
        return Err(SourceError::privacy(format!("unsupported encryption scheme `{scheme}`")));
    }
    Ok(EncryptionEnvelope::age_x25519(recipient))
}

/// The single privacy-gate predicate: a text is "plaintext safe" when the
/// classifier routes it to `Plaintext` storage. Shared by the excerpt pre-check
/// and the raw-projection check so the predicate is defined once.
fn is_plaintext_safe(classifier: &DeterministicPrivacyClassifier, text: &str) -> bool {
    classifier
        .classify(text, PrivacyNamespace::Project, None)
        .is_ok_and(|decision| matches!(decision.storage_action, PrivacyStorageAction::Plaintext))
}
