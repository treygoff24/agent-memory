//! Tree validation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use std::sync::LazyLock;

use crate::error::{ValidationError, ValidationWarning};
use crate::frontmatter::parse_document;
use crate::model::{MemoryId, RepoPath};
use crate::path_validation::is_noncanonical_stream_f_repo_path;
use crate::tree::layout::relative_memory_paths;

/// Slug pattern per spec §5.1: `[a-z0-9][a-z0-9-]{0,62}`.
#[allow(clippy::expect_used)]
static SLUG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,62}$").expect("slug regex literal")); // expect-justified: compile-time regex

/// ISO date pattern for path segments like `2026-04-24`.
#[allow(clippy::expect_used)]
static ISO_DATE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").expect("iso-date regex literal")); // expect-justified: compile-time regex

/// Year-month pattern for Stream F archive files like `2026-04`.
#[allow(clippy::expect_used)]
static YEAR_MONTH_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}$").expect("year-month regex literal")); // expect-justified: compile-time regex

/// ID-based filename prefix; these are validated separately.
const MEM_PREFIX: &str = "mem_";

/// Tree validation mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeValidationMode {
    /// Partial sync allows missing references as warnings.
    PartialSync,
    /// Fully synced mode treats missing references as errors.
    FullySynced,
}

/// Tree validation report.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TreeValidationReport {
    /// Warnings.
    pub warnings: Vec<ValidationWarning>,
    /// Parsed id to path map.
    pub ids: HashMap<MemoryId, RepoPath>,
}

/// Validate tree structure and cross-file references.
pub fn validate_tree(root: &Path, mode: TreeValidationMode) -> Result<TreeValidationReport, ValidationError> {
    let mut report = TreeValidationReport::default();
    let mut folded_paths = HashSet::new();
    let mut refs = Vec::new();
    let mut supersedes_edges: HashMap<MemoryId, Vec<MemoryId>> = HashMap::new();
    let mut superseded_by_edges: HashMap<MemoryId, Vec<MemoryId>> = HashMap::new();
    validate_noncanonical_stream_f_files(root, &mut folded_paths)?;
    for relative in relative_memory_paths(root) {
        let rel = relative.to_string_lossy().replace('\\', "/");
        validate_no_case_fold_collision(&mut folded_paths, &relative)?;

        // B-FT-4: route through RepoPath::try_new to catch unknown top-level dirs
        // and paths outside the Stream A tree (replaces bare RepoPath::new).
        let repo_path = RepoPath::try_new(rel.clone()).map_err(ValidationError::Other)?;

        // B-FT-4: slug/date segment validation on non-ID-based filenames.
        validate_path_segments(&relative)?;

        let text =
            std::fs::read_to_string(root.join(&relative)).map_err(|err| ValidationError::Other(err.to_string()))?;
        let parsed = parse_document(&text, Some(repo_path.clone()))?;
        let id = parsed.memory.frontmatter.id.clone();

        // B-FT-4: plaintext-under-encrypted/ detection.
        // Any file under encrypted/ must have an `encryption:` frontmatter block
        // (per Q6 decision: open-questions-resolved.md §Q6).
        if rel.starts_with("encrypted/") {
            validate_encrypted_tier(&parsed.memory.frontmatter, &relative)?;
        }

        if report.ids.insert(id.clone(), repo_path).is_some() {
            return Err(ValidationError::DuplicateMemoryId(id));
        }
        supersedes_edges.insert(id.clone(), parsed.memory.frontmatter.supersedes.clone());
        superseded_by_edges.insert(id.clone(), parsed.memory.frontmatter.superseded_by.clone());
        refs.extend(parsed.memory.frontmatter.supersedes.iter().cloned());
        refs.extend(parsed.memory.frontmatter.superseded_by.iter().cloned());
        refs.extend(parsed.memory.frontmatter.related.iter().cloned());
        if relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with(MEM_PREFIX) && stem != id.as_str())
        {
            return Err(ValidationError::Other("id filename/frontmatter mismatch".to_string()));
        }
    }
    for id in refs {
        if !report.ids.contains_key(&id) {
            if matches!(mode, TreeValidationMode::PartialSync) {
                report.warnings.push(ValidationWarning::PartialSyncMissingReference { id });
            } else {
                return Err(ValidationError::MissingReference(id));
            }
        }
    }
    validate_supersession_graph(&mut report, mode, &supersedes_edges, &superseded_by_edges)?;
    Ok(report)
}

