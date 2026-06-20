//! Repository-relative path validation (spec §5.1).
//!
//! The allow-list state machine that decides whether a repo-relative path is a
//! valid Stream A / Stream F file. Extracted from the `model` DTO module so the
//! canonical-contract file stays DTO-only; [`crate::model::RepoPath::try_new`]
//! and the `tree` validators are the consumers.

use std::path::{Component, Path};

/// Top-level prefixes where Markdown memory files (`.md`) are valid (spec §5.1).
const MEMORY_PREFIXES: &[&str] = &["me/", "projects/", "agent/", "dreams/", "encrypted/"];

/// Top-level prefixes that are JSONL-only (spec §5.1). Markdown is rejected here.
const JSONL_PREFIXES: &[&str] = &["substrate/", "events/", "tombstones/", "policies/", "leases/"];

/// Repository-root files we allow alongside the namespaced trees.
const ROOT_FILES: &[&str] = &[".gitattributes", ".gitignore", "config.yaml"];

pub(crate) fn validate_repo_relative_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') {
        return Err("empty or nul path".to_string());
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("forbidden path component in {value}"));
            }
        }
    }
    if ROOT_FILES.contains(&value) {
        return Ok(());
    }
    if is_noncanonical_stream_f_repo_path(value) {
        return Ok(());
    }
    if let Some(prefix) = MEMORY_PREFIXES.iter().find(|prefix| value.starts_with(*prefix)) {
        return validate_memory_tier_extension(value, prefix);
    }
    if let Some(prefix) = JSONL_PREFIXES.iter().find(|prefix| value.starts_with(*prefix)) {
        return validate_jsonl_tier_extension(value, prefix);
    }
    Err(format!("path is outside Stream A tree: {value}"))
}

pub(crate) fn is_noncanonical_stream_f_repo_path(value: &str) -> bool {
    if value == "leases/journal.lease" {
        return true;
    }
    let extension = Path::new(value).extension().and_then(|ext| ext.to_str());
    match extension {
        Some("md") => value.starts_with("dreams/journal/"),
        Some("jsonl") => {
            value.starts_with("dreams/questions/")
                || value.starts_with("substrate/")
                || value.starts_with("encrypted/substrate/")
        }
        Some("json") => value.starts_with("dreams/cleanup/"),
        _ => false,
    }
}

fn validate_memory_tier_extension(value: &str, prefix: &str) -> Result<(), String> {
    let extension = Path::new(value).extension().and_then(|ext| ext.to_str());
    match extension {
        Some("md") => Ok(()),
        Some(other) => Err(format!("memory tier {prefix} accepts only .md, got .{other}: {value}")),
        None => Err(format!("memory tier {prefix} accepts only .md: {value}")),
    }
}

fn validate_jsonl_tier_extension(value: &str, prefix: &str) -> Result<(), String> {
    let extension = Path::new(value).extension().and_then(|ext| ext.to_str());
    match extension {
        Some("jsonl") => Ok(()),
        Some("yaml") if prefix == "policies/" => Ok(()),
        Some("md") => Err(format!("JSONL-only tier {prefix} rejects markdown: {value}")),
        Some(other) => Err(format!("JSONL-only tier {prefix} rejects .{other}: {value}")),
        None => Err(format!("JSONL-only tier {prefix} requires an extension: {value}")),
    }
}
