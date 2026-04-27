//! Frontmatter parser, validator, and canonical serializer.

mod defaults;
mod parse;
mod schema;
mod serialize;
mod validate;

pub use defaults::{default_retrieval_policy, default_source, default_write_policy};
pub use parse::{parse_document, parse_frontmatter_yaml, ParsedMemory};
pub use schema::SUPPORTED_SCHEMA_VERSION;
pub use serialize::serialize_document;
pub use validate::{validate_frontmatter, validate_memory_id};
