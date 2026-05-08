use chrono::Utc;
use memory_privacy::{DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyNamespace, PrivacyStorageAction};
use reqwest::header::{ACCEPT, CONTENT_ENCODING, CONTENT_TYPE, ETAG, LAST_MODIFIED, LOCATION, USER_AGENT};

use crate::error::{SourceError, SourceResult};
use crate::excerpt::create_excerpt_records;
use crate::extract::{extract_text, raw_textual_projection};
use crate::hash::sha256_prefixed;
use crate::model::{
    CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, RawStorage, RedirectHop,
    SourceArtifactId, WebCaptureManifest, WebCaptureSourceRef,
};
use crate::storage::{excerpts_jsonl, ArtifactStore, WebCaptureArtifact};
use crate::url_safety::{
    pinned_reqwest_client, redact_sensitive_location_header, redact_sensitive_url, validate_initial_url,
    validate_redirect_url, AddressPolicy, DefaultDnsResolver, DnsResolver,
};

const MAX_RAW_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureWebSourceRequest {
    pub url: String,
    pub excerpts: Vec<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureWebSourceResponse {
    pub artifact_id: String,
    pub source_refs: Vec<String>,
    pub final_url: String,
    pub captured_at: chrono::DateTime<Utc>,
    pub capture_status: String,
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
    let store = ArtifactStore::new(repo_root);
    let mut hop = validate_initial_url(&request.url, resolver, policy).await?;
    let original_url = redact_sensitive_url(&hop.url).to_string();
    let mut redirect_chain = Vec::new();
    let final_response;
    let raw_bytes;
    loop {
        let client = pinned_reqwest_client(&hop)?;
        let response = client
            .get(hop.url.clone())
            .header(USER_AGENT, CaptureRequestSnapshot::default().user_agent)
            .header(ACCEPT, CaptureRequestSnapshot::default().accept)
            .send()
            .await
            .map_err(|err| SourceError::CaptureFailed(format!("HTTP request failed: {err}")))?;
        let remote_addr = response.remote_addr();
        let Some(remote_addr) = remote_addr else {
            return Err(SourceError::CaptureFailed("HTTP response did not expose remote address".to_string()));
        };
        if !hop.contains_remote_addr(remote_addr) {
            return Err(SourceError::url_safety(format!(
                "response remote address {remote_addr} was not in pinned DNS set"
            )));
        }
        let status = response.status();
        let headers = response.headers().clone();
        if status.is_redirection() {
            let location = headers
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| SourceError::CaptureFailed("redirect response is missing Location".to_string()))?
                .to_string();
            redirect_chain.push(RedirectHop {
                url: redact_sensitive_url(&hop.url).to_string(),
                status: status.as_u16(),
                location: redact_sensitive_location_header(&location, &hop.url),
            });
            hop = validate_redirect_url(&hop.url, &location, resolver, policy, &redirect_chain).await?;
            continue;
        }
        let response_snapshot = CaptureResponseSnapshot {
            http_status: status.as_u16(),
            content_type: header_string(&headers, CONTENT_TYPE),
            content_encoding: header_string(&headers, CONTENT_ENCODING),
            etag: header_string(&headers, ETAG),
            last_modified: header_string(&headers, LAST_MODIFIED),
            remote_addr: Some(remote_addr.to_string()),
        };
        if !status.is_success() {
            return Err(SourceError::CaptureFailed(format!("HTTP status {} is not groundable", status.as_u16())));
        }
        let bytes = read_bounded_body(response).await?;
        final_response = response_snapshot;
        raw_bytes = bytes.to_vec();
        break;
    }

    let captured_at = Utc::now();
    let artifact_id = SourceArtifactId::generate();
    let response = final_response;
    let mut extracted = extract_text(response.content_type.as_deref(), &raw_bytes)?;
    if !extracted.is_supported() {
        return Err(SourceError::Unsupported(
            extracted.unsupported_reason.unwrap_or_else(|| "unsupported content type".to_string()),
        ));
    }
    enforce_extracted_privacy(&extracted.text)?;
    let excerpts = create_excerpt_records(&artifact_id, &extracted.text, &request.excerpts, captured_at)?;
    let raw_text = raw_textual_projection(response.content_type.as_deref(), &raw_bytes);
    let raw_is_safe = raw_text.as_deref().is_some_and(raw_text_is_plaintext_safe);
    let (raw_storage, raw_omitted_reason, raw_bytes_for_storage, capture_status) = if raw_is_safe {
        (RawStorage::Stored, None, Some(raw_bytes.clone()), CaptureStatus::Complete)
    } else {
        (
            RawStorage::OmittedPrivacy,
            Some("raw textual projection is not safe plaintext".to_string()),
            None,
            CaptureStatus::CompleteTextOnly,
        )
    };
    let excerpts_jsonl = excerpts_jsonl(&excerpts)?;
    let raw_zstd_sha256 = if let Some(raw) = &raw_bytes_for_storage {
        Some(sha256_prefixed(zstd::encode_all(raw.as_slice(), 0)?.as_slice()))
    } else {
        None
    };
    let mut warnings = Vec::new();
    warnings.append(&mut extracted.warnings);
    if raw_omitted_reason.is_some() {
        warnings.push("raw_omitted_privacy".to_string());
    }
    let manifest = WebCaptureManifest {
        schema_version: 1,
        artifact_id: artifact_id.clone(),
        kind: "web_capture".to_string(),
        original_url,
        final_url: redact_sensitive_url(&hop.url).to_string(),
        redirect_chain,
        captured_at,
        capture_method: CaptureMethod::HttpStaticV1,
        request: CaptureRequestSnapshot::default(),
        response,
        raw_sha256: Some(sha256_prefixed(&raw_bytes)),
        raw_zstd_sha256,
        raw_storage,
        raw_omitted_reason,
        extracted_text_sha256: sha256_prefixed(extracted.text.as_bytes()),
        excerpts_sha256: sha256_prefixed(excerpts_jsonl.as_bytes()),
        raw_byte_len: raw_bytes.len(),
        extracted_text_byte_len: extracted.text.len(),
        capture_status,
        warnings,
        merge_conflict: None,
    };
    let artifact =
        WebCaptureArtifact { manifest, extracted_text: extracted.text, excerpts, raw_bytes: raw_bytes_for_storage };
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
        capture_status: capture_status_name(artifact.manifest.capture_status).to_string(),
        warnings: artifact.manifest.warnings,
    })
}

