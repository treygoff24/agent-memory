pub mod auth;
pub mod config;
pub mod routes;
pub mod server;
pub mod state;

pub use config::WebConfig;
#[cfg(feature = "dev-fixtures")]
pub use server::fixture_router;
pub use server::{embedded_asset_names, router, router_with_state, run, run_with_state};
pub use state::WebState;
