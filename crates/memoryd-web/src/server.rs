use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::RustEmbed;
use tokio::net::TcpListener;

use crate::auth::require_csrf;
use crate::config::WebConfig;
use crate::routes::{
    audit, audit_temporal, audit_walk, entity_detail, entity_graph, notifications_stream, policy_editor_get,
    policy_editor_post, reality_check, reality_check_history, reality_check_respond, recall_hits, review_action,
    review_queue, roi, search, status, sync_dashboard,
};
use crate::state::WebState;

const INDEX_HTML: &str = "index.html";
const CSRF_PLACEHOLDER: &str = "__MEMORUM_CSRF_TOKEN__";
const SHUTDOWN_DRAIN_LIMIT: Duration = Duration::from_secs(5);

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct Assets;

pub fn router() -> Router {
    router_with_state(WebState::new())
}

pub fn fixture_router() -> Router {
    router_with_state(WebState::fixture())
}

pub fn router_with_state(state: WebState) -> Router {
    let protected_post_routes = Router::new()
        .route("/api/reality-check/respond", post(reality_check_respond))
        .route("/api/review/action", post(review_action))
        .route("/api/policy-editor", post(policy_editor_post))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_csrf));

    Router::new()
        .route("/", get(index))
        .route("/assets/{*path}", get(asset))
        .route("/api/status", get(status))
        .route("/api/entity-graph", get(entity_graph))
        .route("/api/entity-graph/{entity_id}", get(entity_detail))
        .route("/api/roi", get(roi))
        .route("/api/reality-check", get(reality_check))
        .route("/api/reality-check/history", get(reality_check_history))
        .route("/api/recall-hits", get(recall_hits))
        .route("/api/search", get(search))
        .route("/api/audit/{id}", get(audit))
        .route("/api/audit/{id}/walk", get(audit_walk))
        .route("/api/audit/{id}/temporal", get(audit_temporal))
        .route("/api/review", get(review_queue))
        .route("/api/notifications/stream", get(notifications_stream))
        .route("/api/policy-editor", get(policy_editor_get))
        .route("/api/sync-dashboard", get(sync_dashboard))
        .merge(protected_post_routes)
        .with_state(state)
}

pub async fn run(config: WebConfig, shutdown: impl Future<Output = ()> + Send + 'static) -> Result<()> {
    run_with_state(config, WebState::new(), shutdown).await
}

pub async fn run_with_state(
    config: WebConfig,
    state: WebState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    config.validate_localhost()?;
    let address = SocketAddr::new(config.bind_address, config.port);
    let listener = TcpListener::bind(address).await?;
    let (stop_accepting, stop_requested) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router_with_state(state)).with_graceful_shutdown(async {
        let _ = stop_requested.await;
    });
    let mut server_task = tokio::spawn(async move { server.await });

    shutdown.await;
    let _ = stop_accepting.send(());

    match tokio::time::timeout(SHUTDOWN_DRAIN_LIMIT, &mut server_task).await {
        Ok(join_result) => join_result??,
        Err(_) => {
            tracing::warn!("memoryd web graceful shutdown exceeded drain limit");
            server_task.abort();
        }
    }

    Ok(())
}

async fn index(State(state): State<WebState>) -> impl IntoResponse {
    match embedded_text(INDEX_HTML) {
        Some(template) => Html(template.replace(CSRF_PLACEHOLDER, state.csrf_token().as_str())).into_response(),
        None => {
            tracing::error!("embedded dashboard asset {INDEX_HTML} is missing or not valid UTF-8");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn asset(Path(path): Path<String>) -> Response {
    let embedded_path = format!("assets/{path}");
    embedded_response(&embedded_path, content_type_for(&embedded_path))
}

fn embedded_text(path: &str) -> Option<String> {
    let asset = Assets::get(path)?;
    String::from_utf8(asset.data.into_owned()).ok()
}

pub fn embedded_asset_names() -> Vec<String> {
    Assets::iter().map(|path| path.into_owned()).collect()
}

fn embedded_response(path: &str, content_type: &str) -> Response {
    match Assets::get(path) {
        Some(asset) => {
            let mut response = asset.data.into_owned().into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(content_type).unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            response
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript",
        Some("woff2") => "font/woff2",
        Some("html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}
