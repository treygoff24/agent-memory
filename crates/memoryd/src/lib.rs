//! Stream B daemon and MCP bridge foundation.

pub mod cli;
pub mod client;
pub mod coordination_config;
pub mod dashboard;
pub mod dream;
pub mod dynamics;
pub mod embedding;
pub mod export;
pub mod first_write;
pub mod handlers;
pub mod import;
pub mod mcp;
pub mod mcp_stdio;
pub mod notifications;
/// Path-resolution leaf: repo/runtime/socket defaults shared by `cli` and `setup`.
/// Lives at the crate root (not under `cli`) to break the `cli` <-> `setup` module cycle.
pub(crate) mod paths;
pub mod policy_editor;
pub mod protocol;
pub mod reality_check;
pub mod recall;
pub mod recall_hits;
pub mod server;
pub mod setup;
pub mod slash_commands;
pub mod socket;
pub mod state;
pub mod trust_artifact;
pub(crate) mod util;

/// Env var carrying the web-dashboard auth token from the daemon to its spawned
/// `memoryd-web` child. Single source of truth for both sides of the handshake.
pub use handlers::web_dashboard::WEB_AUTH_ENV;
