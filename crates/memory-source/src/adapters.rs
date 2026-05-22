//! Source capture adapters for transport-specific artifact reads.

use std::io::Read;
use std::path::Path;

use crate::capture::http_fetch;
use crate::error::{SourceError, SourceResult};
use crate::model::{CaptureMethod, CaptureMode, CaptureRequestSnapshot, CaptureResponseSnapshot, RedirectHop};
use crate::url_safety::{AddressPolicy, DnsResolver};

const MAX_LOCAL_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchedArtifact {
    pub bytes: Vec<u8>,
    pub original_ref: String,
    pub final_ref: String,
    pub redirect_chain: Vec<RedirectHop>,
    pub capture_method: CaptureMethod,
    pub request: CaptureRequestSnapshot,
    pub response: CaptureResponseSnapshot,
}

impl FetchedArtifact {
    pub fn content_type(&self) -> Option<&str> {
        self.response.content_type.as_deref()
    }
}

pub struct LocalArtifactAdapter;

impl LocalArtifactAdapter {
    pub fn read(path: &Path) -> SourceResult<FetchedArtifact> {
        let bytes = read_bounded_local_artifact(path)?;
        let source_ref = local_source_ref(path);
        Ok(FetchedArtifact {
            bytes,
            original_ref: source_ref.clone(),
            final_ref: source_ref,
            redirect_chain: Vec::new(),
            capture_method: CaptureMethod::LocalArtifactV1,
            request: CaptureRequestSnapshot {
                method: "LOCAL".to_string(),
                user_agent: "memorum-source-capture/0.1 local-artifact".to_string(),
                accept: infer_content_type(path).unwrap_or_else(|| "application/octet-stream".to_string()),
            },
            response: CaptureResponseSnapshot {
                http_status: 0,
                content_type: infer_content_type(path),
                content_encoding: None,
                etag: None,
                last_modified: None,
                remote_addr: None,
            },
        })
    }
}

pub struct UnsupportedArtifactAdapter;

impl UnsupportedArtifactAdapter {
    pub fn classify() -> SourceError {
        SourceError::Unsupported(
            "artifact type requires a specialized adapter that is not available in this source-capture path"
                .to_string(),
        )
    }
}

#[derive(Clone, Copy)]
pub struct CaptureDispatch<'a> {
    pub mode: CaptureMode,
    pub url: Option<&'a str>,
    pub local_path: Option<&'a Path>,
    pub resolver: &'a dyn DnsResolver,
    pub policy: AddressPolicy,
}

pub async fn dispatch_capture(request: CaptureDispatch<'_>) -> SourceResult<FetchedArtifact> {
    match request.mode {
        CaptureMode::HttpStatic => {
            let url =
                request.url.ok_or_else(|| SourceError::CaptureFailed("http_static mode requires a URL".to_string()))?;
            let fetched = http_fetch(url, request.resolver, request.policy).await?;
            Ok(FetchedArtifact {
                bytes: fetched.bytes,
                original_ref: fetched.original_url,
                final_ref: fetched.final_url,
                redirect_chain: fetched.redirect_chain,
                capture_method: CaptureMethod::HttpStaticV1,
                request: fetched.request,
                response: fetched.response,
            })
        }
        CaptureMode::LocalArtifact => {
            let path = request
                .local_path
                .ok_or_else(|| SourceError::CaptureFailed("local_artifact mode requires a local path".to_string()))?;
            LocalArtifactAdapter::read(path)
        }
        CaptureMode::Unsupported => Err(UnsupportedArtifactAdapter::classify()),
    }
}

fn local_source_ref(_path: &Path) -> String {
    "local:artifact".to_string()
}

fn read_bounded_local_artifact(path: &Path) -> SourceResult<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    let read = file.take(MAX_LOCAL_ARTIFACT_BYTES + 1).read_to_end(&mut bytes)?;
    if read as u64 > MAX_LOCAL_ARTIFACT_BYTES {
        return Err(SourceError::CaptureFailed(format!("local artifact exceeded {MAX_LOCAL_ARTIFACT_BYTES} bytes")));
    }
    Ok(bytes)
}

fn infer_content_type(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let content_type = match extension.as_str() {
        "txt" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "xhtml" => "application/xhtml+xml",
        _ => return None,
    };
    Some(content_type.to_string())
}
