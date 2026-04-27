//! Frontmatter schema constants.

/// Supported Stream A frontmatter schema version. Re-exported from the
/// canonical crate-root constant so there is a single source of truth.
pub use crate::SUBSTRATE_SCHEMA_VERSION as SUPPORTED_SCHEMA_VERSION;

/// Known fields emitted by the canonical serializer.
pub const CANONICAL_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "type",
    "scope",
    "summary",
    "confidence",
    "trust_level",
    "sensitivity",
    "status",
    "created_at",
    "updated_at",
    "author",
    "namespace",
    "canonical_namespace_id",
    "tags",
    "entities",
    "aliases",
    "source",
    "evidence",
    "requires_user_confirmation",
    "review_state",
    "supersedes",
    "superseded_by",
    "related",
    "tombstone_events",
    "retrieval_policy",
    "write_policy",
    "_merge_diagnostics",
];
