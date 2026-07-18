use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use chrono::{Duration, Utc};
use memory_substrate::{
    config::load_config, AuthorKind, Evidence, Memory, MemoryContent, MemoryId, MemoryStatus, Substrate,
};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GroundingRehydrationError {
    #[error("grounding reference is missing: {0}")]
    Missing(String),
    #[error("grounding reference is aged out: {0}")]
    AgedOut(String),
    #[error("grounding reference drifted: {0}")]
    Drifted(String),
    #[error("grounding reference is inactive: {0}")]
    Inactive(String),
    #[error("grounding reference points at dream prose: {0}")]
    DreamProse(String),
    #[error("failed to inspect grounding reference {reference}: {message}")]
    Inspect { reference: String, message: String },
}

/// Stable protocol code for every grounding-rehydration verification failure.
/// Mirrors the [`crate::dream::types::DreamError::code`] pattern so the review
/// handler can surface a single typed refusal to the UI rather than leaking the
/// per-variant Display text as an opaque message.
pub const GROUNDING_REHYDRATION_FAILED: &str = "grounding_rehydration_failed";

impl GroundingRehydrationError {
    /// The stable protocol code carried into the typed handler refusal. All
    /// verification failures map to the same code; the variant-specific Display
    /// string carries the human-readable detail (which reference, and why).
    pub fn code(&self) -> &'static str {
        GROUNDING_REHYDRATION_FAILED
    }
}

#[derive(Debug, Clone)]
struct RehydrationConfig {
    drift_threshold: f64,
    fragment_lifetime_days: i64,
}

#[derive(Debug, Clone)]
struct GroundingCitation<'a> {
    reference: &'a str,
    quote: Option<&'a str>,
}

#[derive(Debug, Clone)]
struct FragmentRecord {
    ts: chrono::DateTime<Utc>,
    text: Option<String>,
    archived: bool,
}

struct FragmentScan<'a> {
    repo: &'a Path,
    archived: bool,
}

pub async fn verify_dream_candidate(
    substrate: &Substrate,
    candidate: &Memory,
) -> Result<(), GroundingRehydrationError> {
    if !requires_rehydration(candidate) {
        return Ok(());
    }

    let config = rehydration_config(substrate)?;
    let citations = grounding_citations(candidate);

    // Walk the `substrate/` tree at most once per verification run rather than
    // once per citation. The previous per-citation full-directory scan was
    // O(citations × fragments); a single index lookup keyed by fragment id keeps
    // verification O(fragments + citations). Build it lazily — a candidate whose
    // citations are all file/memory refs never touches `substrate/`.
    let fragment_index = if citations.iter().any(|citation| references_substrate_fragment(citation.reference)) {
        build_substrate_fragment_index(substrate.roots().repo.as_path())?
    } else {
        BTreeMap::new()
    };

    for citation in citations {
        verify_citation(substrate, &config, &fragment_index, citation).await?;
    }

    Ok(())
}

/// Whether a citation reference resolves to a substrate fragment (`sub_…`) once
/// its `memory:`/`substrate:` prefix and `#fragment` suffix are stripped. Used to
/// decide whether the per-run fragment index needs building at all.
fn references_substrate_fragment(reference: &str) -> bool {
    normalize_reference(reference).starts_with("sub_")
}

pub fn requires_rehydration(memory: &Memory) -> bool {
    memory.frontmatter.author.kind == AuthorKind::Dreaming && memory.frontmatter.grounding_rehydration_required()
}

async fn verify_citation(
    substrate: &Substrate,
    config: &RehydrationConfig,
    fragment_index: &BTreeMap<String, FragmentRecord>,
    citation: GroundingCitation<'_>,
) -> Result<(), GroundingRehydrationError> {
    let reference = normalize_reference(citation.reference);
    if is_dream_prose_ref(reference) {
        return Err(GroundingRehydrationError::DreamProse(reference.to_string()));
    }
    if reference.starts_with("sub_") {
        return verify_substrate_fragment(config, fragment_index, reference, citation.quote);
    }
    if reference.starts_with("mem_") {
        return verify_memory_ref(substrate, config, reference, citation.quote).await;
    }
    verify_file_ref(substrate.roots().repo.as_path(), config, reference, citation.quote)
}

