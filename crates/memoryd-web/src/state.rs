use std::collections::HashSet;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::Json;
use rand::RngCore;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::routes::DashboardData;

pub const CSRF_HEADER: &str = "x-memorum-csrf";
const CSRF_TOKEN_BYTES: usize = 32;

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
        request.headers().get(CSRF_HEADER).and_then(|value| value.to_str().ok()).is_some_and(|value| value == self.0)
    }
}

#[derive(Clone)]
pub struct WebState {
    csrf_token: CsrfToken,
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

    pub fn csrf_token(&self) -> &CsrfToken {
        &self.csrf_token
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
