//! Frontmatter parser, validator, and canonical serializer.

mod defaults;
mod parse;
mod schema;
mod semantic;
mod serialize;
mod validate;

pub use defaults::{default_retrieval_policy, default_source, default_write_policy};
pub use parse::{parse_document, parse_frontmatter_yaml, ParsedMemory};
pub use schema::SUPPORTED_SCHEMA_VERSION;
pub(crate) use semantic::canonicalize_cue_union;
pub use semantic::{
    normalize_abstraction_cues, normalize_abstraction_value, normalize_cue_values, normalize_semantic_text,
};
pub use serialize::{serialize_document, serialize_frontmatter};
pub use validate::{validate_frontmatter, validate_lifecycle_transition};