fn verify_substrate_fragment(
    config: &RehydrationConfig,
    fragment_index: &BTreeMap<String, FragmentRecord>,
    reference: &str,
    quote: Option<&str>,
) -> Result<(), GroundingRehydrationError> {
    let Some(record) = fragment_index.get(reference) else {
        return Err(GroundingRehydrationError::Missing(reference.to_string()));
    };
    if record.archived || record.ts + Duration::days(config.fragment_lifetime_days) <= Utc::now() {
        return Err(GroundingRehydrationError::AgedOut(reference.to_string()));
    }
    if let (Some(original), Some(current)) = (quote, record.text.as_deref()) {
        verify_content_drift(reference, original, current, config.drift_threshold)?;
    }
    Ok(())
}

async fn verify_memory_ref(
    substrate: &Substrate,
    config: &RehydrationConfig,
    reference: &str,
    quote: Option<&str>,
) -> Result<(), GroundingRehydrationError> {
    let id = MemoryId::try_new(reference).map_err(|error| GroundingRehydrationError::Inspect {
        reference: reference.to_string(),
        message: error.to_string(),
    })?;
    let envelope = substrate
        .read_memory_envelope(&id)
        .await
        .map_err(|_| GroundingRehydrationError::Missing(reference.to_string()))?;

    if !is_acceptable_grounding_memory(envelope.metadata.frontmatter.status, envelope.metadata.frontmatter.trust_level)
    {
        return Err(GroundingRehydrationError::Inactive(reference.to_string()));
    }

    if let (Some(original), MemoryContent::Plaintext(current)) = (quote, envelope.content) {
        verify_content_drift(reference, original, &current, config.drift_threshold)?;
    }
    Ok(())
}

fn is_acceptable_grounding_memory(status: MemoryStatus, trust_level: memory_substrate::TrustLevel) -> bool {
    matches!(status, MemoryStatus::Active | MemoryStatus::Pinned)
        && matches!(trust_level, memory_substrate::TrustLevel::Trusted | memory_substrate::TrustLevel::Pinned)
}

fn verify_file_ref(
    repo: &Path,
    config: &RehydrationConfig,
    reference: &str,
    quote: Option<&str>,
) -> Result<(), GroundingRehydrationError> {
    let path = resolve_repo_relative_file_ref(repo, reference)?;
    if !path.is_file() {
        return Err(GroundingRehydrationError::Missing(reference.to_string()));
    }
    if let Some(original) = quote {
        let current = fs::read_to_string(&path).map_err(|error| GroundingRehydrationError::Inspect {
            reference: reference.to_string(),
            message: error.to_string(),
        })?;
        verify_content_drift(reference, original, &current, config.drift_threshold)?;
    }
    Ok(())
}

fn rehydration_config(substrate: &Substrate) -> Result<RehydrationConfig, GroundingRehydrationError> {
    let dreams = load_config(substrate.roots().repo.as_path(), substrate.roots().runtime.as_path(), None)
        .map(|loaded| loaded.synced.dreams)
        .map_err(|error| GroundingRehydrationError::Inspect { reference: "config.yaml".to_string(), message: error })?;
    Ok(RehydrationConfig {
        drift_threshold: dreams.pass_2_drift_threshold,
        fragment_lifetime_days: i64::from(dreams.fragment_lifetime_days),
    })
}

fn grounding_citations(memory: &Memory) -> Vec<GroundingCitation<'_>> {
    let mut citations = Vec::new();
    if let Some(reference) = memory.frontmatter.source.reference.as_deref() {
        citations.push(GroundingCitation { reference, quote: None });
    }
    citations.extend(memory.frontmatter.evidence.iter().map(citation_from_evidence));
    citations
}

fn citation_from_evidence(evidence: &Evidence) -> GroundingCitation<'_> {
    GroundingCitation {
        reference: evidence.reference.as_str(),
        quote: (!evidence.quote.trim().is_empty()).then_some(evidence.quote.as_str()),
    }
}

/// Build a `fragment_id -> record` index by walking the `substrate/` tree once.
///
/// Reproduces the prior per-citation scanner's precedence exactly: the active
/// pass walks all of `substrate/` (including the nested `archive/` subtree, whose
/// records it tags `archived: false`, matching the old `archived: false` scan),
/// then the archive pass fills only ids not yet seen. First occurrence wins, so a
/// repeat id never overwrites an earlier record. This collapses what was one
/// full-tree walk *per citation* into a single walk per verification run.
fn build_substrate_fragment_index(repo: &Path) -> Result<BTreeMap<String, FragmentRecord>, GroundingRehydrationError> {
    let mut index = BTreeMap::new();
    index_jsonl_tree(&FragmentScan { repo, archived: false }, &repo.join("substrate"), &mut index)?;
    index_jsonl_tree(&FragmentScan { repo, archived: true }, &repo.join("substrate/archive"), &mut index)?;
    Ok(index)
}