fn header_string(headers: &reqwest::header::HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_string)
}

async fn read_bounded_body(mut response: reqwest::Response) -> SourceResult<Vec<u8>> {
    if response.content_length().is_some_and(|length| length > MAX_RAW_BYTES as u64) {
        return Err(SourceError::CaptureFailed(format!("HTTP response exceeded {MAX_RAW_BYTES} bytes")));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) =
        response.chunk().await.map_err(|err| SourceError::CaptureFailed(format!("read HTTP response body: {err}")))?
    {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| SourceError::CaptureFailed("HTTP response size overflow".to_string()))?;
        if next_len > MAX_RAW_BYTES {
            return Err(SourceError::CaptureFailed(format!("HTTP response exceeded {MAX_RAW_BYTES} bytes")));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn enforce_extracted_privacy(text: &str) -> SourceResult<()> {
    let classifier = DeterministicPrivacyClassifier::new();
    let decision = classifier
        .classify(text, PrivacyNamespace::Project, None)
        .map_err(|err| SourceError::privacy(format!("classify extracted text: {err}")))?;
    match decision.storage_action {
        PrivacyStorageAction::Plaintext => Ok(()),
        PrivacyStorageAction::EncryptAtRest => Err(SourceError::privacy("encrypted_source_artifacts_unsupported")),
        PrivacyStorageAction::Refuse => Err(SourceError::privacy("extracted text refused by privacy policy")),
    }
}

fn raw_text_is_plaintext_safe(text: &str) -> bool {
    let classifier = DeterministicPrivacyClassifier::new();
    classifier
        .classify(text, PrivacyNamespace::Project, None)
        .is_ok_and(|decision| matches!(decision.storage_action, PrivacyStorageAction::Plaintext))
}

fn capture_status_name(status: CaptureStatus) -> &'static str {
    match status {
        CaptureStatus::Complete => "complete",
        CaptureStatus::CompleteTextOnly => "complete_text_only",
        CaptureStatus::Partial => "partial",
        CaptureStatus::Failed => "failed",
    }
}
