use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use memory_substrate::frontmatter::parse_document;
use memory_substrate::tree::relative_memory_paths;
use memory_substrate::{AuthorKind, MemoryStatus};
use serde_json::Value;

const PREVIEW_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamReviewReport {
    pub since: String,
    pub scope: Option<String>,
    pub entries: Vec<DreamReviewEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamReviewEntry {
    pub kind: String,
    pub path: String,
    pub summary: String,
}

pub fn parse_since_duration(value: &str) -> Result<Duration, String> {
    let value = value.trim();
    if value.len() < 2 {
        return Err("duration must look like 7d, 24h, or 60m".to_string());
    }
    let (number, unit) = value.split_at(value.len() - 1);
    let amount = number.parse::<u64>().map_err(|_| "duration amount must be a positive integer".to_string())?;
    if amount == 0 {
        return Err("duration amount must be positive".to_string());
    }
    match unit {
        "d" => Ok(Duration::from_secs(amount * 24 * 60 * 60)),
        "h" => Ok(Duration::from_secs(amount * 60 * 60)),
        "m" => Ok(Duration::from_secs(amount * 60)),
        _ => Err("duration unit must be d, h, or m".to_string()),
    }
}

pub fn collect_review(repo: &Path, since: &str, scope: Option<&str>) -> Result<DreamReviewReport, String> {
    let duration = parse_since_duration(since)?;
    let cutoff = SystemTime::now().checked_sub(duration).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut entries = Vec::new();

    collect_journals(repo, cutoff, scope, &mut entries)?;
    collect_questions(repo, cutoff, scope, &mut entries)?;
    collect_candidates(repo, cutoff, scope, &mut entries)?;
    collect_cleanup_reports(repo, cutoff, &mut entries)?;

    entries.sort_by(|left, right| left.kind.cmp(&right.kind).then(left.path.cmp(&right.path)));
    Ok(DreamReviewReport { since: since.to_string(), scope: scope.map(str::to_string), entries })
}

pub fn render_human_review(report: &DreamReviewReport) -> String {
    let mut out = format!("dream review since={} scope={}\n", report.since, report.scope.as_deref().unwrap_or("*"));
    if report.entries.is_empty() {
        out.push_str("no dream outputs found\n");
        return out;
    }
    for entry in &report.entries {
        out.push_str(&format!("- {} {} :: {}\n", entry.kind, entry.path, entry.summary));
    }
    out
}

fn collect_journals(
    repo: &Path,
    cutoff: SystemTime,
    scope: Option<&str>,
    entries: &mut Vec<DreamReviewEntry>,
) -> Result<(), String> {
    let root = repo.join("dreams/journal");
    for path in recent_files(&root, cutoff, Some("md"))? {
        if !matches_scope(&root, &path, scope) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        entries.push(DreamReviewEntry {
            kind: "journal".to_string(),
            path: repo_relative(repo, &path),
            summary: first_safe_lines(&text, 2),
        });
    }
    Ok(())
}

fn collect_questions(
    repo: &Path,
    cutoff: SystemTime,
    scope: Option<&str>,
    entries: &mut Vec<DreamReviewEntry>,
) -> Result<(), String> {
    let root = repo.join("dreams/questions");
    for path in recent_files(&root, cutoff, Some("jsonl"))? {
        if !matches_scope(&root, &path, scope) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let questions = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|json| json.get("question").and_then(Value::as_str).map(str::to_string))
            .take(3)
            .collect::<Vec<_>>();
        entries.push(DreamReviewEntry {
            kind: "question".to_string(),
            path: repo_relative(repo, &path),
            summary: bounded(&questions.join(" | ")),
        });
    }
    Ok(())
}