fn index_jsonl_tree(
    scan: &FragmentScan<'_>,
    directory: &Path,
    index: &mut BTreeMap<String, FragmentRecord>,
) -> Result<(), GroundingRehydrationError> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|error| GroundingRehydrationError::Inspect {
        reference: repo_relative(scan.repo, directory),
        message: error.to_string(),
    })? {
        let entry = entry.map_err(|error| GroundingRehydrationError::Inspect {
            reference: repo_relative(scan.repo, directory),
            message: error.to_string(),
        })?;
        let path = entry.path();
        if path.is_dir() {
            index_jsonl_tree(scan, &path, index)?;
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            index_jsonl_file(&path, scan.archived, index)?;
        }
    }
    Ok(())
}

fn index_jsonl_file(
    path: &Path,
    archived: bool,
    index: &mut BTreeMap<String, FragmentRecord>,
) -> Result<(), GroundingRehydrationError> {
    let text = fs::read_to_string(path).map_err(|error| GroundingRehydrationError::Inspect {
        reference: path.display().to_string(),
        message: error.to_string(),
    })?;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|error| GroundingRehydrationError::Inspect {
            reference: path.display().to_string(),
            message: error.to_string(),
        })?;
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        if index.contains_key(id) {
            // First occurrence wins, matching the prior scanner's first-match-stop.
            continue;
        }
        let ts = value
            .get("ts")
            .and_then(Value::as_str)
            .ok_or_else(|| GroundingRehydrationError::Inspect {
                reference: id.to_string(),
                message: "substrate fragment missing ts".to_string(),
            })?
            .parse::<chrono::DateTime<Utc>>()
            .map_err(|error| GroundingRehydrationError::Inspect {
                reference: id.to_string(),
                message: error.to_string(),
            })?;
        index.insert(
            id.to_string(),
            FragmentRecord { ts, text: value.get("text").and_then(Value::as_str).map(str::to_string), archived },
        );
    }
    Ok(())
}

fn verify_content_drift(
    reference: &str,
    original: &str,
    current: &str,
    threshold: f64,
) -> Result<(), GroundingRehydrationError> {
    if original.is_empty() || current.contains(original) {
        return Ok(());
    }
    let distance = levenshtein(original, current);
    let allowed = (original.len() as f64 * threshold).ceil() as usize;
    if distance > allowed {
        Err(GroundingRehydrationError::Drifted(reference.to_string()))
    } else {
        Ok(())
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution = usize::from(left_char != *right_char);
            current[right_index + 1] =
                (previous[right_index + 1] + 1).min(current[right_index] + 1).min(previous[right_index] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()]
}

/// Strip a citation reference's `memory:`/`substrate:` prefix and `#fragment`
/// suffix down to its bare id/path. Shared with the fragment-archival deferral
/// path (`fragment_archival.rs`) so citation-id mapping stays identical between
/// grounding rehydration and cleanup-time citation counting.
pub(crate) fn normalize_reference(reference: &str) -> &str {
    let without_prefix = reference
        .strip_prefix("memory:")
        .or_else(|| reference.strip_prefix("substrate:"))
        .or_else(|| reference.strip_prefix("substrate_fragment:"))
        .unwrap_or(reference);
    without_prefix.split_once('#').map_or(without_prefix, |(path, _fragment)| path)
}

pub(crate) fn resolve_repo_relative_file_ref(
    repo: &Path,
    reference: &str,
) -> Result<PathBuf, GroundingRehydrationError> {
    let without_file_prefix = reference.strip_prefix("file:").unwrap_or(reference);
    let normalized = without_file_prefix.split_once('#').map_or(without_file_prefix, |(path, _)| path);
    if normalized.is_empty() || normalized.contains('\0') || normalized.contains('\\') {
        return Err(inspect_ref(reference, "file ref must be a non-empty repo-relative path"));
    }
    let path = Path::new(normalized);
    if path.is_absolute() {
        return Err(inspect_ref(reference, "absolute file refs are not allowed"));
    }
    if path.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(inspect_ref(reference, "file refs may not contain . or .. components"));
    }

    let repo = repo.canonicalize().map_err(|error| inspect_ref(reference, error.to_string()))?;
    let resolved = repo.join(path);
    if resolved.exists() {
        let canonical = resolved.canonicalize().map_err(|error| inspect_ref(reference, error.to_string()))?;
        if !canonical.starts_with(&repo) {
            return Err(inspect_ref(reference, "file ref escapes repository root"));
        }
        return Ok(canonical);
    }
    if let Some(parent) = resolved.parent().filter(|parent| parent.exists()) {
        let canonical_parent = parent.canonicalize().map_err(|error| inspect_ref(reference, error.to_string()))?;
        if !canonical_parent.starts_with(&repo) {
            return Err(inspect_ref(reference, "file ref parent escapes repository root"));
        }
    }
    Ok(resolved)
}

