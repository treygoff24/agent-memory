//! Small shared helpers for the index query submodules.
//!
//! Holds the column-decode error helper and the RFC3339 timestamp parser used by
//! several of the sibling query submodules, plus the shared embedding-triple
//! `WHERE` predicate so the identical `(provider, model_ref, dimension)` filters
//! cannot drift apart.

use chrono::{DateTime, Utc};

/// Shared `WHERE` predicate matching an embedding triple's identity columns.
///
/// Positional binds `?1`, `?2`, `?3` are the triple's `(provider, model_ref,
/// dimension)` in that order; callers bind
/// `params![triple.provider, triple.model_ref, i64::from(triple.dimension)]`
/// so every triple-keyed statement shares one predicate text and one bind shape.
pub(super) const EMBEDDING_TRIPLE_PREDICATE: &str = "provider=?1 AND model_ref=?2 AND dimension=?3";

pub(super) fn parse_index_time(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err)))
}

pub(super) fn invalid_column_value(field: &'static str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid {field}: {value}"))),
    )
}
