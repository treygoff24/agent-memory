//! Shared daemon round-trip dispatch for dashboard routes.
//!
//! Every daemon-backed handler used to inline the same ladder: send the request,
//! match `Success(ResponsePayload::X(v))` to the one payload it wanted, fold
//! `Error` / an unexpected payload / a transport failure into [`daemon_error`].
//! [`daemon_call`] collapses that ladder into one generic round-trip keyed on the
//! [`FromPayload`] impl for the expected response type, mirroring the sibling TUI
//! crate's `expect_response`.

use std::path::Path as FsPath;

use memoryd::protocol::{RecallHitsResponse, RequestPayload, ResponsePayload, ResponseResult, StatusResponse};

use crate::routes::error::daemon_error;

/// Extract the one [`ResponsePayload`] variant a route expects.
///
/// On a variant mismatch the payload is handed back (`Err(payload)`) so the
/// caller can render the exact same `unexpected_response` debug string the
/// hand-written matches produced — `{ResponseResult::Success(payload):?}`.
pub(crate) trait FromPayload: Sized {
    // The `Err` carries the unmatched `ResponsePayload` (a large enum) back so the
    // caller can render the exact `unexpected_response` debug string; boxing it
    // would buy nothing here.
    #[allow(clippy::result_large_err)]
    fn from_payload(payload: ResponsePayload) -> Result<Self, ResponsePayload>;
}

macro_rules! impl_payload_from {
    ($type:ty, $variant:path) => {
        impl FromPayload for $type {
            #[allow(clippy::result_large_err)]
            fn from_payload(payload: ResponsePayload) -> Result<Self, ResponsePayload> {
                match payload {
                    $variant(value) => Ok(value),
                    other => Err(other),
                }
            }
        }
    };
}

impl_payload_from!(StatusResponse, ResponsePayload::Status);
impl_payload_from!(memoryd::protocol::SearchResponse, ResponsePayload::Search);
impl_payload_from!(memoryd::protocol::DashboardRoiResponse, ResponsePayload::DashboardRoi);
impl_payload_from!(RecallHitsResponse, ResponsePayload::RecallHits);
impl_payload_from!(memoryd::protocol::ReviewQueueResponse, ResponsePayload::ReviewQueue);
impl_payload_from!(memoryd::protocol::InspectEntitiesResponse, ResponsePayload::InspectEntities);
impl_payload_from!(memoryd::protocol::PeerStatusResponse, ResponsePayload::PeerStatus);
impl_payload_from!(Box<memoryd::trust_artifact::TrustArtifact>, ResponsePayload::TrustArtifact);

/// Send a request to the daemon and extract the expected response payload.
///
/// Folds every failure mode into a [`daemon_error`] response keyed on `route`:
/// transport failure → `daemon_unavailable`, a typed protocol error →
/// the daemon's own `code`/`message`, an unexpected payload → `unexpected_response`
/// with the full `Success(...)` debug repr. The `Ok` arm yields the typed payload
/// for the caller to map onto its wire shape.
///
/// The `Err` variant is the fully rendered `axum::response::Response` (the
/// idiomatic axum error-as-response), so callers `?` it straight into their own
/// `Response` return type; boxing it would only add ceremony at every call site.
#[allow(clippy::result_large_err)]
pub(crate) async fn daemon_call<T: FromPayload>(
    socket_path: &FsPath,
    route: &'static str,
    request_id: impl Into<String>,
    request: RequestPayload,
) -> Result<T, axum::response::Response> {
    use axum::response::IntoResponse;

    match memoryd::client::request(socket_path, request_id, request).await {
        Ok(response) => match response.result {
            ResponseResult::Success(payload) => T::from_payload(payload).map_err(|payload| {
                let unexpected = ResponseResult::Success(payload);
                daemon_error(route, "unexpected_response", format!("{unexpected:?}")).into_response()
            }),
            ResponseResult::Error(error) => Err(daemon_error(route, error.code, error.message).into_response()),
        },
        Err(error) => Err(daemon_error(route, "daemon_unavailable", error.to_string()).into_response()),
    }
}