/// Return true when a path belongs to Stream F's valid-but-noncanonical file families.
pub fn is_noncanonical_stream_f_path(path: &Path) -> bool {
    is_noncanonical_stream_f_repo_path(&path.to_string_lossy().replace('\\', "/"))
}

fn validate_noncanonical_stream_f_files(
    root: &Path,
    folded_paths: &mut HashSet<String>,
) -> Result<(), ValidationError> {
    for entry in
        walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok).filter(|entry| entry.file_type().is_file())
    {
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if !is_noncanonical_stream_f_path(relative) {
            continue;
        }
        validate_no_case_fold_collision(folded_paths, relative)?;
        let rel = relative.to_string_lossy().replace('\\', "/");
        RepoPath::try_new(rel).map_err(ValidationError::Other)?;
        validate_noncanonical_stream_f_file(root, relative)?;
    }
    Ok(())
}

fn validate_noncanonical_stream_f_file(root: &Path, relative: &Path) -> Result<(), ValidationError> {
    match stream_f_path_family(relative) {
        StreamFPathFamily::DreamJournal => validate_dream_scope_date_path(relative, "dreams/journal", "md"),
        StreamFPathFamily::DreamQuestions => {
            validate_dream_scope_date_path(relative, "dreams/questions", "jsonl")?;
            validate_dream_question_jsonl(root, relative)
        }
        StreamFPathFamily::DreamCleanup => {
            validate_device_date_path(relative, "dreams/cleanup", "json")?;
            validate_json_object_file(root, relative)
        }
        StreamFPathFamily::SubstrateArchive => {
            validate_device_month_path(relative, "substrate/archive", "jsonl")?;
            validate_jsonl_objects(root, relative)
        }
        StreamFPathFamily::PlaintextSubstrate => {
            validate_device_date_path(relative, "substrate", "jsonl")?;
            validate_jsonl_objects(root, relative)
        }
        StreamFPathFamily::EncryptedSubstrate => {
            validate_device_date_path(relative, "encrypted/substrate", "jsonl")?;
            validate_jsonl_objects(root, relative)
        }
        StreamFPathFamily::JournalLease => validate_jsonl_objects(root, relative),
        StreamFPathFamily::Unrecognized => {
            panic!("noncanonical Stream F path family must have an explicit validation branch")
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamFPathFamily {
    DreamJournal,
    DreamQuestions,
    DreamCleanup,
    SubstrateArchive,
    PlaintextSubstrate,
    EncryptedSubstrate,
    JournalLease,
    Unrecognized,
}

fn stream_f_path_family(relative: &Path) -> StreamFPathFamily {
    let rel = relative.to_string_lossy().replace('\\', "/");
    if rel == "leases/journal.lease" {
        return StreamFPathFamily::JournalLease;
    }
    if rel.starts_with("dreams/journal/") {
        return StreamFPathFamily::DreamJournal;
    }
    if rel.starts_with("dreams/questions/") {
        return StreamFPathFamily::DreamQuestions;
    }
    if rel.starts_with("dreams/cleanup/") {
        return StreamFPathFamily::DreamCleanup;
    }
    if rel.starts_with("substrate/archive/") {
        return StreamFPathFamily::SubstrateArchive;
    }
    if rel.starts_with("encrypted/substrate/") {
        return StreamFPathFamily::EncryptedSubstrate;
    }
    if rel.starts_with("substrate/") {
        return StreamFPathFamily::PlaintextSubstrate;
    }
    StreamFPathFamily::Unrecognized
}

fn validate_dream_scope_date_path(relative: &Path, prefix: &str, expected_ext: &str) -> Result<(), ValidationError> {
    let rel = relative.to_string_lossy().replace('\\', "/");
    validate_extension(relative, expected_ext)?;
    let rest = rel
        .strip_prefix(prefix)
        .and_then(|value| value.strip_prefix('/'))
        .ok_or_else(|| stream_f_error(relative, "path is outside expected dream prefix"))?;
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["me" | "agent", date_file] => validate_date_file(relative, date_file, expected_ext),
        ["project" | "org", id, date_file] if is_safe_stream_f_segment(id) => {
            validate_date_file(relative, date_file, expected_ext)
        }
        _ => Err(stream_f_error(relative, "invalid dream scope path")),
    }
}

fn validate_device_date_path(relative: &Path, prefix: &str, expected_ext: &str) -> Result<(), ValidationError> {
    let rel = relative.to_string_lossy().replace('\\', "/");
    validate_extension(relative, expected_ext)?;
    let rest = rel
        .strip_prefix(prefix)
        .and_then(|value| value.strip_prefix('/'))
        .ok_or_else(|| stream_f_error(relative, "path is outside expected device-date prefix"))?;
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [device_id, date_file] if is_safe_stream_f_segment(device_id) => {
            validate_date_file(relative, date_file, expected_ext)
        }
        _ => Err(stream_f_error(relative, "invalid device-date path")),
    }
}

fn validate_device_month_path(relative: &Path, prefix: &str, expected_ext: &str) -> Result<(), ValidationError> {
    let rel = relative.to_string_lossy().replace('\\', "/");
    validate_extension(relative, expected_ext)?;
    let rest = rel
        .strip_prefix(prefix)
        .and_then(|value| value.strip_prefix('/'))
        .ok_or_else(|| stream_f_error(relative, "path is outside expected device-month prefix"))?;
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [device_id, month_file] if is_safe_stream_f_segment(device_id) => {
            validate_month_file(relative, month_file, expected_ext)
        }
        _ => Err(stream_f_error(relative, "invalid device-month path")),
    }
}

