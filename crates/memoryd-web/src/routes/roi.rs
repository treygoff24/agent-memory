use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use serde::{Deserialize, Serialize};

use crate::routes::status::daemon_error;
use crate::state::{backend_unavailable, WebState};

#[derive(Clone, Debug, Deserialize)]
pub struct RoiQuery {
    pub window: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RoiResponse {
    pub window_days: u16,
    pub promotion_rate: f64,
    pub promotion_precision: f64,
    pub refusal_breakdown: BTreeMap<String, u32>,
    pub dreaming: DreamingRoi,
    pub reality_check_adherence: RealityCheckAdherence,
}

#[derive(Clone, Debug, Serialize)]
pub struct DreamingRoi {
    pub candidates_generated: u32,
    pub promoted_silent: u32,
    pub entered_review_queue: u32,
    pub dropped: u32,
    pub review_queue_approval_rate: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RealityCheckAdherence {
    pub weeks_completed: u32,
    pub weeks_skipped: u32,
}

#[cfg(feature = "dev-fixtures")]
impl RoiResponse {
    pub fn fixture(window_days: u16) -> Self {
        Self {
            window_days,
            promotion_rate: 0.68,
            promotion_precision: 0.91,
            refusal_breakdown: BTreeMap::from([
                ("contradiction".to_owned(), 1),
                ("grounding".to_owned(), 7),
                ("policy".to_owned(), 2),
                ("review_required".to_owned(), 0),
                ("tombstone".to_owned(), 3),
            ]),
            dreaming: DreamingRoi {
                candidates_generated: 18,
                promoted_silent: 9,
                entered_review_queue: 5,
                dropped: 4,
                review_queue_approval_rate: 0.80,
            },
            reality_check_adherence: RealityCheckAdherence { weeks_completed: 4, weeks_skipped: 1 },
        }
    }
}

impl From<memoryd::protocol::DashboardRoiResponse> for RoiResponse {
    fn from(value: memoryd::protocol::DashboardRoiResponse) -> Self {
        Self {
            window_days: value.window_days,
            promotion_rate: value.promotion_rate,
            promotion_precision: value.promotion_precision,
            refusal_breakdown: value.refusal_breakdown,
            dreaming: DreamingRoi {
                candidates_generated: value.dreaming.candidates_generated,
                promoted_silent: value.dreaming.promoted_silent,
                entered_review_queue: value.dreaming.entered_review_queue,
                dropped: value.dreaming.dropped,
                review_queue_approval_rate: value.dreaming.review_queue_approval_rate,
            },
            reality_check_adherence: RealityCheckAdherence {
                weeks_completed: value.reality_check_adherence.weeks_completed,
                weeks_skipped: value.reality_check_adherence.weeks_skipped,
            },
        }
    }
}

pub async fn roi(State(state): State<WebState>, Query(query): Query<RoiQuery>) -> impl IntoResponse {
    let window_days = query.window.unwrap_or(90);
    let Some(data) = state.dashboard_data() else {
        if let Some(socket_path) = state.daemon_socket() {
            return match memoryd::client::request(
                socket_path,
                "web-dashboard-roi",
                RequestPayload::DashboardRoi { window_days },
            )
            .await
            {
                Ok(response) => match response.result {
                    ResponseResult::Success(ResponsePayload::DashboardRoi(roi)) => {
                        Json(RoiResponse::from(roi)).into_response()
                    }
                    ResponseResult::Error(error) => daemon_error("roi", error.code, error.message).into_response(),
                    other => daemon_error("roi", "unexpected_response", format!("{other:?}")).into_response(),
                },
                Err(error) => daemon_error("roi", "daemon_unavailable", error.to_string()).into_response(),
            };
        }
        return backend_unavailable("roi").into_response();
    };
    Json(data.roi_for_window(window_days)).into_response()
}
