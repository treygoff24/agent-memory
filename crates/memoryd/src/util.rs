//! Small leaf utilities shared across daemon modules.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Serialize a unit-variant enum to its JSON string representation.
///
/// Lives in the `util` leaf (not `handlers`) so DTO modules like
/// `trust_artifact` can use it without forming a `handlers` <-> `trust_artifact`
/// module cycle.
pub(crate) fn serialized_enum_value<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_value(value)
        .expect("invariant: caller passes a unit-variant enum that serde always serializes infallibly");
    json.as_str().expect("invariant: callers pass unit-variant enums that serialize to JSON strings").to_string()
}

/// Parse an RFC 3339 timestamp into a UTC `DateTime`, returning `None` when the
/// value is not a well-formed RFC 3339 string.
pub(crate) fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

/// Build a comma-separated list of `count` SQL bind placeholders (`?,?,...`).
///
/// `count` must be greater than zero; an empty placeholder list would produce an
/// invalid `IN ()` clause.
pub(crate) fn sql_placeholders(count: usize) -> String {
    debug_assert!(count > 0);
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}