fn validate_date_file(relative: &Path, file_name: &str, expected_ext: &str) -> Result<(), ValidationError> {
    let expected_suffix = format!(".{expected_ext}");
    let Some(date) = file_name.strip_suffix(&expected_suffix) else {
        return Err(stream_f_error(relative, "unexpected file extension"));
    };
    if ISO_DATE_REGEX.is_match(date) {
        Ok(())
    } else {
        Err(stream_f_error(relative, "expected YYYY-MM-DD file stem"))
    }
}

fn validate_month_file(relative: &Path, file_name: &str, expected_ext: &str) -> Result<(), ValidationError> {
    let expected_suffix = format!(".{expected_ext}");
    let Some(month) = file_name.strip_suffix(&expected_suffix) else {
        return Err(stream_f_error(relative, "unexpected file extension"));
    };
    if YEAR_MONTH_REGEX.is_match(month) {
        Ok(())
    } else {
        Err(stream_f_error(relative, "expected YYYY-MM file stem"))
    }
}

fn validate_extension(relative: &Path, expected_ext: &str) -> Result<(), ValidationError> {
    if relative.extension().and_then(|ext| ext.to_str()) == Some(expected_ext) {
        Ok(())
    } else {
        Err(stream_f_error(relative, "unexpected file extension"))
    }
}

fn is_safe_stream_f_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn validate_dream_question_jsonl(root: &Path, relative: &Path) -> Result<(), ValidationError> {
    for value in parse_jsonl_objects(root, relative)? {
        let entities = value.get("entities").ok_or_else(|| stream_f_error(relative, "missing entities"))?;
        let question = value.get("question").ok_or_else(|| stream_f_error(relative, "missing question"))?;
        if !entities.as_array().is_some_and(|items| items.iter().all(|item| item.as_str().is_some()))
            || question.as_str().is_none()
        {
            return Err(stream_f_error(relative, "question records require {entities: string[], question: string}"));
        }
    }
    Ok(())
}