fn inspect_ref(reference: &str, message: impl Into<String>) -> GroundingRehydrationError {
    GroundingRehydrationError::Inspect { reference: reference.to_string(), message: message.into() }
}

fn is_dream_prose_ref(reference: &str) -> bool {
    let without_file_prefix = reference.strip_prefix("file:").unwrap_or(reference);
    without_file_prefix
        .split('/')
        .collect::<Vec<_>>()
        .windows(3)
        .any(|window| window[0] == "dreams" && matches!(window[1], "journal" | "questions"))
}

fn repo_relative(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo).unwrap_or(path).display().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{build_substrate_fragment_index, levenshtein};

    #[test]
    fn levenshtein_counts_basic_edits() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    fn write_jsonl(path: &Path, lines: &[&str]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create jsonl parent");
        }
        fs::write(path, lines.join("\n")).expect("write jsonl");
    }

    fn record(id: &str, ts: &str, text: &str) -> String {
        format!(r#"{{"id":"{id}","ts":"{ts}","text":"{text}"}}"#)
    }

    /// One walk resolves every id across nested directories and multiple files,
    /// replacing the prior per-citation full-tree scan.
    #[test]
    fn fragment_index_resolves_ids_across_nested_files_in_one_walk() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        write_jsonl(&repo.join("substrate/2026/04/a.jsonl"), &[&record("sub_a", "2026-04-30T00:00:00Z", "alpha")]);
        write_jsonl(
            &repo.join("substrate/2026/05/b.jsonl"),
            &[&record("sub_b", "2026-05-01T00:00:00Z", "beta"), &record("sub_c", "2026-05-02T00:00:00Z", "gamma")],
        );

        let index = build_substrate_fragment_index(repo).expect("index");

        assert_eq!(index.get("sub_a").and_then(|r| r.text.as_deref()), Some("alpha"));
        assert_eq!(index.get("sub_b").and_then(|r| r.text.as_deref()), Some("beta"));
        assert_eq!(index.get("sub_c").and_then(|r| r.text.as_deref()), Some("gamma"));
        assert!(!index.contains_key("sub_missing"));
        // Every record in the active tree is tagged active (the prior scanner's
        // `archived: false` pass covered all of `substrate/`).
        assert!(index.values().all(|record| !record.archived));
    }

    /// First occurrence wins for a repeated id, matching the prior scanner's
    /// first-match-then-stop behavior within a file.
    #[test]
    fn fragment_index_keeps_first_occurrence_for_repeated_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        write_jsonl(
            &repo.join("substrate/log.jsonl"),
            &[&record("sub_x", "2026-04-30T00:00:00Z", "first"), &record("sub_x", "2026-05-30T00:00:00Z", "second")],
        );

        let index = build_substrate_fragment_index(repo).expect("index");

        assert_eq!(index.get("sub_x").and_then(|r| r.text.as_deref()), Some("first"));
    }

    /// Equivalence guard for the active-walk precedence. The prior per-citation
    /// scanner walked all of `substrate/` (which physically contains the nested
    /// `archive/` subtree) on its `archived: false` pass and stopped at the first
    /// match, so a fragment physically under `substrate/archive` was reached and
    /// tagged active before the dedicated archive pass ran. The index reproduces
    /// that exactly: the fragment is found and tagged active, not archived. A
    /// fragment is only tagged archived when the active walk cannot reach it.
    #[test]
    fn fragment_index_reproduces_active_walk_precedence_over_nested_archive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        write_jsonl(
            &repo.join("substrate/archive/old.jsonl"),
            &[&record("sub_archived", "2026-01-01T00:00:00Z", "stale")],
        );

        let index = build_substrate_fragment_index(repo).expect("index");

        let found = index.get("sub_archived").expect("archive-subtree fragment indexed via active walk");
        assert!(!found.archived, "active walk reaches the nested archive subtree first, tagging it active");
        assert_eq!(found.text.as_deref(), Some("stale"));
    }

    /// An empty repo (no `substrate/` tree) yields an empty index rather than an
    /// error — a candidate with no substrate citations never pays for a walk.
    #[test]
    fn fragment_index_is_empty_without_substrate_tree() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = build_substrate_fragment_index(temp.path()).expect("index");
        assert!(index.is_empty());
    }
}
