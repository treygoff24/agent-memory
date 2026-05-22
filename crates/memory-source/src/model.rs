use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{SourceError, SourceResult};

const ARTIFACT_PREFIX: &str = "src_";
const SOURCE_REF_PREFIX: &str = "webcap:";
const ULID_LEN: usize = 26;
pub const WEB_CAPTURE_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceArtifactId(String);

impl SourceArtifactId {
    pub fn try_new(value: impl Into<String>) -> SourceResult<Self> {
        let value = value.into();
        let Some(body) = value.strip_prefix(ARTIFACT_PREFIX) else {
            return Err(SourceError::InvalidId(value));
        };
        if body.len() != ULID_LEN || !body.bytes().all(is_crockford_ulid_byte) {
            return Err(SourceError::InvalidId(value));
        }
        Ok(Self(value))
    }

    pub fn generate() -> Self {
        Self(format!("{ARTIFACT_PREFIX}{}", ulid::Ulid::new()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SourceArtifactId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn is_crockford_ulid_byte(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebCaptureManifest {
    pub schema_version: u32,
    pub artifact_id: SourceArtifactId,
    pub kind: String,
    pub original_url: String,
    pub final_url: String,
    pub redirect_chain: Vec<RedirectHop>,
    pub captured_at: DateTime<Utc>,
    pub capture_method: CaptureMethod,
    pub request: CaptureRequestSnapshot,
    pub response: CaptureResponseSnapshot,
    pub raw_sha256: Option<String>,
    pub raw_zstd_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_encrypted_sha256: Option<String>,
    pub raw_storage: RawStorage,
    pub raw_omitted_reason: Option<String>,
    #[serde(default, skip_serializing_if = "is_plaintext_extracted_storage")]
    pub extracted_text_storage: ExtractedTextStorage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_envelope: Option<EncryptionEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_text_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_text_encrypted_sha256: Option<String>,
    pub excerpts_sha256: String,
    pub raw_byte_len: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_text_byte_len: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_text_encrypted_byte_len: Option<usize>,
    pub capture_status: CaptureStatus,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_conflict: Option<serde_json::Value>,
}

impl WebCaptureManifest {
    pub fn is_groundable(&self) -> bool {
        matches!(self.capture_status, CaptureStatus::Complete | CaptureStatus::CompleteTextOnly)
            && matches!(self.capture_method, CaptureMethod::HttpStaticV1 | CaptureMethod::LocalArtifactV1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RedirectHop {
    pub url: String,
    pub status: u16,
    pub location: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CaptureRequestSnapshot {
    pub method: String,
    pub user_agent: String,
    pub accept: String,
}

impl Default for CaptureRequestSnapshot {
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            user_agent: "memorum-source-capture/0.1".to_string(),
            accept: "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.1".to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CaptureResponseSnapshot {
    pub http_status: u16,
    pub content_type: Option<String>,
    pub content_encoding: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub remote_addr: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Complete,
    CompleteTextOnly,
    Partial,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    HttpStaticV1,
    LocalArtifactV1,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    HttpStatic,
    LocalArtifact,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractedTextStorage {
    #[default]
    Plaintext,
    Encrypted,
}

fn is_plaintext_extracted_storage(storage: &ExtractedTextStorage) -> bool {
    matches!(storage, ExtractedTextStorage::Plaintext)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncryptionEnvelope {
    pub scheme: String,
    pub recipient: String,
}

impl EncryptionEnvelope {
    pub fn age_x25519(recipient: impl Into<String>) -> Self {
        Self { scheme: "age-x25519".to_string(), recipient: recipient.into() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawStorage {
    Stored,
    Encrypted,
    OmittedPrivacy,
    OmittedUnsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExcerptRecord {
    pub excerpt_id: String,
    pub artifact_id: SourceArtifactId,
    pub quote: String,
    pub quote_sha256: String,
    pub locator: ExcerptLocator,
    pub match_kind: ExcerptMatchKind,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExcerptLocator {
    ByteRange { start: usize, end: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcerptMatchKind {
    Exact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebCaptureSourceRef {
    artifact_id: SourceArtifactId,
    excerpt_id: String,
}

impl WebCaptureSourceRef {
    pub fn parse(value: &str) -> SourceResult<Self> {
        let Some(rest) = value.strip_prefix(SOURCE_REF_PREFIX) else {
            return Err(SourceError::InvalidSourceRef(value.to_string()));
        };
        let Some((artifact, excerpt_id)) = rest.split_once('#') else {
            return Err(SourceError::InvalidSourceRef(value.to_string()));
        };
        if excerpt_id.is_empty() || excerpt_id.contains('/') || excerpt_id.contains("..") {
            return Err(SourceError::InvalidSourceRef(value.to_string()));
        }
        Ok(Self { artifact_id: SourceArtifactId::try_new(artifact.to_string())?, excerpt_id: excerpt_id.to_string() })
    }

    pub fn new(artifact_id: SourceArtifactId, excerpt_id: impl Into<String>) -> Self {
        Self { artifact_id, excerpt_id: excerpt_id.into() }
    }

    pub fn artifact_id(&self) -> &SourceArtifactId {
        &self.artifact_id
    }

    pub fn excerpt_id(&self) -> &str {
        &self.excerpt_id
    }
}

impl std::fmt::Display for WebCaptureSourceRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{SOURCE_REF_PREFIX}{}#{}", self.artifact_id, self.excerpt_id)
    }
}
