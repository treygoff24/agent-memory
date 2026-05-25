//! Source capture adapters for transport-specific artifact reads.

use std::io::Read;
use std::path::Path;

use reqwest::header::{ACCEPT, CONTENT_ENCODING, CONTENT_TYPE, ETAG, LAST_MODIFIED, LOCATION, USER_AGENT};

use crate::error::{SourceError, SourceResult};
use crate::model::{CaptureMethod, CaptureMode, CaptureRequestSnapshot, CaptureResponseSnapshot, RedirectHop};
use crate::url_safety::{
    pinned_reqwest_client, redact_sensitive_location_header, redact_sensitive_url, validate_initial_url,
    validate_redirect_url, AddressPolicy, DnsResolver,
};

const MAX_LOCAL_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_HTTP_BYTES: usize = 2 * 1024 * 1024;

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
    pub fn classify(mode: CaptureMode) -> SourceError {
        SourceError::Unsupported(unsupported_mode_guidance(mode).to_string())
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
        CaptureMode::PdfText
        | CaptureMode::BrowserRendered
        | CaptureMode::Screenshot
        | CaptureMode::Authenticated
        | CaptureMode::Unsupported => Err(UnsupportedArtifactAdapter::classify(request.mode)),
    }
}

fn unsupported_mode_guidance(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::PdfText => {
            "pdf_text capture is unsupported in alpha; save/export a text/html artifact and import with --file"
        }
        CaptureMode::BrowserRendered => {
            "browser_rendered capture is unsupported in alpha; save/export a text/html/PDF artifact and import with --file"
        }
        CaptureMode::Screenshot => {
            "screenshot capture is unsupported in alpha; save/export a text/html/PDF artifact and import with --file"
        }
        CaptureMode::Authenticated => {
            "authenticated capture is unsupported in alpha; save/export a text/html/PDF artifact and import with --file"
        }
        CaptureMode::Unsupported | CaptureMode::HttpStatic | CaptureMode::LocalArtifact => {
            "artifact type is unsupported in alpha; save/export a text/html/PDF artifact and import with --file"
        }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpFetchResult {
    pub bytes: Vec<u8>,
    pub original_url: String,
    pub final_url: String,
    pub redirect_chain: Vec<RedirectHop>,
    pub request: CaptureRequestSnapshot,
    pub response: CaptureResponseSnapshot,
}

pub async fn http_fetch(url: &str, resolver: &dyn DnsResolver, policy: AddressPolicy) -> SourceResult<HttpFetchResult> {
    let mut hop = validate_initial_url(url, resolver, policy).await?;
    let request_snapshot = CaptureRequestSnapshot::default();
    let original_url = redact_sensitive_url(&hop.url).to_string();
    let mut redirect_chain = Vec::new();
    loop {
        let client = pinned_reqwest_client(&hop)?;
        let response = client
            .get(hop.url.clone())
            .header(USER_AGENT, request_snapshot.user_agent.as_str())
            .header(ACCEPT, request_snapshot.accept.as_str())
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
        return Ok(HttpFetchResult {
            bytes,
            original_url,
            final_url: redact_sensitive_url(&hop.url).to_string(),
            redirect_chain,
            request: request_snapshot,
            response: response_snapshot,
        });
    }
}

fn header_string(headers: &reqwest::header::HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_string)
}

async fn read_bounded_body(mut response: reqwest::Response) -> SourceResult<Vec<u8>> {
    if response.content_length().is_some_and(|length| length > MAX_HTTP_BYTES as u64) {
        return Err(SourceError::CaptureFailed(format!("HTTP response exceeded {MAX_HTTP_BYTES} bytes")));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) =
        response.chunk().await.map_err(|err| SourceError::CaptureFailed(format!("read HTTP response body: {err}")))?
    {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| SourceError::CaptureFailed("HTTP response size overflow".to_string()))?;
        if next_len > MAX_HTTP_BYTES {
            return Err(SourceError::CaptureFailed(format!("HTTP response exceeded {MAX_HTTP_BYTES} bytes")));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
