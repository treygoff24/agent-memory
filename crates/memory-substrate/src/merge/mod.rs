//! Semantic frontmatter merge library.

mod body_diff3;
mod field_rules;
mod lifecycle;
mod quarantine;
mod source_artifact;
mod three_way;

pub use three_way::{merge_markdown, MergeInput, MergeResult};

/// Re-export merge errors so embedders don't need to reach into
/// `crate::error` to handle exit codes.
pub use crate::error::{MergeError, MergeSide};

/// Merge-driver supported frontmatter schema version. Routed through the
/// canonical [`crate::SUBSTRATE_SCHEMA_VERSION`] so the driver and substrate
/// stay in lockstep — see CLAUDE.md invariant 5.
pub use crate::SUBSTRATE_SCHEMA_VERSION as MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION;
