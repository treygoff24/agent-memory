//! Memorum daemon library: protocol, client/server runtime, MCP bridge,
//! coordination, recall, governance handlers, notifications, and supporting
//! services for the local-first memory daemon.

pub mod cli;
pub mod client;
pub mod coordination_config;
pub mod dream;
pub mod handlers;
pub mod mcp;
pub mod mcp_stdio;
pub mod notifications;
pub mod protocol;
pub mod reality_check;
pub mod recall;
pub mod recall_hits;
pub mod runtime_privacy;
pub mod serve_runtime;
pub mod server;
pub mod slash_commands;
pub mod socket;
pub mod state;
pub mod trust_artifact;
pub mod workers;