fn validate_jsonl_objects(root: &Path, relative: &Path) -> Result<(), ValidationError> {
    parse_jsonl_objects(root, relative).map(|_| ())
}

fn validate_json_object_file(root: &Path, relative: &Path) -> Result<(), ValidationError> {
    let text = read_stream_f_file(root, relative)?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| stream_f_error(relative, &format!("invalid JSON: {err}")))?;
    if value.is_object() {
        Ok(())
    } else {
        Err(stream_f_error(relative, "expected JSON object"))
    }
}

fn parse_jsonl_objects(root: &Path, relative: &Path) -> Result<Vec<serde_json::Value>, ValidationError> {
    let text = read_stream_f_file(root, relative)?;
    let mut values = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| stream_f_error(relative, &format!("invalid JSONL line {}: {err}", index + 1)))?;
        if !value.is_object() {
            return Err(stream_f_error(relative, &format!("JSONL line {} must be an object", index + 1)));
        }
        values.push(value);
    }
    Ok(values)
}

fn read_stream_f_file(root: &Path, relative: &Path) -> Result<String, ValidationError> {
    std::fs::read_to_string(root.join(relative)).map_err(|err| stream_f_error(relative, &format!("read failed: {err}")))
}

fn stream_f_error(path: &Path, message: &str) -> ValidationError {
    ValidationError::NonCanonicalStreamFFile { path: path.to_path_buf(), message: message.to_string() }
}

/// Validate that a file under `encrypted/` has an encryption frontmatter block.
///
/// Per Q6 decision: the `encryption:` key must be present in frontmatter.
/// Since `Frontmatter::extras` captures unknown fields, the check is:
/// `extras.contains_key("encryption")`.
fn validate_encrypted_tier(frontmatter: &crate::model::Frontmatter, path: &Path) -> Result<(), ValidationError> {
    if !frontmatter.extras.contains_key("encryption") {
        return Err(ValidationError::PlaintextUnderEncryptedTier { path: path.to_path_buf() });
    }
    Ok(())
}

/// Validate path segments against spec §5.1 slug and date rules.
///
/// For non-ID-based filenames (i.e. filenames that do not start with `mem_`),
/// all path components (including the stem) must match either:
/// - A slug: `[a-z0-9][a-z0-9-]{0,62}`, or
/// - A date-prefixed slug: `<YYYY-MM-DD>-<slug>` (used in `decisions/`), or
/// - An ISO date: `\d{4}-\d{2}-\d{2}` (used as a bare directory component).
///
/// ID-based filenames have their own validation via the frontmatter-id mismatch
/// check; slug rules do not apply to their file stems.
fn validate_path_segments(relative: &Path) -> Result<(), ValidationError> {
    let stem = relative.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    // ID-based filenames skip slug validation; they are validated separately.
    if stem.starts_with(MEM_PREFIX) {
        return Ok(());
    }

    // Validate every path component except the final extension-bearing component.
    // (Directory components must be slugs; the file stem must also be slug-like.)
    for component in relative.components() {
        let seg = match component {
            std::path::Component::Normal(s) => s.to_string_lossy(),
            _ => continue,
        };
        let seg = seg.as_ref();

        // Strip extension for the final component.
        let without_ext = if let Some(dot) = seg.rfind('.') { &seg[..dot] } else { seg };

        // Accept: pure ISO date, date-prefixed slug (`YYYY-MM-DD-slug`), or slug.
        if is_valid_path_segment(without_ext) {
            continue;
        }
        return Err(ValidationError::Other(format!(
            "path segment {without_ext:?} in {:?} does not match slug or date rules",
            relative
        )));
    }
    Ok(())
}

