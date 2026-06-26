//! Canonical-content convergence comparison helpers per spec §13.6.1.
//!
//! `roots_converged` implements the spec's canonical-content equality, not
//! raw byte equality. See `roots_byte_equal` for the weaker byte-level check.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Result of comparing two repo roots for canonical convergence.
pub struct ConvergenceReport {
    /// True only when all boundaries match.
    pub converged: bool,
    /// First divergence found, with structured details.
    pub first_divergence: Option<DivergenceDetail>,
}

/// Details about the first divergent path.
pub struct DivergenceDetail {
    /// Repo-relative path where divergence was found.
    pub path: PathBuf,
    /// Nature of the divergence.
    pub kind: DivergenceKind,
    /// Human-readable diff summary.
    pub diff: String,
}

/// How two files diverge.
pub enum DivergenceKind {
    /// File exists only in the left root.
    OnlyInLeft,
    /// File exists only in the right root.
    OnlyInRight,
    /// Markdown file canonical content differs.
    DifferentMarkdown,
    /// JSONL file canonical record set differs.
    DifferentJsonl,
    /// Byte-compared file (YAML, gitattributes, etc.) differs.
    DifferentBytes,
}

/// Compare two repo roots for spec §13.6.1 canonical-content equality.
///
/// - `.md` files: parse → re-serialize via serde_yaml (canonical) → compare.
/// - `.jsonl` files: parse each line as JSON, sort by
///   `(ts, device, seq, id)`, re-serialize → compare.
/// - Everything else (YAML, gitattributes, etc.): byte-compare directly.
///
/// `target/`, `.git/`, `.memoryd/` are excluded from the walk.
pub fn roots_converged(left: &Path, right: &Path) -> ConvergenceReport {
    match compare_roots(left, right) {
        Ok(report) => report,
        Err(err) => ConvergenceReport {
            converged: false,
            first_divergence: Some(DivergenceDetail {
                path: PathBuf::from("<io error>"),
                kind: DivergenceKind::DifferentBytes,
                diff: err.to_string(),
            }),
        },
    }
}

/// Weaker byte-equality check for cases where canonical equality is not needed.
///
/// Returns `Ok(true)` when the two roots are byte-for-byte identical (excluding
/// `.git/`, `.memoryd/`, `target/`). Diverges from spec §13.6.1 because JSONL
/// line order is not normalised.
pub fn roots_byte_equal(left: &Path, right: &Path) -> anyhow::Result<bool> {
    Ok(snapshot_bytes(left)? == snapshot_bytes(right)?)
}

fn compare_roots(left: &Path, right: &Path) -> anyhow::Result<ConvergenceReport> {
    let left_files = collect_relative_paths(left)?;
    let right_files = collect_relative_paths(right)?;

    for path in &left_files {
        if !right_files.contains(path) {
            return Ok(diverged(
                path.clone(),
                DivergenceKind::OnlyInLeft,
                format!("{} exists only in left", path.display()),
            ));
        }
    }
    for path in &right_files {
        if !left_files.contains(path) {
            return Ok(diverged(
                path.clone(),
                DivergenceKind::OnlyInRight,
                format!("{} exists only in right", path.display()),
            ));
        }
    }

    // Compare each shared file using content-appropriate comparison.
    for path in &left_files {
        let left_bytes = std::fs::read(left.join(path)).context("read left")?;
        let right_bytes = std::fs::read(right.join(path)).context("read right")?;

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext {
                "md" => {
                    let diff = compare_markdown(path, &left_bytes, &right_bytes)?;
                    if let Some(diff) = diff {
                        return Ok(diverged(path.clone(), DivergenceKind::DifferentMarkdown, diff));
                    }
                    continue;
                }
                "jsonl" => {
                    let diff = compare_jsonl(path, &left_bytes, &right_bytes)?;
                    if let Some(diff) = diff {
                        return Ok(diverged(path.clone(), DivergenceKind::DifferentJsonl, diff));
                    }
                    continue;
                }
                _ => {}
            }
        }

        // Default: byte compare.
        if left_bytes != right_bytes {
            return Ok(diverged(
                path.clone(),
                DivergenceKind::DifferentBytes,
                format!("{} differs ({} vs {} bytes)", path.display(), left_bytes.len(), right_bytes.len()),
            ));
        }
    }

    Ok(ConvergenceReport { converged: true, first_divergence: None })
}

