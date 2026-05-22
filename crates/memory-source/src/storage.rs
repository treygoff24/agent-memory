use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Utc};

use crate::error::{SourceError, SourceResult};
use crate::excerpt::verify_excerpt_anchor;
use crate::hash::sha256_prefixed;
use crate::model::{
    ExcerptRecord, ExtractedTextStorage, RawStorage, SourceArtifactId, WebCaptureManifest, WebCaptureSourceRef,
};

#[derive(Clone, Debug)]
pub struct ArtifactStore {
    repo_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceArtifactPath {
    relative: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebCaptureArtifact {
    pub manifest: WebCaptureManifest,
    pub extracted_text: String,
    pub excerpts: Vec<ExcerptRecord>,
    pub raw_bytes: Option<Vec<u8>>,
    pub encrypted_extracted_bytes: Option<Vec<u8>>,
    pub encrypted_raw_bytes: Option<Vec<u8>>,
}

impl ArtifactStore {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self { repo_root: repo_root.into() }
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn source_artifact_path(
        &self,
        artifact_id: &SourceArtifactId,
        captured_at: chrono::DateTime<Utc>,
    ) -> SourceArtifactPath {
        SourceArtifactPath::new(captured_at.year(), captured_at.month(), artifact_id)
            .expect("generated source artifact path is safe")
    }

    pub fn find_artifact_path(&self, artifact_id: &SourceArtifactId) -> SourceResult<SourceArtifactPath> {
        let root = self.repo_root.join("sources/web");
        if !root.exists() {
            return Err(SourceError::Integrity(format!("artifact {} is missing", artifact_id)));
        }
        for entry in walk_dirs(&root)? {
            if entry.file_name().and_then(|name| name.to_str()) == Some(artifact_id.as_str()) {
                let relative =
                    entry.strip_prefix(&self.repo_root).map_err(|err| SourceError::integrity(err.to_string()))?;
                return SourceArtifactPath::from_relative(relative.to_path_buf());
            }
        }
        Err(SourceError::Integrity(format!("artifact {} is missing", artifact_id)))
    }

    pub fn write_web_capture(&self, artifact: &WebCaptureArtifact) -> SourceResult<SourceArtifactPath> {
        let artifact_path = self.source_artifact_path(&artifact.manifest.artifact_id, artifact.manifest.captured_at);
        let final_dir = self.repo_root.join(artifact_path.relative());
        let parent = final_dir.parent().ok_or_else(|| SourceError::integrity("artifact path has no parent"))?;
        fs::create_dir_all(parent)?;
        let tmp_dir = parent.join(format!(".tmp-{}-{}", artifact.manifest.artifact_id, std::process::id()));
        if tmp_dir.exists() {
            fs::remove_dir_all(&tmp_dir)?;
        }
        fs::create_dir_all(&tmp_dir)?;
        let result = (|| -> SourceResult<()> {
            match artifact.manifest.extracted_text_storage {
                ExtractedTextStorage::Plaintext => {
                    fs::write(tmp_dir.join("extracted.txt"), artifact.extracted_text.as_bytes())?;
                }
                ExtractedTextStorage::Encrypted => {
                    let ciphertext = artifact
                        .encrypted_extracted_bytes
                        .as_ref()
                        .ok_or_else(|| SourceError::integrity("extracted_text_storage=encrypted without ciphertext"))?;
                    fs::write(tmp_dir.join("extracted.enc.age"), ciphertext)?;
                }
            }
            let excerpts_jsonl = excerpts_jsonl(&artifact.excerpts)?;
            fs::write(tmp_dir.join("excerpts.jsonl"), excerpts_jsonl.as_bytes())?;
            match artifact.manifest.raw_storage {
                RawStorage::Stored => {
                    let raw = artifact
                        .raw_bytes
                        .as_ref()
                        .ok_or_else(|| SourceError::integrity("raw_storage=stored without raw bytes"))?;
                    let compressed = zstd::encode_all(raw.as_slice(), 0)?;
                    fs::write(tmp_dir.join("raw.bin.zst"), compressed)?;
                }
                RawStorage::Encrypted => {
                    let ciphertext = artifact
                        .encrypted_raw_bytes
                        .as_ref()
                        .ok_or_else(|| SourceError::integrity("raw_storage=encrypted without ciphertext"))?;
                    fs::write(tmp_dir.join("raw.enc.age"), ciphertext)?;
                }
                RawStorage::OmittedPrivacy | RawStorage::OmittedUnsupported => {}
            }
            let manifest = serde_json::to_vec_pretty(&artifact.manifest)?;
            fs::File::create(tmp_dir.join("manifest.json"))?.write_all(&manifest)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err(error);
        }
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir)?;
        }
        fs::rename(tmp_dir, &final_dir)?;
        self.verify_web_capture(&artifact_path)?;
        Ok(artifact_path)
    }

    pub fn verify_web_capture(&self, artifact_path: &SourceArtifactPath) -> SourceResult<WebCaptureArtifact> {
        let dir = self.repo_root.join(artifact_path.relative());
        let manifest: WebCaptureManifest = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?;
        if !manifest.is_groundable() {
            return Err(SourceError::integrity(format!("artifact {} is not groundable", manifest.artifact_id)));
        }
        let (extracted_text, encrypted_extracted_bytes) = match manifest.extracted_text_storage {
            ExtractedTextStorage::Plaintext => {
                let extracted_text = fs::read_to_string(dir.join("extracted.txt"))?;
                if manifest.extracted_text_sha256.as_deref()
                    != Some(sha256_prefixed(extracted_text.as_bytes()).as_str())
                {
                    return Err(SourceError::integrity("extracted.txt hash mismatch"));
                }
                if manifest.extracted_text_byte_len != Some(extracted_text.len()) {
                    return Err(SourceError::integrity("extracted.txt byte length mismatch"));
                }
                (extracted_text, None)
            }
            ExtractedTextStorage::Encrypted => {
                let ciphertext = fs::read(dir.join("extracted.enc.age"))?;
                let Some(expected_hash) = manifest.extracted_text_encrypted_sha256.as_deref() else {
                    return Err(SourceError::integrity(
                        "extracted_text_storage=encrypted missing extracted_text_encrypted_sha256",
                    ));
                };
                if expected_hash != sha256_prefixed(&ciphertext) {
                    return Err(SourceError::integrity("extracted.enc.age hash mismatch"));
                }
                if manifest.extracted_text_encrypted_byte_len != Some(ciphertext.len()) {
                    return Err(SourceError::integrity("extracted.enc.age byte length mismatch"));
                }
                if manifest.encryption_envelope.is_none() {
                    return Err(SourceError::integrity("encrypted extracted text missing encryption envelope"));
                }
                validate_age_ciphertext(&ciphertext, "extracted.enc.age")?;
                (String::new(), Some(ciphertext))
            }
        };
        let excerpts_bytes = fs::read(dir.join("excerpts.jsonl"))?;
        if manifest.excerpts_sha256 != sha256_prefixed(&excerpts_bytes) {
            return Err(SourceError::integrity("excerpts.jsonl hash mismatch"));
        }
        let excerpts = parse_excerpts_jsonl(&excerpts_bytes)?;
        for record in &excerpts {
            if record.artifact_id != manifest.artifact_id {
                return Err(SourceError::integrity("excerpt artifact id mismatch"));
            }
            if matches!(manifest.extracted_text_storage, ExtractedTextStorage::Plaintext) {
                verify_excerpt_anchor(&extracted_text, record)?;
            }
        }
        let (raw_bytes, encrypted_raw_bytes) = match manifest.raw_storage {
            RawStorage::Stored => {
                let compressed = fs::read(dir.join("raw.bin.zst"))?;
                if manifest.raw_zstd_sha256.as_deref() != Some(sha256_prefixed(&compressed).as_str()) {
                    return Err(SourceError::integrity("raw.bin.zst hash mismatch"));
                }
                let raw = zstd::decode_all(compressed.as_slice())?;
                if manifest.raw_sha256.as_deref() != Some(sha256_prefixed(&raw).as_str()) {
                    return Err(SourceError::integrity("raw bytes hash mismatch"));
                }
                if manifest.raw_byte_len != raw.len() {
                    return Err(SourceError::integrity("raw byte length mismatch"));
                }
                (Some(raw), None)
            }
            RawStorage::Encrypted => {
                let ciphertext = fs::read(dir.join("raw.enc.age"))?;
                let Some(expected_hash) = manifest.raw_encrypted_sha256.as_deref() else {
                    return Err(SourceError::integrity("raw_storage=encrypted missing raw_encrypted_sha256"));
                };
                if expected_hash != sha256_prefixed(&ciphertext) {
                    return Err(SourceError::integrity("raw.enc.age hash mismatch"));
                }
                if manifest.encryption_envelope.is_none() {
                    return Err(SourceError::integrity("encrypted raw missing encryption envelope"));
                }
                validate_age_ciphertext(&ciphertext, "raw.enc.age")?;
                (None, Some(ciphertext))
            }
            RawStorage::OmittedPrivacy | RawStorage::OmittedUnsupported => (None, None),
        };
        Ok(WebCaptureArtifact {
            manifest,
            extracted_text,
            excerpts,
            raw_bytes,
            encrypted_extracted_bytes,
            encrypted_raw_bytes,
        })
    }

    pub fn verify_artifact_id(&self, artifact_id: &SourceArtifactId) -> SourceResult<WebCaptureArtifact> {
        let path = self.find_artifact_path(artifact_id)?;
        self.verify_web_capture(&path)
    }

    pub fn resolve_excerpt_ref(&self, source_ref: &str) -> SourceResult<ExcerptRecord> {
        let parsed = WebCaptureSourceRef::parse(source_ref)?;
        let artifact = self.verify_artifact_id(parsed.artifact_id())?;
        artifact
            .excerpts
            .into_iter()
            .find(|record| record.excerpt_id == parsed.excerpt_id())
            .ok_or_else(|| SourceError::ExcerptNotFound(parsed.excerpt_id().to_string()))
    }
}

fn validate_age_ciphertext(ciphertext: &[u8], file_name: &str) -> SourceResult<()> {
    if ciphertext.starts_with(b"age-encryption.org/v1") {
        return Ok(());
    }
    Err(SourceError::integrity(format!("{file_name} is not an age ciphertext")))
}

impl SourceArtifactPath {
    pub fn new(year: i32, month: u32, artifact_id: &SourceArtifactId) -> SourceResult<Self> {
        if !(1..=12).contains(&month) {
            return Err(SourceError::integrity("invalid source artifact month"));
        }
        Self::from_relative(PathBuf::from(format!("sources/web/{year:04}/{month:02}/{artifact_id}")))
    }

