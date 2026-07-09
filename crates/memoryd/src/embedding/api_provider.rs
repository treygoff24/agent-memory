//! API-served Gemini embedding lane.
//!
//! This provider is additive and opt-in: local Qwen3 remains the default. The
//! API key is resolved from device-local runtime state, never the synced repo
//! config, and the sync HTTP client is intended to run behind the existing
//! `spawn_blocking` embedding call sites.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use memory_substrate::EmbeddingTriple;
use reqwest::blocking::Client;
use reqwest::header::RETRY_AFTER;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::paths::gemini_api_key_path;

use super::{check_dimension, EmbeddingError, EmbeddingProvider};

pub const GEMINI_API_PROVIDER: &str = "gemini-api";
/// Default Gemini model selected by the opt-in API embedding lane.
pub const GEMINI_API_DEFAULT_MODEL_REF: &str = "gemini-embedding-2";
/// Recommended output dimension for the default Gemini embedding model.
pub const GEMINI_API_RECOMMENDED_DIMENSION: u32 = 768;
const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const GEMINI_EMBED_CONTENT_ENDPOINT: &str = ":embedContent";
const GEMINI_BATCH_EMBED_CONTENTS_ENDPOINT: &str = ":batchEmbedContents";
const GEMINI_MODEL_RESOURCE_PREFIX: &str = "models/";
const GEMINI_OUTPUT_DIMENSIONALITY_FIELD: &str = "output_dimensionality";
const GEMINI_API_KEY_ENV: &str = "MEMORUM_GEMINI_API_KEY";
const GEMINI_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const GEMINI_TOTAL_TIMEOUT: Duration = Duration::from_secs(8);

// The Gemini batchEmbedContents schema does not currently publish a numeric
// item maximum. Cap at 100 requests conservatively and validate during T4.1.
const GEMINI_BATCH_MAX_REQUESTS: usize = 100;

// Gemini asymmetric-retrieval prefixes. These are subject to the T4.1 bake-off.
const GEMINI_QUERY_PREFIX: &str = "task: search result | query: ";
const GEMINI_DOCUMENT_PREFIX: &str = "title: none | text: ";

/// Whether an active embedding triple belongs to the Gemini API lane.
pub fn is_gemini_api_triple(triple: &EmbeddingTriple) -> bool {
    triple.provider == GEMINI_API_PROVIDER
}

/// Device-local API key wrapper. Debug output intentionally never reveals the
/// secret.
#[derive(Clone, Eq, PartialEq)]
pub struct ApiKey(String);

