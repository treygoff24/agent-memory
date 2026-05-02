use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::server::{backend_unavailable, WebState};

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

pub async fn roi(State(state): State<WebState>, Query(query): Query<RoiQuery>) -> impl IntoResponse {
    let Some(data) = state.dashboard_data() else {
        if state.daemon_socket().is_some() {
            return crate::routes::deferred_response("roi").into_response();
        }
        return backend_unavailable("roi").into_response();
    };
    Json(data.roi_for_window(query.window.unwrap_or(90))).into_response()
}