    pub fn from_relative(relative: PathBuf) -> SourceResult<Self> {
        if relative.is_absolute() {
            return Err(SourceError::integrity("source artifact path must be relative"));
        }
        for component in relative.components() {
            match component {
                std::path::Component::Normal(part) if part.to_string_lossy().contains(std::path::MAIN_SEPARATOR) => {
                    return Err(SourceError::integrity("unsafe source artifact path segment"));
                }
                std::path::Component::Normal(_) => {}
                _ => return Err(SourceError::integrity("unsafe source artifact path component")),
            }
        }
        Ok(Self { relative })
    }

    pub fn relative(&self) -> &Path {
        &self.relative
    }
}

pub fn excerpts_jsonl(excerpts: &[ExcerptRecord]) -> SourceResult<String> {
    let mut out = String::new();
    for record in excerpts {
        out.push_str(&serde_json::to_string(record)?);
        out.push('\n');
    }
    Ok(out)
}

pub fn parse_excerpts_jsonl(bytes: &[u8]) -> SourceResult<Vec<ExcerptRecord>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| SourceError::integrity(format!("excerpts.jsonl is not utf8: {err}")))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(SourceError::from))
        .collect()
}

fn walk_dirs(root: &Path) -> SourceResult<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    fn visit(path: &Path, dirs: &mut Vec<PathBuf>) -> SourceResult<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                dirs.push(path.clone());
                visit(&path, dirs)?;
            }
        }
        Ok(())
    }
    visit(root, &mut dirs)?;
    Ok(dirs)
}