impl ApiKey {
    fn from_raw(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| Self(trimmed.to_string()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

/// Read the runtime-state Gemini API key, trimming surrounding whitespace.
pub fn read_gemini_api_key(runtime_root: &Path) -> Result<Option<ApiKey>, EmbeddingError> {
    let path = gemini_api_key_path(runtime_root);
    match fs::read_to_string(&path) {
        Ok(contents) => Ok(ApiKey::from_raw(&contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(EmbeddingError::Auth(format!("read Gemini API key {}: {error}", path.display()))),
    }
}

/// Write the runtime-state Gemini API key with owner-only permissions on Unix.
pub fn write_gemini_api_key(runtime_root: &Path, key: &str) -> Result<(), EmbeddingError> {
    fs::create_dir_all(runtime_root).map_err(|error| {
        EmbeddingError::Auth(format!("create runtime directory {}: {error}", runtime_root.display()))
    })?;
    let path = gemini_api_key_path(runtime_root);
    reject_key_symlink(&path)?;

    let mut options = OpenOptions::new();
    options.create(true).write(true);
    set_owner_only_create_mode(&mut options);

    let mut file = options
        .open(&path)
        .map_err(|error| EmbeddingError::Auth(format!("write Gemini API key {}: {error}", path.display())))?;
    verify_opened_key_path(&file, &path)?;
    enforce_owner_only_permissions(&file, &path)?;
    file.set_len(0)
        .map_err(|error| EmbeddingError::Auth(format!("truncate Gemini API key {}: {error}", path.display())))?;
    let trimmed = key.trim();
    file.write_all(trimmed.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| EmbeddingError::Auth(format!("write Gemini API key {}: {error}", path.display())))?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_create_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_owner_only_create_mode(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn reject_key_symlink(path: &Path) -> Result<(), EmbeddingError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(EmbeddingError::Auth(format!("refusing to write Gemini API key through symlink {}", path.display())))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(EmbeddingError::Auth(format!("inspect Gemini API key path {}: {error}", path.display()))),
    }
}

#[cfg(not(unix))]
fn reject_key_symlink(_path: &Path) -> Result<(), EmbeddingError> {
    Ok(())
}

#[cfg(unix)]
fn verify_opened_key_path(file: &File, path: &Path) -> Result<(), EmbeddingError> {
    use std::os::unix::fs::MetadataExt;

    let opened = file
        .metadata()
        .map_err(|error| EmbeddingError::Auth(format!("inspect opened Gemini API key {}: {error}", path.display())))?;
    let linked = fs::symlink_metadata(path)
        .map_err(|error| EmbeddingError::Auth(format!("inspect Gemini API key path {}: {error}", path.display())))?;
    if linked.file_type().is_symlink() || opened.dev() != linked.dev() || opened.ino() != linked.ino() {
        return Err(EmbeddingError::Auth(format!(
            "Gemini API key path changed or became a symlink while opening {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_opened_key_path(_file: &File, _path: &Path) -> Result<(), EmbeddingError> {
    Ok(())
}

fn enforce_owner_only_permissions(file: &File, path: &Path) -> Result<(), EmbeddingError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600)).map_err(|error| {
            EmbeddingError::Auth(format!("set Gemini API key permissions {}: {error}", path.display()))
        })?;
    }
    #[cfg(not(unix))]
    let _ = (file, path);
    Ok(())
}

/// Blocking Gemini API embedding provider.
pub struct ApiEmbeddingProvider {
    triple: EmbeddingTriple,
    client: Client,
    api_key: ApiKey,
    model_ref: String,
    base_url: String,
}

impl fmt::Debug for ApiEmbeddingProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiEmbeddingProvider")
            .field("triple", &self.triple)
            .field("api_key", &self.api_key)
            .field("model_ref", &self.model_ref)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl ApiEmbeddingProvider {
    /// Construct the API provider without making a network call. Credential
    /// presence is checked at load time so missing-key slots fail cleanly once.
    pub fn load_for_runtime(runtime_root: &Path, triple: EmbeddingTriple) -> Result<Self, EmbeddingError> {
        let api_key = resolve_gemini_api_key(runtime_root)?.ok_or_else(|| {
            EmbeddingError::Auth(format!(
                "Gemini API key not found; set {GEMINI_API_KEY_ENV} or write {}",
                gemini_api_key_path(runtime_root).display()
            ))
        })?;
        Self::new(triple, api_key, GEMINI_API_BASE_URL.to_string())
    }

    fn new(triple: EmbeddingTriple, api_key: ApiKey, base_url: String) -> Result<Self, EmbeddingError> {
        let client = Client::builder()
            .connect_timeout(GEMINI_CONNECT_TIMEOUT)
            .timeout(GEMINI_TOTAL_TIMEOUT)
            .build()
            .map_err(|error| EmbeddingError::Transport(format!("build Gemini API client: {error}")))?;
        let model_ref = triple.model_ref.clone();
        Ok(Self { triple, client, api_key, model_ref, base_url })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        triple: EmbeddingTriple,
        api_key: &str,
        base_url: String,
    ) -> Result<Self, EmbeddingError> {
        let api_key =
            ApiKey::from_raw(api_key).ok_or_else(|| EmbeddingError::Auth("test Gemini API key is empty".into()))?;
        Self::new(triple, api_key, base_url)
    }

    fn embed_prefixed(&self, text: String) -> Result<Vec<f32>, EmbeddingError> {
        let embeddings = self.post_embeddings(
            GEMINI_EMBED_CONTENT_ENDPOINT,
            embedding_request(self.model_resource(), text, self.triple.dimension),
            1,
        )?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::Contract("Gemini embedContent returned no embeddings".into()))
    }

    fn post_embeddings(
        &self,
        endpoint: &str,
        body: Value,
        expected_count: usize,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let response = self
            .client
            .post(self.endpoint_url(endpoint))
            .header("x-goog-api-key", self.api_key.as_str())
            .json(&body)
            .send()
            .map_err(|error| EmbeddingError::Transport(error.to_string()))?;
        let status = response.status();
        let retry_after = parse_retry_after(response.headers().get(RETRY_AFTER));
        let body = response.text().map_err(|error| EmbeddingError::Transport(error.to_string()))?;
        if !status.is_success() {
            return Err(map_http_error(status, retry_after, &body));
        }
        let vectors = parse_embedding_response(&body, expected_count)?;
        check_vectors(&self.triple, vectors)
    }

    fn endpoint_url(&self, endpoint: &str) -> String {
        format!(
            "{}/models/{}{}",
            self.base_url.trim_end_matches('/'),
            self.model_ref.trim_start_matches(GEMINI_MODEL_RESOURCE_PREFIX),
            endpoint
        )
    }

    fn model_resource(&self) -> String {
        if self.model_ref.starts_with(GEMINI_MODEL_RESOURCE_PREFIX) {
            self.model_ref.clone()
        } else {
            format!("{GEMINI_MODEL_RESOURCE_PREFIX}{}", self.model_ref)
        }
    }
}

impl EmbeddingProvider for ApiEmbeddingProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed_prefixed(format!("{GEMINI_QUERY_PREFIX}{text}"))
    }

    fn embed_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed_prefixed(format!("{GEMINI_DOCUMENT_PREFIX}{text}"))
    }

    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut embeddings = Vec::with_capacity(texts.len());
        for batch in texts.chunks(GEMINI_BATCH_MAX_REQUESTS) {
            let requests: Vec<Value> = batch
                .iter()
                .map(|text| {
                    embedding_request(
                        self.model_resource(),
                        format!("{GEMINI_DOCUMENT_PREFIX}{text}"),
                        self.triple.dimension,
                    )
                })
                .collect();
            embeddings.extend(self.post_embeddings(
                GEMINI_BATCH_EMBED_CONTENTS_ENDPOINT,
                json!({ "requests": requests }),
                batch.len(),
            )?);
        }
        Ok(embeddings)
    }
}

