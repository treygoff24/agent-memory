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

/// Fixed `IN (...)` width buckets, mirroring `memory_substrate::index`'s
/// `IN_CLAUSE_BUCKETS`, so per-recall `IN` queries reuse a handful of cached
/// `prepare_cached` plans instead of minting a new one per distinct id count.
const IN_CLAUSE_BUCKETS: [usize; 5] = [1, 4, 16, 64, 256];

/// Round `count` up to the smallest [`IN_CLAUSE_BUCKETS`] width that holds it, or
/// the next multiple of the largest bucket for oversized inputs.
///
/// `count` must be greater than zero. Callers pad their bound ids out to the
/// returned width with [`pad_in_clause_ids`]; because `IN` is set membership,
/// repeating an already-present id adds no rows.
pub(crate) fn bucketed_in_clause_width(count: usize) -> usize {
    debug_assert!(count > 0, "callers short-circuit on empty id sets before bucketing");
    if let Some(&bucket) = IN_CLAUSE_BUCKETS.iter().find(|&&bucket| count <= bucket) {
        return bucket;
    }
    let largest = IN_CLAUSE_BUCKETS[IN_CLAUSE_BUCKETS.len() - 1];
    count.div_ceil(largest) * largest
}

/// Pad `ids` out to `width` by repeating the first id, matching a
/// [`bucketed_in_clause_width`] placeholder count. The repeated id is already in
/// the set, so it adds no rows to an `IN (...)` match.
pub(crate) fn pad_in_clause_ids<'a>(ids: &'a [&'a str], width: usize) -> impl Iterator<Item = &'a str> + 'a {
    debug_assert!(!ids.is_empty() && ids.len() <= width, "width must hold the non-empty id set");
    let first = ids.first().copied().unwrap_or("");
    ids.iter().copied().chain(std::iter::repeat_n(first, width - ids.len()))
}

#[cfg(test)]
mod in_clause_bucket_tests {
    use super::{bucketed_in_clause_width, pad_in_clause_ids};

    #[test]
    fn width_rounds_up_to_the_smallest_holding_bucket() {
        assert_eq!(bucketed_in_clause_width(1), 1);
        assert_eq!(bucketed_in_clause_width(2), 4);
        assert_eq!(bucketed_in_clause_width(5), 16);
        assert_eq!(bucketed_in_clause_width(64), 64);
        assert_eq!(bucketed_in_clause_width(65), 256);
        assert_eq!(bucketed_in_clause_width(256), 256);
        assert_eq!(bucketed_in_clause_width(257), 512);
    }

    #[test]
    fn padding_fills_to_width_with_the_first_id_and_preserves_the_set() {
        let ids = ["a", "b", "c"];
        let padded: Vec<&str> = pad_in_clause_ids(&ids, bucketed_in_clause_width(ids.len())).collect();
        assert_eq!(padded.len(), 4);
        assert_eq!(&padded[..3], &["a", "b", "c"]);
        assert_eq!(padded[3], "a");
    }
}
