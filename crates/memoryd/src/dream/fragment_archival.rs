//! Citation-aware substrate fragment archival (Memory Dynamics spec v0.1 §4).
//!
//! The cleanup layer normally archives plaintext substrate fragments at a hard
//! lifetime cutoff (`dreams.fragment_lifetime_days`, default 14). This module
//! adds the dynamics amendment: a fragment that is still being **cited** by live
//! memories has its archival deferred — but bounded, so nothing becomes immortal
//! by citation alone.
//!
//! ## Deferral rule (spec §4)
//!
//! For each expired fragment (`ts + base_lifetime <= now`):
//! - if it is cited by `>= dynamics.citation_defer_threshold` distinct live
//!   memories **and** has not yet reached the immortality cap
//!   (`ts + max_fragment_lifetime_days > now`), archival is deferred — the
//!   fragment stays in the active tree;
//! - otherwise it archives on schedule.
//!
//! At the cap, archival proceeds regardless of citations. Deferral changes *when*
//! a fragment archives, never *whether*.
//!
//! ## Citation source (spec §4, the structured one — not journal prose)
//!
//! Citations are counted from `Evidence.reference` (`evidence: Vec<Evidence>`)
//! entries in live memory frontmatter — the same refs grounding rehydration
//! resolves. Multiple references to the same fragment inside one memory count as
//! one citing memory. Each ref is normalized via
//! `crate::dream::rehydration::normalize_reference` and kept only when it names a
//! `sub_…` fragment. Dream-journal markdown is deliberately never scanned: it is
//! prose, and grepping it for fragment ids is brittle by construction.
//!
//! ## Gating (spec §7)
//!
//! Deferral is active only when `dynamics.enabled` (default `true`). With
//! dynamics disabled, this module is bypassed entirely and the cleanup layer
//! falls back to the substrate's hard-cutoff archival — byte-identical to
//! pre-dynamics behavior.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use memory_substrate::tree::relative_memory_paths;
use memory_substrate::{Memory, MemoryStatus};
use serde_json::Value;

use crate::dream::rehydration::normalize_reference;
use crate::dream::report::DeferredFragment;
use crate::dynamics::DynamicsConfig;

/// Outcome of one citation-aware archival pass over a device's active fragments.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FragmentArchivalOutcome {
    /// Number of fragments newly archived this pass.
    pub fragments_archived: usize,
    /// Fragments whose archival was deferred because they are still cited and
    /// have not yet reached the immortality cap. Sorted by fragment id.
    pub deferred_fragments: Vec<DeferredFragment>,
    /// Repo-relative files mutated (active fragment files rewritten and archive
    /// files appended). Empty when the pass is a no-op.
    pub mutated_files: BTreeSet<String>,
}

