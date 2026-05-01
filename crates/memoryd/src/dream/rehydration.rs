use std::{
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
    id: &'a str,
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
    for citation in grounding_citations(candidate) {
        verify_citation(substrate, &config, citation).await?;
    }

    Ok(())
}

pub fn requires_rehydration(memory: &Memory) -> bool {
    memory.frontmatter.author.kind == AuthorKind::Dreaming && memory.frontmatter.grounding_rehydration_required()
}

async fn verify_citation(
    substrate: &Substrate,
    config: &RehydrationConfig,
    citation: GroundingCitation<'_>,
) -> Result<(), GroundingRehydrationError> {
    let reference = normalize_reference(citation.reference);
    if is_dream_prose_ref(reference) {
        return Err(GroundingRehydrationError::DreamProse(reference.to_string()));
    }
    if reference.starts_with("sub_") {
        return verify_substrate_fragment(substrate, config, reference, citation.quote);
    }
    if reference.starts_with("mem_") {
        return verify_memory_ref(substrate, config, reference, citation.quote).await;
    }
    verify_file_ref(substrate.roots().repo.as_path(), config, reference, citation.quote)
}

fn verify_substrate_fragment(
    substrate: &Substrate,
    config: &RehydrationConfig,
    reference: &str,
    quote: Option<&str>,
) -> Result<(), GroundingRehydrationError> {
    let Some(record) = find_substrate_fragment(substrate.roots().repo.as_path(), reference)? else {
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

fn find_substrate_fragment(repo: &Path, id: &str) -> Result<Option<FragmentRecord>, GroundingRehydrationError> {
    let mut found = None;
    scan_jsonl_records(&FragmentScan { repo, id, archived: false }, &repo.join("substrate"), &mut found)?;
    if found.is_none() {
        scan_jsonl_records(&FragmentScan { repo, id, archived: true }, &repo.join("substrate/archive"), &mut found)?;
    }
    Ok(found)
}

fn scan_jsonl_records(
    scan: &FragmentScan<'_>,
    directory: &Path,
    found: &mut Option<FragmentRecord>,
) -> Result<(), GroundingRehydrationError> {
    if found.is_some() || !directory.exists() {
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
            scan_jsonl_records(scan, &path, found)?;
            if found.is_some() {
                return Ok(());
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            scan_jsonl_file(&path, scan.id, scan.archived, found)?;
            if found.is_some() {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn scan_jsonl_file(
    path: &Path,
    id: &str,
    archived: bool,
    found: &mut Option<FragmentRecord>,
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
        if value.get("id").and_then(Value::as_str) != Some(id) {
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
        *found =
            Some(FragmentRecord { ts, text: value.get("text").and_then(Value::as_str).map(str::to_string), archived });
        return Ok(());
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

fn normalize_reference(reference: &str) -> &str {
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
    use super::levenshtein;

    #[test]
    fn levenshtein_counts_basic_edits() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}