fn resolve_gemini_api_key(runtime_root: &Path) -> Result<Option<ApiKey>, EmbeddingError> {
    if let Ok(raw) = std::env::var(GEMINI_API_KEY_ENV) {
        if let Some(key) = ApiKey::from_raw(&raw) {
            return Ok(Some(key));
        }
    }
    read_gemini_api_key(runtime_root)
}

fn content_part(text: String) -> Value {
    json!({ "parts": [{ "text": text }] })
}

fn embedding_request(model: String, text: String, dimension: u32) -> Value {
    let mut request = serde_json::Map::new();
    request.insert("model".to_string(), json!(model));
    request.insert("content".to_string(), content_part(text));
    request.insert(GEMINI_OUTPUT_DIMENSIONALITY_FIELD.to_string(), json!(dimension));
    Value::Object(request)
}

#[derive(Deserialize)]
struct GeminiEmbeddingsResponse {
    embeddings: Vec<GeminiEmbedding>,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

fn parse_embedding_response(body: &str, expected_count: usize) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let response: GeminiEmbeddingsResponse = serde_json::from_str(body)
        .map_err(|error| EmbeddingError::Contract(format!("parse Gemini embedding response: {error}")))?;
    if response.embeddings.len() != expected_count {
        return Err(EmbeddingError::Contract(format!(
            "Gemini returned {} embeddings for {expected_count} inputs",
            response.embeddings.len()
        )));
    }
    let vectors: Vec<Vec<f32>> = response.embeddings.into_iter().map(|embedding| embedding.values).collect();
    Ok(vectors)
}