/// Run the citation-aware archival pass for a single device.
///
/// Walks `substrate/<device>/*.jsonl`, partitions each file into kept (live or
/// deferred) and archived records per the spec §4 rule, rewrites the active file
/// with the kept records, and appends archived records to
/// `substrate/archive/<device>/<YYYY-MM>.jsonl` (matching the substrate's own
/// archive layout, dedup-by-id, sorted).
///
/// `base_lifetime_days` is `dreams.fragment_lifetime_days` (the same value the
/// non-dynamics path passes to the substrate). The immortality cap is taken from
/// `dynamics.max_fragment_lifetime_days`; if it is misconfigured below the base
/// lifetime it is clamped up to the base so deferral can never *shorten* a
/// fragment's life.
#[expect(
    clippy::too_many_arguments,
    reason = "cleanup-call contract: repo, device, clock, base lifetime, and dynamics knobs are distinct inputs"
)]
pub fn archive_with_deferral(
    repo: &Path,
    device_id: &str,
    now: DateTime<Utc>,
    base_lifetime_days: i64,
    dynamics: &DynamicsConfig,
) -> std::io::Result<FragmentArchivalOutcome> {
    let citations = count_fragment_citations(repo);
    let cap_days = i64::from(dynamics.max_fragment_lifetime_days).max(base_lifetime_days);
    let window = DeferralWindow {
        base_cutoff: now - Duration::days(base_lifetime_days),
        cap_cutoff: now - Duration::days(cap_days),
        cap_days,
        threshold: u64::from(dynamics.citation_defer_threshold),
        citations: &citations,
    };

    let device_dir = repo.join("substrate").join(device_id);
    let mut outcome = FragmentArchivalOutcome::default();
    if !device_dir.exists() {
        return Ok(outcome);
    }

    // Archive batches keyed by the same monthly bucket the substrate uses. Each
    // entry carries the original line text verbatim so re-serialization never
    // reorders keys versus what the substrate wrote (byte-stable archives).
    let mut archive_batches: BTreeMap<String, Vec<ArchivedLine>> = BTreeMap::new();

    let mut entries =
        fs::read_dir(&device_dir)?.map(|entry| entry.map(|entry| entry.path())).collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();

    for file_path in entries {
        if file_path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let lines = read_jsonl_lines(&file_path)?;
        let mut kept: Vec<String> = Vec::new();
        let mut archived_here = false;

        for line in lines {
            match window.classify(&line) {
                FragmentDecision::Keep => kept.push(line),
                FragmentDecision::Defer { fragment_id, citations: count, ts } => {
                    kept.push(line);
                    outcome.deferred_fragments.push(DeferredFragment {
                        fragment_id,
                        citations: count,
                        cap_deadline: ts + Duration::days(window.cap_days),
                    });
                }
                FragmentDecision::Archive { id, ts } => {
                    let month = ts.format("%Y-%m").to_string();
                    archive_batches.entry(month).or_default().push(ArchivedLine { id, text: line });
                    archived_here = true;
                }
            }
        }

        if archived_here {
            rewrite_lines(&file_path, &kept)?;
            outcome.mutated_files.insert(repo_relative(repo, &file_path));
        }
    }

    for (month, new_records) in archive_batches {
        let archive_path = repo.join("substrate/archive").join(device_id).join(format!("{month}.jsonl"));
        let archived = merge_into_archive(&archive_path, new_records)?;
        outcome.fragments_archived += archived;
        outcome.mutated_files.insert(repo_relative(repo, &archive_path));
    }

    outcome.deferred_fragments.sort_by(|left, right| left.fragment_id.cmp(&right.fragment_id));
    Ok(outcome)
}

/// One archived fragment: its id (for dedup/sort) and the original line text.
struct ArchivedLine {
    id: Option<String>,
    text: String,
}

enum FragmentDecision {
    /// Live (not yet expired) — left in place untouched.
    Keep,
    /// Expired-at-base but cited and under the cap — kept, deferral recorded.
    Defer { fragment_id: String, citations: u64, ts: DateTime<Utc> },
    /// Archives this pass.
    Archive { id: Option<String>, ts: DateTime<Utc> },
}

/// The decision inputs for one archival pass: the base/cap cutoffs, the citation
/// threshold, and the per-fragment citation counts. Bundled so the per-line
/// classifier reads them through one borrow.
struct DeferralWindow<'a> {
    /// Fragments with `ts <= base_cutoff` are expired at the base lifetime.
    base_cutoff: DateTime<Utc>,
    /// Fragments with `ts <= cap_cutoff` are past the immortality cap and archive
    /// regardless of citations.
    cap_cutoff: DateTime<Utc>,
    /// Total fragment lifetime in days (base, extended by deferral, capped).
    cap_days: i64,
    /// Minimum citation count for deferral eligibility.
    threshold: u64,
    /// Per-fragment citation counts from live memories.
    citations: &'a BTreeMap<String, u64>,
}

impl DeferralWindow<'_> {
    fn classify(&self, line: &str) -> FragmentDecision {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            // An unparseable line is never expired-eligible; keep it so a
            // malformed line survives for a human rather than silently archiving.
            return FragmentDecision::Keep;
        };
        let Some(ts) = record.get("ts").and_then(Value::as_str).and_then(|raw| raw.parse::<DateTime<Utc>>().ok())
        else {
            return FragmentDecision::Keep;
        };
        let id = record.get("id").and_then(Value::as_str).map(str::to_string);
        if ts > self.base_cutoff {
            return FragmentDecision::Keep;
        }
        // Expired at the base lifetime. At or past the immortality cap, archive
        // regardless of citations.
        if ts <= self.cap_cutoff {
            return FragmentDecision::Archive { id, ts };
        }
        let count = id.as_deref().and_then(|id| self.citations.get(id)).copied().unwrap_or(0);
        if let Some(fragment_id) = id.clone() {
            if count >= self.threshold && self.threshold > 0 {
                return FragmentDecision::Defer { fragment_id, citations: count, ts };
            }
        }
        FragmentDecision::Archive { id, ts }
    }
}