/// Compare two Markdown files at the canonical-content level.
///
/// Parses frontmatter + body, re-serializes via serde_yaml (sorted keys),
/// and byte-compares the result.
///
/// Falls back to byte comparison when the parse fails (preserving error
/// visibility rather than silently declaring convergence).
fn compare_markdown(path: &Path, left: &[u8], right: &[u8]) -> anyhow::Result<Option<String>> {
    let left_canonical = canonical_markdown(path, left);
    let right_canonical = canonical_markdown(path, right);

    match (left_canonical, right_canonical) {
        (Ok(l), Ok(r)) => {
            if l != r {
                Ok(Some(format!(
                    "{}: markdown canonical content differs ({} vs {} bytes)",
                    path.display(),
                    l.len(),
                    r.len()
                )))
            } else {
                Ok(None)
            }
        }
        // If one or both parse fails, fall back to byte compare.
        (Err(_), Err(_)) => {
            if left == right {
                Ok(None)
            } else {
                Ok(Some(format!("{}: unparsable markdown differs", path.display())))
            }
        }
        _ => Ok(Some(format!("{}: one side parsable, other not", path.display()))),
    }
}

fn canonical_markdown(path: &Path, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes).context("markdown utf8")?;
    let (frontmatter_yaml, body) = split_frontmatter(path, text)?;
    // Parse and re-serialize via serde_yaml (canonical: sorted keys, no anchors).
    let value: serde_yaml::Value = serde_yaml::from_str(frontmatter_yaml).context("frontmatter parse")?;
    let re_serialized = serde_yaml::to_string(&value).context("frontmatter serialize")?;
    let canonical = format!("---\n{re_serialized}---\n{body}");
    Ok(canonical.into_bytes())
}

fn split_frontmatter<'a>(path: &Path, text: &'a str) -> anyhow::Result<(&'a str, &'a str)> {
    let text = text.strip_prefix("---\n").ok_or_else(|| anyhow::anyhow!("{}: missing opening ---", path.display()))?;
    let end = text.find("\n---\n").ok_or_else(|| anyhow::anyhow!("{}: missing closing ---", path.display()))?;
    let frontmatter = &text[..end];
    let body = &text[end + 5..]; // skip "\n---\n"
    Ok((frontmatter, body))
}

/// Compare two JSONL files at the canonical-content level per spec §13.6.1.
///
/// Parses each line as a JSON object, sorts by `(ts, device, seq, id)` tuple,
/// re-serializes, and byte-compares. This makes line-order differences
/// irrelevant (as required by the union-merge strategy for event logs).
fn compare_jsonl(path: &Path, left: &[u8], right: &[u8]) -> anyhow::Result<Option<String>> {
    let left_canon = canonical_jsonl(path, left)?;
    let right_canon = canonical_jsonl(path, right)?;
    if left_canon != right_canon {
        Ok(Some(format!(
            "{}: jsonl canonical set differs ({} vs {} records)",
            path.display(),
            left_canon.len(),
            right_canon.len()
        )))
    } else {
        Ok(None)
    }
}

fn canonical_jsonl(path: &Path, bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    let text = std::str::from_utf8(bytes).context("jsonl utf8")?;
    let mut records: Vec<(SortKey, String)> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(trimmed).with_context(|| format!("{}: jsonl parse", path.display()))?;
        let key = SortKey::from_value(&value);
        let canonical = serde_json::to_string(&value).context("jsonl re-serialize")?;
        records.push((key, canonical));
    }

    records.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(records.into_iter().map(|(_, s)| s).collect())
}

/// Sort key for JSONL records: `(ts, device, seq, id)` per spec §13.6.1.
#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct SortKey(String, String, u64, String);

impl SortKey {
    fn from_value(value: &serde_json::Value) -> Self {
        let ts = value.get("ts").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let device = value.get("device").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let seq = value.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        SortKey(ts, device, seq, id)
    }
}

fn collect_relative_paths(root: &Path) -> anyhow::Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry.context("walkdir")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(root).context("strip prefix")?;
        if should_ignore(rel) {
            continue;
        }
        paths.insert(rel.to_path_buf());
    }
    Ok(paths)
}

fn should_ignore(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        // Exclude build artifacts and runtime directories.
        // Note: spec §13.6.1 does not exclude `target/`; we do because it's a
        // Cargo build artifact that should never appear under the repo root.
        matches!(s.as_ref(), ".git" | ".memoryd" | "target")
    })
}

fn snapshot_bytes(root: &Path) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.context("walkdir")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(root).context("strip prefix")?.to_path_buf();
        if should_ignore(&relative) {
            continue;
        }
        files.insert(relative, std::fs::read(entry.path())?);
    }
    Ok(files)
}

fn diverged(path: PathBuf, kind: DivergenceKind, diff: String) -> ConvergenceReport {
    ConvergenceReport { converged: false, first_divergence: Some(DivergenceDetail { path, kind, diff }) }
}
