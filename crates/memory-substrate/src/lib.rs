#![deny(unsafe_op_in_unsafe_fn)]
//! Stream A core memory substrate.

pub mod api;
pub mod cas;
pub mod config;
pub mod error;
pub mod events;
pub mod frontmatter;
pub mod git;
pub mod ids;
pub mod index;
pub mod markdown;
pub mod merge;
pub mod model;
pub mod runtime;
pub mod tree;
pub mod watcher;

pub use api::Substrate;
pub use config::PromptVersion;
pub use error::*;
pub use index::MirrorEvent;
pub use model::*;

/// Stream A spec version implemented by this crate.
pub const STREAM_A_SPEC_VERSION: &str = "1.1";

/// Single source of truth — see CLAUDE.md invariant 5.
pub const SUBSTRATE_SCHEMA_VERSION: u32 = 1;
