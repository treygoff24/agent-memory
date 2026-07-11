//! Derived SQLite index, chunk, and vector helpers.

mod chunking;
mod embedding;
mod events_read;
mod fts;
mod migrations;
mod query;
mod read;
mod schema;
mod search;
pub mod sqlite_vec;
mod upsert;
mod util;
mod vector;

pub use chunking::{chunk_memory, Chunk};
pub use events_read::{EventsLogPage, MirrorEvent};
pub use migrations::{migrate_v6, open_index, INDEX_SUPPORTED_SCHEMA_VERSION};
pub use query::Index;
pub use vector::{
    held_local_embedding_jobs, reconcile_missing, reconcile_orphans, reconcile_pending_jobs, VectorStore,
};

/// Render `count` comma-separated `?` SQL bind placeholders (e.g. `?,?,?`).
///
/// Shared by the index read modules that build `IN (...)` clauses. Callers are
/// responsible for never passing `count == 0`: an empty `IN ()` is invalid SQL,
/// and an empty filter set means "match nothing", which each caller handles by
/// short-circuiting before reaching here.
pub fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}

/// Fixed `IN (...)` width buckets used to keep `prepare_cached` from churning a
/// distinct statement for every result-set size. A bound `IN (...)` clause sized
/// to one of these widths reuses a handful of cached plans regardless of the
/// actual id count.
const IN_CLAUSE_BUCKETS: [usize; 5] = [1, 4, 16, 64, 256];

/// Round `count` up to the smallest `IN_CLAUSE_BUCKETS` width that holds it, or
/// the next multiple of the largest bucket for oversized inputs.
///
/// Bucketing the placeholder count means varied id-set sizes collapse onto a
/// small set of cached `IN (...)` statements instead of one cache entry per
/// distinct size. Callers pad their bindings to the returned width (see
/// [`pad_in_clause_bindings`]); because `IN` is set membership, repeating a real
/// id is a no-op that matches the same rows.
pub fn bucketed_in_clause_width(count: usize) -> usize {
    debug_assert!(count > 0, "callers short-circuit on empty id sets before bucketing");
    if let Some(&bucket) = IN_CLAUSE_BUCKETS.iter().find(|&&bucket| count <= bucket) {
        return bucket;
    }
    let largest = IN_CLAUSE_BUCKETS[IN_CLAUSE_BUCKETS.len() - 1];
    count.div_ceil(largest) * largest
}

/// Pad `ids` out to `width` by repeating the first id, so the binding count
/// matches a [`bucketed_in_clause_width`] placeholder count. The repeated id is
/// already in the set, so it adds no rows to an `IN (...)` match.
pub fn pad_in_clause_bindings<'a, S: AsRef<str>>(ids: &'a [S], width: usize) -> impl Iterator<Item = &'a str> + 'a {
    debug_assert!(!ids.is_empty() && ids.len() <= width, "width must hold the non-empty id set");
    let first = ids.first().map(AsRef::as_ref).unwrap_or("");
    ids.iter().map(AsRef::as_ref).chain(std::iter::repeat_n(first, width.saturating_sub(ids.len())))
}

#[cfg(test)]
mod in_clause_bucket_tests {
    use super::{bucketed_in_clause_width, pad_in_clause_bindings, IN_CLAUSE_BUCKETS};

    #[test]
    fn width_rounds_up_to_the_smallest_holding_bucket() {
        assert_eq!(bucketed_in_clause_width(1), 1);
        assert_eq!(bucketed_in_clause_width(2), 4);
        assert_eq!(bucketed_in_clause_width(4), 4);
        assert_eq!(bucketed_in_clause_width(5), 16);
        assert_eq!(bucketed_in_clause_width(16), 16);
        assert_eq!(bucketed_in_clause_width(17), 64);
        assert_eq!(bucketed_in_clause_width(64), 64);
        assert_eq!(bucketed_in_clause_width(65), 256);
        assert_eq!(bucketed_in_clause_width(256), 256);
    }

    #[test]
    fn oversized_inputs_round_to_a_multiple_of_the_largest_bucket() {
        assert_eq!(bucketed_in_clause_width(257), 512);
        assert_eq!(bucketed_in_clause_width(512), 512);
        assert_eq!(bucketed_in_clause_width(513), 768);
    }

    #[test]
    fn width_always_holds_the_input_and_collapses_to_few_distinct_widths() {
        let mut widths = std::collections::BTreeSet::new();
        for count in 1..=256usize {
            let width = bucketed_in_clause_width(count);
            assert!(width >= count, "bucket width {width} must hold {count} ids");
            widths.insert(width);
        }
        // 1..=256 collapses onto exactly the five configured buckets, not 256
        // distinct prepared-statement widths.
        assert_eq!(widths.into_iter().collect::<Vec<_>>(), IN_CLAUSE_BUCKETS.to_vec());
    }

    #[test]
    fn padding_preserves_the_id_set_and_fills_to_width_by_repeating_the_first() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let width = bucketed_in_clause_width(ids.len());
        assert_eq!(width, 4);
        let padded: Vec<&str> = pad_in_clause_bindings(&ids, width).collect();
        assert_eq!(padded, vec!["a", "b", "c", "a"], "the pad repeats the first (already-present) id");
        // The distinct set the IN clause matches is unchanged by padding.
        let distinct: std::collections::BTreeSet<&str> = padded.iter().copied().collect();
        assert_eq!(distinct, ["a", "b", "c"].into_iter().collect());
    }

    #[test]
    fn padding_is_a_noop_when_count_equals_width() {
        let ids = vec!["x".to_string(), "y".to_string(), "z".to_string(), "w".to_string()];
        let padded: Vec<&str> = pad_in_clause_bindings(&ids, 4).collect();
        assert_eq!(padded, vec!["x", "y", "z", "w"]);
    }
}
