pub mod auth;
pub mod config;
pub mod routes;
pub mod server;

pub use config::WebConfig;
pub use server::{embedded_asset_names, fixture_router, router, router_with_state, run, run_with_state, WebState};