/// Count distinct live memories citing substrate fragments via
/// `Evidence.reference` (spec §4 citation source).
///
/// Only memories in a live status are scanned — `Active`, `Pinned` (active
/// canonical) and `Candidate` / `Quarantined` (queued for review, the "queued
/// candidates" of the spec). Archived, superseded and tombstoned memories are no
/// longer live citers and do not keep a fragment alive. Evidence refs are
/// normalized identically to grounding rehydration and counted only when they
/// name a `sub_…` fragment. Duplicate refs to the same fragment inside one
/// memory count once.
fn count_fragment_citations(repo: &Path) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for relative in relative_memory_paths(repo) {
        let Ok(text) = fs::read_to_string(repo.join(&relative)) else {
            continue;
        };
        let Ok(parsed) = memory_substrate::frontmatter::parse_document(&text, None) else {
            continue;
        };
        accumulate_citations(&parsed.memory, &mut counts);
    }
    counts
}

fn accumulate_citations(memory: &Memory, counts: &mut BTreeMap<String, u64>) {
    if !is_live_citer(memory.frontmatter.status) {
        return;
    }
    let mut cited_fragments = BTreeSet::new();
    for evidence in &memory.frontmatter.evidence {
        let normalized = normalize_reference(&evidence.reference);
        if let Some(id) = normalized.strip_prefix("sub_") {
            cited_fragments.insert(format!("sub_{id}"));
        }
    }
    for fragment_id in cited_fragments {
        *counts.entry(fragment_id).or_insert(0) += 1;
    }
}

/// Whether a memory in this status keeps a cited fragment alive.
fn is_live_citer(status: MemoryStatus) -> bool {
    matches!(status, MemoryStatus::Active | MemoryStatus::Pinned | MemoryStatus::Candidate | MemoryStatus::Quarantined)
}

/// Read a jsonl file into its non-empty lines, verbatim (no re-serialization).
fn read_jsonl_lines(path: &Path) -> std::io::Result<Vec<String>> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect())
}

/// Extract the `id` field from a raw jsonl line, for archive dedup/sort.
fn line_id(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line).ok()?.get("id").and_then(Value::as_str).map(str::to_string)
}

/// Append new archived lines into a monthly archive file, dedup-by-id, sorted by
/// id — matching the substrate's own archive write semantics. Lines are carried
/// verbatim so key ordering stays byte-stable with what the substrate wrote.
/// Returns the number of records actually added (already-present ids are not
/// double-counted).
fn merge_into_archive(archive_path: &Path, new_records: Vec<ArchivedLine>) -> std::io::Result<usize> {
    let mut lines = if archive_path.exists() { read_jsonl_lines(archive_path)? } else { Vec::new() };
    let mut seen: BTreeSet<String> = lines.iter().filter_map(|line| line_id(line)).collect();
    let mut added = 0;
    for record in new_records {
        match record.id {
            Some(id) if seen.contains(&id) => {}
            Some(id) => {
                seen.insert(id);
                lines.push(record.text);
                added += 1;
            }
            None => {
                lines.push(record.text);
                added += 1;
            }
        }
    }
    lines.sort_by(|left, right| {
        let left_id = line_id(left).unwrap_or_default();
        let right_id = line_id(right).unwrap_or_default();
        left_id.cmp(&right_id)
    });
    rewrite_lines(archive_path, &lines)?;
    Ok(added)
}

/// Atomically rewrite a jsonl file with the given verbatim lines (temp file +
/// rename), mirroring the durability posture of the cleanup layer's other
/// writers.
fn rewrite_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("substrate.jsonl");
    let temp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temp);
    {
        let mut file = fs::File::create(&temp)?;
        for line in lines {
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn repo_relative(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo).unwrap_or(path).to_string_lossy().replace('\\', "/")
}