fn parse_retry_after(value: Option<&reqwest::header::HeaderValue>) -> Option<Duration> {
    let raw = value?.to_str().ok()?.trim();
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let now = chrono::Utc::now();
    let duration = date.with_timezone(&chrono::Utc).signed_duration_since(now);
    duration.to_std().ok().or(Some(Duration::ZERO))
}

fn map_http_error(status: StatusCode, retry_after: Option<Duration>, body: &str) -> EmbeddingError {
    let message = format!("HTTP {status}: {}", remote_error_message(body));
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => EmbeddingError::Auth(message),
        StatusCode::TOO_MANY_REQUESTS => EmbeddingError::RateLimit { retry_after, message },
        StatusCode::BAD_REQUEST if bad_request_is_auth(body) => EmbeddingError::Auth(message),
        StatusCode::BAD_REQUEST => EmbeddingError::Contract(message),
        _ => EmbeddingError::Transport(message),
    }
}

fn bad_request_is_auth(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let status = value.pointer("/error/status").and_then(Value::as_str).unwrap_or_default();
    let message = value.pointer("/error/message").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    matches!(status, "API_KEY_INVALID" | "PERMISSION_DENIED")
        || message.contains("api key not valid")
        || message.contains("api_key_invalid")
}

fn remote_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.pointer("/error/message").and_then(Value::as_str).map(ToOwned::to_owned))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