/// Return true if a path segment is a valid slug, ISO date, or date-prefixed slug.
fn is_valid_path_segment(seg: &str) -> bool {
    if SLUG_REGEX.is_match(seg) {
        return true;
    }
    if ISO_DATE_REGEX.is_match(seg) {
        return true;
    }
    // Date-prefixed slug: `YYYY-MM-DD-<slug>`.
    if seg.len() > 11 && ISO_DATE_REGEX.is_match(&seg[..10]) && seg.as_bytes().get(10) == Some(&b'-') {
        let slug_part = &seg[11..];
        if SLUG_REGEX.is_match(slug_part) {
            return true;
        }
    }
    false
}

/// Validate a list of relative paths has no case-folded collision.
pub fn validate_case_fold_paths(paths: &[PathBuf]) -> Result<(), ValidationError> {
    let mut folded_paths = HashSet::new();
    for path in paths {
        validate_no_case_fold_collision(&mut folded_paths, path)?;
    }
    Ok(())
}

fn validate_no_case_fold_collision(folded_paths: &mut HashSet<String>, relative: &Path) -> Result<(), ValidationError> {
    let rel = relative.to_string_lossy().replace('\\', "/");
    let folded = rel.to_lowercase();
    if !folded_paths.insert(folded) {
        return Err(ValidationError::CaseFoldCollision(rel));
    }
    Ok(())
}

fn validate_supersession_graph(
    report: &mut TreeValidationReport,
    mode: TreeValidationMode,
    supersedes_edges: &HashMap<MemoryId, Vec<MemoryId>>,
    superseded_by_edges: &HashMap<MemoryId, Vec<MemoryId>>,
) -> Result<(), ValidationError> {
    for (newer, older_ids) in supersedes_edges {
        for older in older_ids {
            if report.ids.contains_key(older)
                && !superseded_by_edges.get(older).is_some_and(|newer_ids| newer_ids.contains(newer))
            {
                record_inverse_mismatch(
                    report,
                    mode,
                    format!("inverse supersession mismatch: {newer} supersedes {older}"),
                )?;
            }
        }
    }
    for (older, newer_ids) in superseded_by_edges {
        for newer in newer_ids {
            if report.ids.contains_key(newer)
                && !supersedes_edges.get(newer).is_some_and(|older_ids| older_ids.contains(older))
            {
                record_inverse_mismatch(
                    report,
                    mode,
                    format!("inverse supersession mismatch: {older} superseded_by {newer}"),
                )?;
            }
        }
    }

    let mut semantic_edges: HashMap<MemoryId, Vec<MemoryId>> = supersedes_edges.clone();
    for (older, newer_ids) in superseded_by_edges {
        for newer in newer_ids {
            semantic_edges.entry(newer.clone()).or_default().push(older.clone());
        }
    }
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    for id in report.ids.keys() {
        detect_cycle(id, &semantic_edges, &mut visited, &mut stack)?;
    }
    Ok(())
}

fn record_inverse_mismatch(
    report: &mut TreeValidationReport,
    mode: TreeValidationMode,
    message: String,
) -> Result<(), ValidationError> {
    if matches!(mode, TreeValidationMode::PartialSync) {
        report.warnings.push(ValidationWarning::InverseSupersessionMismatch { message });
        Ok(())
    } else {
        Err(ValidationError::Other(message))
    }
}

fn detect_cycle(
    id: &MemoryId,
    edges: &HashMap<MemoryId, Vec<MemoryId>>,
    visited: &mut HashSet<MemoryId>,
    stack: &mut HashSet<MemoryId>,
) -> Result<(), ValidationError> {
    if stack.contains(id) {
        return Err(ValidationError::SupersessionCycle(id.clone()));
    }
    if !visited.insert(id.clone()) {
        return Ok(());
    }
    stack.insert(id.clone());
    if let Some(next_ids) = edges.get(id) {
        for next_id in next_ids {
            detect_cycle(next_id, edges, visited, stack)?;
        }
    }
    stack.remove(id);
    Ok(())
}
