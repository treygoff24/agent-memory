pub mod auth;
pub mod config;
pub mod routes;
pub mod server;
pub mod state;

pub use config::WebConfig;
#[cfg(feature = "dev-fixtures")]
pub use server::fixture_router;
pub use server::{embedded_asset_names, router, router_with_state, run, run_with_state};
#[cfg(feature = "dev-fixtures")]
pub use state::DEV_FIXTURE_DASHBOARD_AUTH_TOKEN;
pub use state::{
    DashboardAuthToken, WebState, DASHBOARD_AUTH_COOKIE, DASHBOARD_AUTH_ENV, DASHBOARD_AUTH_HEADER,
    DASHBOARD_AUTH_QUERY,
};