fn collect_candidates(
    repo: &Path,
    cutoff: SystemTime,
    scope: Option<&str>,
    entries: &mut Vec<DreamReviewEntry>,
) -> Result<(), String> {
    for relative in relative_memory_paths(repo) {
        let path = repo.join(&relative);
        if !is_recent(&path, cutoff)? {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let Ok(parsed) = parse_document(&text, None) else {
            if let Some(summary) = raw_dream_candidate_summary(&text) {
                entries.push(DreamReviewEntry {
                    kind: "candidate".to_string(),
                    path: relative.to_string_lossy().into_owned(),
                    summary: bounded(&summary),
                });
            }
            continue;
        };
        let memory = parsed.memory;
        if memory.frontmatter.status != MemoryStatus::Candidate {
            continue;
        }
        if memory.frontmatter.author.kind != AuthorKind::Dreaming
            && memory.frontmatter.write_policy.policy_applied != "dreaming-strict"
        {
            continue;
        }
        if let Some(scope) = scope {
            let namespace_matches = memory.frontmatter.namespace.as_deref() == Some(scope);
            let scope_matches = format!("{:?}", memory.frontmatter.scope).eq_ignore_ascii_case(scope);
            if !namespace_matches && !scope_matches {
                continue;
            }
        }
        entries.push(DreamReviewEntry {
            kind: "candidate".to_string(),
            path: relative.to_string_lossy().into_owned(),
            summary: bounded(&memory.frontmatter.summary),
        });
    }
    Ok(())
}

fn raw_dream_candidate_summary(text: &str) -> Option<String> {
    let frontmatter = text.strip_prefix("---")?.split_once("---")?.0;
    let is_candidate = frontmatter.lines().any(|line| line.trim() == "status: candidate");
    let is_dream = frontmatter.lines().any(|line| line.trim() == "kind: dreaming")
        || frontmatter.lines().any(|line| line.trim() == "policy_applied: dreaming-strict");
    if !is_candidate || !is_dream {
        return None;
    }
    frontmatter
        .lines()
        .find_map(|line| line.trim().strip_prefix("summary:").map(str::trim))
        .map(|summary| summary.trim_matches('"').to_string())
}

fn collect_cleanup_reports(repo: &Path, cutoff: SystemTime, entries: &mut Vec<DreamReviewEntry>) -> Result<(), String> {
    let root = repo.join("dreams/cleanup");
    for path in recent_files(&root, cutoff, Some("json"))? {
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let summary = serde_json::from_str::<Value>(&text)
            .ok()
            .map(|json| cleanup_summary(&json))
            .unwrap_or_else(|| "unparseable cleanup report".to_string());
        entries.push(DreamReviewEntry {
            kind: "cleanup".to_string(),
            path: repo_relative(repo, &path),
            summary: bounded(&summary),
        });
    }
    Ok(())
}

fn cleanup_summary(json: &Value) -> String {
    let fragments = json.pointer("/operations/fragments_archived").and_then(Value::as_u64).unwrap_or(0);
    let findings = json.get("findings").and_then(Value::as_array).map_or(0, Vec::len);
    format!("fragments_archived={fragments} findings={findings}")
}

fn recent_files(root: &Path, cutoff: SystemTime, extension: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.retain(|path| {
        path.is_file()
            && extension.is_none_or(|ext| path.extension().and_then(|value| value.to_str()) == Some(ext))
            && is_recent(path, cutoff).unwrap_or(false)
    });
    files.sort();
    Ok(files)
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn is_recent(path: &Path, cutoff: SystemTime) -> Result<bool, String> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(|modified| modified >= cutoff)
        .map_err(|err| err.to_string())
}

fn matches_scope(root: &Path, path: &Path, requested: Option<&str>) -> bool {
    let Some(requested) = requested else {
        return true;
    };
    scope_from_dream_path(root, path).as_deref() == Some(requested)
}

fn scope_from_dream_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let pieces = relative.iter().map(|piece| piece.to_str()).collect::<Option<Vec<_>>>()?;
    match pieces.as_slice() {
        ["me", _file] => Some("me".to_string()),
        ["agent", _file] => Some("agent".to_string()),
        ["project", id, _file] => Some(format!("project:{id}")),
        ["org", id, _file] => Some(format!("org:{id}")),
        _ => None,
    }
}

fn first_safe_lines(text: &str, count: usize) -> String {
    bounded(&text.lines().take(count).collect::<Vec<_>>().join(" "))
}

fn bounded(text: &str) -> String {
    let mut chars = text.chars();
    let bounded = chars.by_ref().take(PREVIEW_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}...")
    } else {
        bounded
    }
}

fn repo_relative(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo).unwrap_or(path).to_string_lossy().into_owned()
}