fn check_vectors(triple: &EmbeddingTriple, vectors: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    for vector in &vectors {
        check_dimension(triple, vector)?;
    }
    Ok(vectors)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};

    #[derive(Clone, Debug)]
    pub(crate) struct RecordedRequest {
        pub(crate) method: String,
        pub(crate) path: String,
        pub(crate) body: String,
    }

    #[derive(Clone)]
    pub(crate) struct MockResponse {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: String,
    }

    impl MockResponse {
        pub(crate) fn json(status: u16, body: String) -> Self {
            Self { status, headers: vec![("Content-Type", "application/json")], body }
        }

        pub(crate) fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
            self.headers.push((name, value));
            self
        }
    }

    pub(crate) struct MockGeminiServer {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        shutdown: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl MockGeminiServer {
        pub(crate) fn new(responses: Vec<MockResponse>) -> Self {
            Self::spawn(responses, false)
        }

        pub(crate) fn panic_on_any_request() -> Self {
            Self::spawn(Vec::new(), true)
        }

        fn spawn(responses: Vec<MockResponse>, panic_on_request: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
            listener.set_nonblocking(true).expect("nonblocking listener");
            let addr = listener.local_addr().expect("mock addr");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let shutdown = Arc::new(AtomicBool::new(false));
            let thread_requests = Arc::clone(&requests);
            let thread_shutdown = Arc::clone(&shutdown);
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
            let thread_responses = Arc::clone(&responses);
            let handle = thread::spawn(move || {
                while !thread_shutdown.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_connection(stream, &thread_requests, &thread_responses, panic_on_request)
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("mock server accept failed: {error}"),
                    }
                }
            });
            Self { addr, requests, shutdown, handle: Some(handle) }
        }

        pub(crate) fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        pub(crate) fn requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().expect("requests lock").clone()
        }
    }

    impl Drop for MockGeminiServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr);
            if let Some(handle) = self.handle.take() {
                if let Err(payload) = handle.join() {
                    if !thread::panicking() {
                        panic!("mock Gemini server thread panicked: {}", panic_payload(payload));
                    }
                }
            }
        }
    }

    fn handle_connection(
        stream: TcpStream,
        requests: &Arc<Mutex<Vec<RecordedRequest>>>,
        responses: &Arc<Mutex<VecDeque<MockResponse>>>,
        panic_on_request: bool,
    ) {
        // The listener is non-blocking for shutdown polling; on macOS/BSD accepted
        // streams inherit that flag, so large request bodies that span packets
        // would fail reads with WouldBlock. Restore blocking mode per-connection.
        stream.set_nonblocking(false).expect("blocking stream");
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).expect("read request line") == 0 {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read header");
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:").or_else(|| line.strip_prefix("content-length:")) {
                content_length = value.trim().parse().expect("content length");
            }
        }
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).expect("read body");
        let recorded = RecordedRequest { method, path, body: String::from_utf8(body).expect("utf8 body") };
        requests.lock().expect("requests lock").push(recorded.clone());
        assert!(!panic_on_request, "unexpected request to mock Gemini server: {recorded:?}");

        let response = responses.lock().expect("responses lock").pop_front().unwrap_or_else(|| {
            MockResponse::json(500, json!({ "error": { "message": "unexpected request" } }).to_string())
        });
        write_response(reader.get_mut(), response);
    }

    fn write_response(stream: &mut TcpStream, response: MockResponse) {
        let reason = match response.status {
            200 => "OK",
            401 => "Unauthorized",
            429 => "Too Many Requests",
            _ => "Error",
        };
        write!(stream, "HTTP/1.1 {} {}\r\n", response.status, reason).expect("status");
        write!(stream, "Content-Length: {}\r\n", response.body.len()).expect("content length");
        for (name, value) in response.headers {
            write!(stream, "{name}: {value}\r\n").expect("header");
        }
        write!(stream, "Connection: close\r\n\r\n{}", response.body).expect("body");
    }

    fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
        if let Some(message) = payload.downcast_ref::<&str>() {
            (*message).to_string()
        } else if let Some(message) = payload.downcast_ref::<String>() {
            message.clone()
        } else {
            "unknown panic payload".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{MockGeminiServer, MockResponse};
    use super::*;
    use serial_test::serial;

    fn triple(dimension: u32) -> EmbeddingTriple {
        EmbeddingTriple {
            provider: GEMINI_API_PROVIDER.to_string(),
            model_ref: "gemini-embedding-2".to_string(),
            dimension,
        }
    }

    fn provider(base_url: String, dimension: u32) -> ApiEmbeddingProvider {
        ApiEmbeddingProvider::new_for_test(triple(dimension), "test-api-key", base_url).expect("provider")
    }

    fn embedding_response(vectors: Vec<Vec<f32>>) -> String {
        json!({
            "embeddings": vectors.into_iter().map(|values| json!({ "values": values })).collect::<Vec<_>>()
        })
        .to_string()
    }

    fn request_body(server: &MockGeminiServer) -> Value {
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        serde_json::from_str(&requests[0].body).expect("request json")
    }

    #[test]
    fn gemini_lane_accepts_only_gemini_provider() {
        assert!(is_gemini_api_triple(&triple(768)));
        assert!(!is_gemini_api_triple(&EmbeddingTriple {
            provider: "fastembed-candle".to_string(),
            model_ref: "gemini-embedding-2".to_string(),
            dimension: 768,
        }));
    }

    #[test]
    fn query_prefix_is_applied() {
        let server = MockGeminiServer::new(vec![MockResponse::json(200, embedding_response(vec![vec![1.0, 2.0]]))]);
        let vector = provider(server.base_url(), 2).embed_query("find alpha").expect("embed query");

        assert_eq!(vector, vec![1.0, 2.0]);
        let body = request_body(&server);
        assert_eq!(body["model"], "models/gemini-embedding-2");
        assert_eq!(body[GEMINI_OUTPUT_DIMENSIONALITY_FIELD], 2);
        assert_eq!(body["content"]["parts"][0]["text"], format!("{GEMINI_QUERY_PREFIX}find alpha"));
        assert_eq!(server.requests()[0].method, "POST");
        assert_eq!(server.requests()[0].path, "/models/gemini-embedding-2:embedContent");
    }

    #[test]
    fn document_prefix_is_applied() {
        let server = MockGeminiServer::new(vec![MockResponse::json(200, embedding_response(vec![vec![3.0, 4.0]]))]);
        let vector = provider(server.base_url(), 2).embed_document("alpha document").expect("embed document");

        assert_eq!(vector, vec![3.0, 4.0]);
        let body = request_body(&server);
        assert_eq!(body["content"]["parts"][0]["text"], format!("{GEMINI_DOCUMENT_PREFIX}alpha document"));
    }

    #[test]
    fn batch_returns_vectors_in_positional_order() {
        let server = MockGeminiServer::new(vec![MockResponse::json(
            200,
            embedding_response(vec![vec![1.0, 1.5], vec![2.0, 2.5], vec![3.0, 3.5]]),
        )]);
        let vectors =
            provider(server.base_url(), 2).embed_documents(&["first", "second", "third"]).expect("batch embed");

        assert_eq!(vectors, vec![vec![1.0, 1.5], vec![2.0, 2.5], vec![3.0, 3.5]]);
        let body = request_body(&server);
        let requests = body["requests"].as_array().expect("requests array");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0]["content"]["parts"][0]["text"], format!("{GEMINI_DOCUMENT_PREFIX}first"));
        assert_eq!(requests[1]["content"]["parts"][0]["text"], format!("{GEMINI_DOCUMENT_PREFIX}second"));
        assert_eq!(requests[2]["content"]["parts"][0]["text"], format!("{GEMINI_DOCUMENT_PREFIX}third"));
        assert_eq!(server.requests()[0].path, "/models/gemini-embedding-2:batchEmbedContents");
    }

    #[test]
    fn batch_splits_large_inputs_and_preserves_positional_order() {
        let first_vectors: Vec<Vec<f32>> = (0..100).map(|index| vec![index as f32, 1.0]).collect();
        let server = MockGeminiServer::new(vec![
            MockResponse::json(200, embedding_response(first_vectors)),
            MockResponse::json(200, embedding_response(vec![vec![100.0, 1.0]])),
        ]);
        let texts: Vec<String> = (0..101).map(|index| format!("document {index}")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();

        let vectors = provider(server.base_url(), 2).embed_documents(&refs).expect("microbatch embed");

        assert_eq!(vectors.len(), 101);
        assert_eq!(vectors[0], vec![0.0, 1.0]);
        assert_eq!(vectors[100], vec![100.0, 1.0]);
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        let first_body: Value = serde_json::from_str(&requests[0].body).expect("first request json");
        let second_body: Value = serde_json::from_str(&requests[1].body).expect("second request json");
        assert_eq!(first_body["requests"].as_array().expect("first requests").len(), 100);
        assert_eq!(second_body["requests"].as_array().expect("second requests").len(), 1);
    }

    #[test]
    fn dimension_mismatch_is_reported() {
        let server = MockGeminiServer::new(vec![MockResponse::json(200, embedding_response(vec![vec![1.0]]))]);
        let error = provider(server.base_url(), 2).embed_document("short vector").expect_err("dimension mismatch");

        assert!(matches!(error, EmbeddingError::DimensionMismatch { expected: 2, found: 1 }));
    }

    #[test]
    fn unauthorized_maps_to_auth() {
        let server = MockGeminiServer::new(vec![MockResponse::json(
            401,
            json!({ "error": { "message": "bad API key" } }).to_string(),
        )]);
        let error = provider(server.base_url(), 2).embed_document("doc").expect_err("auth");

        assert!(matches!(error, EmbeddingError::Auth(message) if message.contains("bad API key")));
    }

    #[test]
    fn invalid_api_key_bad_request_maps_to_auth() {
        let server = MockGeminiServer::new(vec![MockResponse::json(
            400,
            json!({
                "error": {
                    "code": 400,
                    "status": "INVALID_ARGUMENT",
                    "message": "API key not valid. Please pass a valid API key."
                }
            })
            .to_string(),
        )]);
        let error = provider(server.base_url(), 2).embed_document("doc").expect_err("invalid key");

        assert!(matches!(error, EmbeddingError::Auth(message) if message.contains("API key not valid")));
    }

    #[test]
    fn rate_limit_maps_retry_after() {
        let server = MockGeminiServer::new(vec![MockResponse::json(
            429,
            json!({ "error": { "message": "slow down" } }).to_string(),
        )
        .with_header("Retry-After", "7")]);
        let error = provider(server.base_url(), 2).embed_document("doc").expect_err("rate limit");

        assert!(matches!(
            error,
            EmbeddingError::RateLimit { retry_after: Some(delay), message } if delay == Duration::from_secs(7) && message.contains("slow down")
        ));
    }

    #[test]
    fn malformed_json_maps_to_contract() {
        let server = MockGeminiServer::new(vec![MockResponse::json(200, "{\"embeddings\": [".to_string())]);
        let error = provider(server.base_url(), 2).embed_document("doc").expect_err("contract");

        assert!(
            matches!(error, EmbeddingError::Contract(message) if message.contains("parse Gemini embedding response"))
        );
    }

    #[test]
    #[serial]
    fn missing_credential_returns_auth_on_load() {
        let previous = std::env::var_os(GEMINI_API_KEY_ENV);
        std::env::remove_var(GEMINI_API_KEY_ENV);
        let runtime = tempfile::tempdir().expect("tempdir");

        let error = ApiEmbeddingProvider::load_for_runtime(runtime.path(), triple(2)).expect_err("missing key");

        restore_env(GEMINI_API_KEY_ENV, previous);
        assert!(matches!(error, EmbeddingError::Auth(message) if message.contains("Gemini API key not found")));
    }

    #[cfg(unix)]
    #[test]
    fn key_write_tightens_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let runtime = tempfile::tempdir().expect("tempdir");
        let key_path = gemini_api_key_path(runtime.path());
        fs::write(&key_path, "old-key\n").expect("seed key");
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o644)).expect("loosen key permissions");

        write_gemini_api_key(runtime.path(), "new-key").expect("write key");

        assert_eq!(fs::read_to_string(&key_path).expect("read key"), "new-key\n");
        assert_eq!(fs::metadata(&key_path).expect("key metadata").permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn key_write_rejects_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let runtime = tempfile::tempdir().expect("tempdir");
        let target = runtime.path().join("target-secret");
        fs::write(&target, "unchanged\n").expect("seed target");
        symlink(&target, gemini_api_key_path(runtime.path())).expect("plant symlink");

        let error = write_gemini_api_key(runtime.path(), "redirected-key").expect_err("symlink must fail");

        assert!(matches!(error, EmbeddingError::Auth(message) if message.contains("symlink")));
        assert_eq!(fs::read_to_string(target).expect("read target"), "unchanged\n");
    }

    #[test]
    fn debug_redacts_api_key() {
        let provider =
            ApiEmbeddingProvider::new_for_test(triple(2), "super-secret-test-key", "http://127.0.0.1:1".to_string())
                .expect("provider");

        let debug = format!("{provider:?}");
        assert!(!debug.contains("super-secret-test-key"));
        assert!(debug.contains("ApiKey(***)"));
    }

    #[test]
    fn mock_server_can_assert_zero_requests() {
        let _server = MockGeminiServer::panic_on_any_request();
    }

    fn restore_env(name: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}
