use std::collections::HashSet;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::Json;
use rand::RngCore;
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

use crate::routes::DashboardData;

pub const CSRF_HEADER: &str = "x-memorum-csrf";
pub const DASHBOARD_AUTH_HEADER: &str = "x-memorum-dashboard-auth";
pub const DASHBOARD_AUTH_COOKIE: &str = "memorum_web_auth";
pub const DASHBOARD_AUTH_QUERY: &str = "auth";
/// Re-exported from `memoryd` so the daemon (which sets this env var) and the
/// dashboard (which reads it) share exactly one literal. A compile-time alias to
/// the daemon's canonical const keeps the token handshake from drifting.
pub const DASHBOARD_AUTH_ENV: &str = memoryd::WEB_AUTH_ENV;
const CSRF_TOKEN_BYTES: usize = 32;
const DASHBOARD_AUTH_TOKEN_BYTES: usize = 32;

#[cfg(feature = "dev-fixtures")]
pub const DEV_FIXTURE_DASHBOARD_AUTH_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsrfToken(String);

impl CsrfToken {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; CSRF_TOKEN_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(hex::encode(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches_header(&self, request: &axum::http::Request<axum::body::Body>) -> bool {
        request
            .headers()
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| self.constant_time_eq(value))
    }

    /// Constant-time equality against a candidate token.
    ///
    /// The CSRF token is a bearer secret, so the comparison must not leak how
    /// many leading bytes matched via timing. `subtle::ConstantTimeEq` only
    /// compares constant-time when the slices are the same length, so we first
    /// reject any length mismatch — the token length (`CSRF_TOKEN_BYTES * 2` hex
    /// chars) is fixed and public, so branching on length leaks nothing secret.
    fn constant_time_eq(&self, candidate: &str) -> bool {
        let expected = self.0.as_bytes();
        let candidate = candidate.as_bytes();
        if expected.len() != candidate.len() {
            return false;
        }
        expected.ct_eq(candidate).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardAuthToken(String);

impl DashboardAuthToken {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; DASHBOARD_AUTH_TOKEN_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(hex::encode(bytes))
    }

    pub fn from_hex(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        if value.len() == DASHBOARD_AUTH_TOKEN_BYTES * 2 && value.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn constant_time_eq(&self, candidate: &str) -> bool {
        constant_time_token_eq(self.0.as_bytes(), candidate.as_bytes())
    }
}

#[derive(Clone)]
pub struct WebState {
    csrf_token: CsrfToken,
    dashboard_auth_token: DashboardAuthToken,
    review_actions: Arc<ReviewActionTracker>,
    #[cfg(feature = "dev-fixtures")]
    dashboard_data: Option<Arc<DashboardData>>,
    daemon_socket: Option<Arc<PathBuf>>,
    policy_dir: Option<Arc<PathBuf>>,
    recorded_review_actions: Arc<Mutex<Vec<ReviewActionRecord>>>,
    recorded_reality_check_actions: Arc<Mutex<Vec<RealityCheckActionRecord>>>,
}

impl WebState {
    pub fn new() -> Self {
        Self::unconfigured()
    }

    pub fn unconfigured() -> Self {
        Self::fresh(None, None)
    }

    pub fn daemon(socket_path: impl Into<PathBuf>) -> Self {
        Self::fresh(Some(Arc::new(socket_path.into())), None)
    }

    #[cfg(feature = "dev-fixtures")]
    pub fn fixture() -> Self {
        Self::with_dashboard_data(DashboardData::default())
    }

    #[cfg(feature = "dev-fixtures")]
    pub fn with_dashboard_data(dashboard_data: DashboardData) -> Self {
        let mut state = Self::fresh(None, None);
        state.dashboard_data = Some(Arc::new(dashboard_data));
        state
    }

    fn fresh(daemon_socket: Option<Arc<PathBuf>>, policy_dir: Option<Arc<PathBuf>>) -> Self {
        Self {
            csrf_token: CsrfToken::generate(),
            dashboard_auth_token: default_dashboard_auth_token(),
            review_actions: Arc::new(ReviewActionTracker::default()),
            #[cfg(feature = "dev-fixtures")]
            dashboard_data: None,
            daemon_socket,
            policy_dir,
            recorded_review_actions: Arc::new(Mutex::new(Vec::new())),
            recorded_reality_check_actions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_policy_dir(mut self, policy_dir: impl Into<PathBuf>) -> Self {
        self.policy_dir = Some(Arc::new(policy_dir.into()));
        self
    }

    pub fn with_dashboard_auth_token(mut self, token: DashboardAuthToken) -> Self {
        self.dashboard_auth_token = token;
        self
    }

    pub fn csrf_token(&self) -> &CsrfToken {
        &self.csrf_token
    }

    pub fn dashboard_auth_token(&self) -> &DashboardAuthToken {
        &self.dashboard_auth_token
    }

    pub fn dashboard_data(&self) -> Option<Arc<DashboardData>> {
        #[cfg(feature = "dev-fixtures")]
        {
            self.dashboard_data.clone()
        }
        // Production builds never carry dashboard fixtures: dashboard routes must
        // fall through to the daemon backend (or fail loudly) rather than serve
        // plausible fake numbers.
        #[cfg(not(feature = "dev-fixtures"))]
        {
            None
        }
    }

    pub fn daemon_socket(&self) -> Option<&FsPath> {
        self.daemon_socket.as_deref().map(PathBuf::as_path)
    }

    /// Resolve which backend a dashboard route should read from.
    ///
    /// Collapses the `dashboard_data() -> daemon_socket() -> unavailable` ladder
    /// every handler used to inline. Fixture data wins (dev/test only), then a
    /// daemon socket, otherwise the route has no backend. The `Fixture` arm only
    /// exists under `dev-fixtures`; release builds resolve to `Daemon` or
    /// `Unavailable`.
    pub fn backend(&self) -> Backend<'_> {
        #[cfg(feature = "dev-fixtures")]
        if let Some(data) = self.dashboard_data() {
            return Backend::Fixture(data);
        }
        match self.daemon_socket() {
            Some(socket) => Backend::Daemon(socket),
            None => Backend::Unavailable,
        }
    }

    pub fn policy_dir(&self) -> Option<&FsPath> {
        self.policy_dir.as_deref().map(PathBuf::as_path)
    }

    pub fn is_reviewable(&self, id: &str) -> bool {
        self.dashboard_data().is_some_and(|data| data.reviewable_memory_ids.contains(id))
    }

    pub async fn claim_review_action(&self, id: &str) -> bool {
        self.review_actions.claim(id).await
    }

    pub async fn release_review_action(&self, id: &str) {
        self.review_actions.release(id).await;
    }

    pub async fn record_review_action(&self, action: ReviewActionRecord) {
        self.recorded_review_actions.lock().await.push(action);
    }

    pub async fn recorded_review_actions(&self) -> Vec<ReviewActionRecord> {
        self.recorded_review_actions.lock().await.clone()
    }

    pub async fn record_reality_check_action(&self, action: RealityCheckActionRecord) {
        self.recorded_reality_check_actions.lock().await.push(action);
    }

    pub async fn recorded_reality_check_actions(&self) -> Vec<RealityCheckActionRecord> {
        self.recorded_reality_check_actions.lock().await.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewActionRecord {
    pub id: String,
    pub action: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealityCheckActionRecord {
    pub memory_id: String,
    pub action: String,
    pub correction: Option<String>,
}

impl Default for WebState {
    fn default() -> Self {
        Self::new()
    }
}

fn default_dashboard_auth_token() -> DashboardAuthToken {
    #[cfg(feature = "dev-fixtures")]
    {
        DashboardAuthToken::from_hex(DEV_FIXTURE_DASHBOARD_AUTH_TOKEN).expect("fixture auth token is valid hex")
    }
    #[cfg(not(feature = "dev-fixtures"))]
    {
        DashboardAuthToken::generate()
    }
}

fn constant_time_token_eq(expected: &[u8], candidate: &[u8]) -> bool {
    if expected.len() != candidate.len() {
        return false;
    }
    expected.ct_eq(candidate).into()
}

#[derive(Default)]
struct ReviewActionTracker {
    active: Mutex<HashSet<String>>,
}

impl ReviewActionTracker {
    async fn claim(&self, id: &str) -> bool {
        self.active.lock().await.insert(id.to_owned())
    }

    async fn release(&self, id: &str) {
        self.active.lock().await.remove(id);
    }
}

/// The backend a dashboard route reads from, resolved once via [`WebState::backend`].
///
/// `Fixture` carries in-process dev/test data and only exists under the
/// `dev-fixtures` feature; release builds only ever produce `Daemon` or
/// `Unavailable`.
pub enum Backend<'a> {
    #[cfg(feature = "dev-fixtures")]
    Fixture(Arc<DashboardData>),
    Daemon(&'a FsPath),
    Unavailable,
}

pub fn backend_unavailable(route: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "dashboard_backend_unavailable",
            "route": route,
            "note": "dashboard routes require a daemon-backed or test fixture backend"
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_constant_time_eq_accepts_exact_match() {
        let token = CsrfToken::generate();
        let value = token.as_str().to_owned();
        assert!(token.constant_time_eq(&value));
    }

    #[test]
    fn csrf_constant_time_eq_rejects_wrong_token() {
        let token = CsrfToken::generate();
        let other = CsrfToken::generate();
        assert_ne!(token.as_str(), other.as_str());
        assert!(!token.constant_time_eq(other.as_str()));
    }

    #[test]
    fn csrf_constant_time_eq_rejects_length_mismatch() {
        let token = CsrfToken::generate();
        // A correct prefix must not pass: rejecting a length mismatch is what
        // keeps the constant-time comparison sound (subtle requires equal len).
        let prefix = &token.as_str()[..token.as_str().len() - 1];
        assert!(!token.constant_time_eq(prefix));
        assert!(!token.constant_time_eq(""));
        assert!(!token.constant_time_eq(&format!("{}0", token.as_str())));
    }

    #[test]
    fn csrf_token_is_fixed_hex_length() {
        // Length is public and fixed; the constant-time guard relies on it.
        let token = CsrfToken::generate();
        assert_eq!(token.as_str().len(), CSRF_TOKEN_BYTES * 2);
    }

    #[test]
    fn dashboard_auth_token_rejects_non_hex_or_wrong_length() {
        assert!(DashboardAuthToken::from_hex("a".repeat(DASHBOARD_AUTH_TOKEN_BYTES * 2)).is_some());
        assert!(DashboardAuthToken::from_hex("a".repeat(DASHBOARD_AUTH_TOKEN_BYTES * 2 - 1)).is_none());
        assert!(DashboardAuthToken::from_hex("z".repeat(DASHBOARD_AUTH_TOKEN_BYTES * 2)).is_none());
    }

    #[cfg(feature = "dev-fixtures")]
    #[test]
    fn dev_fixture_dashboard_auth_token_is_valid() {
        let token = DashboardAuthToken::from_hex(DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
            .expect("fixture dashboard auth token must be valid");
        assert_eq!(token.as_str().len(), DASHBOARD_AUTH_TOKEN_BYTES * 2);
    }
}
