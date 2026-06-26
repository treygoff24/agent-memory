//! Deterministic perf corpus helpers per spec §17.6.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use sha2::{Digest, Sha256};

#[allow(clippy::mixed_case_hex_literals)]
/// Smoke-tier corpus seed.
pub const SEED_SMOKE: u64 = 0xA17_50f7;
#[allow(clippy::mixed_case_hex_literals, clippy::unusual_byte_groupings)]
/// Release-tier corpus seed.
pub const SEED_RELEASE: u64 = 0xA17_5e1e_a5e;

/// Materialized corpus returned by `build_corpus`.
pub struct CorpusBuild {
    /// Temporary directory owning the corpus files.
    ///
    /// Dropping this value deletes the directory. Keep it alive for the bench
    /// duration; do not extract the path unless you want manual cleanup.
    pub dir: tempfile::TempDir,
    /// Total number of memory files written.
    pub total: usize,
    /// SHA-256 of the corpus content for result fingerprinting (spec §17.6).
    pub sha256: String,
}

/// Build a deterministic synthetic corpus with Stream-E-like namespacing.
///
/// Proportions mirror realistic usage patterns so cold-reindex p95s are
/// comparable to production. Must be called before any baseline is recorded —
/// baselines are immutable (CLAUDE.md invariant 7).
///
/// Spec §17.6: the corpus shape is pinned via `corpus_sha256` in bench results
/// so regressions can be verified against an identical corpus.
pub fn build_corpus(seed: u64, size: usize) -> CorpusBuild {
    let dir = tempfile::TempDir::new().expect("corpus tempdir");
    let mut rng = StdRng::seed_from_u64(seed);

    let placements: &[(&str, f64)] = &[
        ("me/notes/", 0.20),
        ("me/preferences/", 0.05),
        ("projects/alpha/decisions/", 0.10),
        ("projects/alpha/conventions/", 0.05),
        ("projects/beta/decisions/", 0.10),
        ("projects/beta/conventions/", 0.05),
        ("projects/gamma/decisions/", 0.10),
        ("agent/patterns/", 0.20),
        ("dreams/2026-04-15/", 0.05),
        ("dreams/2026-04-22/", 0.05),
        ("dreams/2026-04-23/", 0.05),
    ];

    let allocations = allocate_proportional(size, placements);
    let mut global_index = 0usize;
    let mut hasher = Sha256::new();

    for ((namespace, _proportion), count) in placements.iter().zip(allocations.iter()) {
        let ns_dir = dir.path().join(namespace);
        std::fs::create_dir_all(&ns_dir).expect("corpus ns dir");
        for local_i in 0..*count {
            let id = format!("mem_bench_{seed:016x}_{global_index:08}");
            let body: String = (0..rng.gen_range(8..64)).map(|_| rng.gen::<char>()).collect();
            let content = format!("---\nid: {id}\nns: {namespace}\nindex: {local_i}\n---\n{body}\n");
            let path = ns_dir.join(format!("{id}.md"));
            std::fs::write(&path, &content).expect("corpus file write");
            hasher.update(content.as_bytes());
            global_index += 1;
        }
    }

    let sha256 = format!("sha256:{}", hex::encode(hasher.finalize()));
    CorpusBuild { dir, total: global_index, sha256 }
}

/// Generate deterministic L2-normalized synthetic vectors.
///
/// Spec §17.6 / §18 boilerplate item 13: this is the **sanctioned source**
/// for bench and test vector generation. All callers — including
/// `stream_a_bench` — must use this instead of local xorshift implementations.
pub fn synthetic_vectors(seed: u64, dimension: usize, n: usize) -> Vec<Vec<f32>> {
    (0..n).map(|index| synthetic_vector_inner(seed, dimension, index)).collect()
}

/// Single synthetic L2-normalized vector at the given index.
///
/// Public so `stream_a_bench` can call it for per-iteration queries without
/// materializing the full vector set.
pub fn synthetic_vector(seed: u64, dimension: usize, index: usize) -> Vec<f32> {
    synthetic_vector_inner(seed, dimension, index)
}

fn synthetic_vector_inner(seed: u64, dimension: usize, index: usize) -> Vec<f32> {
    let mut rng = StdRng::seed_from_u64(seed ^ index as u64);
    let mut vector: Vec<f32> = (0..dimension).map(|_| rng.gen_range(-1.0_f32..1.0_f32)).collect();
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt().max(f32::EPSILON);
    for v in &mut vector {
        *v /= norm;
    }
    vector
}

/// SHA-256 of a corpus directory's content.
///
/// Walks the directory deterministically (sorted), hashes all file bytes.
/// Called by `stream_a_bench` to record `corpus_sha256` in results (spec §17.6).
///
/// Panics if any file under `corpus_dir` cannot be read: a bench corpus is
/// author-controlled, so an unreadable file is a setup bug, not a runtime
/// condition to swallow. The previous `if let Ok(...)` form silently produced
/// incorrect hashes when reads failed, which corrupted baseline fingerprints.
pub fn corpus_sha256(corpus_dir: &std::path::Path) -> String {
    let mut hasher = Sha256::new();
    let mut paths: Vec<_> = walkdir::WalkDir::new(corpus_dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    paths.sort();
    for path in paths {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|err| panic!("corpus_sha256: failed to read {}: {err}", path.display()));
        hasher.update(&bytes);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Distribute `total` items across buckets proportionally.
/// Ensures the sum equals `total` by adding remainder to the largest bucket.
fn allocate_proportional(total: usize, placements: &[(&str, f64)]) -> Vec<usize> {
    let sum: f64 = placements.iter().map(|(_, p)| p).sum();
    let mut allocated: Vec<usize> =
        placements.iter().map(|(_, p)| ((total as f64) * p / sum).floor() as usize).collect();
    let assigned: usize = allocated.iter().sum();
    let remainder = total.saturating_sub(assigned);
    if remainder > 0 {
        let largest = allocated.iter().enumerate().max_by_key(|(_, &c)| c).map(|(i, _)| i).unwrap_or(0);
        allocated[largest] += remainder;
    }
    allocated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_proportional_sums_to_total() {
        let placements = &[("a", 0.5), ("b", 0.3), ("c", 0.2)];
        for total in [0usize, 1, 7, 100, 1000] {
            let allocated = allocate_proportional(total, placements);
            assert_eq!(allocated.iter().sum::<usize>(), total, "total={total}");
            assert_eq!(allocated.len(), placements.len());
        }
    }

    #[test]
    fn allocate_proportional_zero_total_yields_zeros() {
        let placements = &[("a", 0.5), ("b", 0.5)];
        let allocated = allocate_proportional(0, placements);
        assert_eq!(allocated, vec![0, 0]);
    }

    #[test]
    fn allocate_proportional_single_bucket_takes_all() {
        let placements = &[("only", 1.0)];
        let allocated = allocate_proportional(42, placements);
        assert_eq!(allocated, vec![42]);
    }

    #[test]
    fn allocate_proportional_remainder_added_to_largest() {
        // 10 items, weights 0.6 / 0.4 -> floors to 6 + 4 = 10, no remainder.
        let placements = &[("big", 0.6), ("small", 0.4)];
        let allocated = allocate_proportional(10, placements);
        assert_eq!(allocated.iter().sum::<usize>(), 10);
        // 7 items, weights 0.6 / 0.4 -> floors to 4 + 2 = 6, remainder 1 to "big".
        let allocated = allocate_proportional(7, placements);
        assert_eq!(allocated.iter().sum::<usize>(), 7);
        assert!(allocated[0] >= allocated[1]);
    }
}
