use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::types::DreamError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DreamScope {
    Me,
    Agent,
    Project(String),
    Org(String),
}

impl DreamScope {
    pub fn parse(raw_scope: &str) -> Result<Self, DreamError> {
        match raw_scope {
            "me" => Ok(Self::Me),
            "agent" => Ok(Self::Agent),
            _ => parse_prefixed_scope(raw_scope),
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Me => "me".to_owned(),
            Self::Agent => "agent".to_owned(),
            Self::Project(project_id) => format!("project:{project_id}"),
            Self::Org(org_id) => format!("org:{org_id}"),
        }
    }

    pub fn storage_path_for_date(&self, date: NaiveDate) -> String {
        let date = date.format("%Y-%m-%d");
        match self {
            Self::Me => format!("me/{date}"),
            Self::Agent => format!("agent/{date}"),
            Self::Project(project_id) => format!("project/{project_id}/{date}"),
            Self::Org(org_id) => format!("org/{org_id}/{date}"),
        }
    }

    pub fn journal_path(&self, date: NaiveDate) -> String {
        format!("dreams/journal/{}.md", self.storage_path_for_date(date))
    }

    pub fn questions_path(&self, date: NaiveDate) -> String {
        format!("dreams/questions/{}.jsonl", self.storage_path_for_date(date))
    }
}

fn parse_prefixed_scope(raw_scope: &str) -> Result<DreamScope, DreamError> {
    if let Some(project_id) = raw_scope.strip_prefix("project:") {
        validate_scope_id(raw_scope, project_id)?;
        return Ok(DreamScope::Project(project_id.to_owned()));
    }

    if let Some(org_id) = raw_scope.strip_prefix("org:") {
        validate_scope_id(raw_scope, org_id)?;
        return Ok(DreamScope::Org(org_id.to_owned()));
    }

    Err(invalid_scope(raw_scope))
}

fn validate_scope_id(raw_scope: &str, id: &str) -> Result<(), DreamError> {
    let is_valid = !id.is_empty()
        && id.len() <= 128
        && id != "."
        && id != ".."
        && !id.chars().all(|character| character == '.')
        && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));

    if is_valid {
        Ok(())
    } else {
        Err(invalid_scope(raw_scope))
    }
}

fn invalid_scope(raw_scope: &str) -> DreamError {
    DreamError::invalid_request(format!(
        "invalid dream scope `{raw_scope}`; expected me, agent, project:<id>, or org:<id>"
    ))
}

/// Recursively collect every file under `path` into `files`. Missing paths are
/// treated as empty (no error). Shared by the dream review and status surfaces,
/// which both walk a dream journal/questions tree.
pub(super) fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
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

/// Map a dream artifact path (relative to `root`) back to its scope string
/// (`me`, `agent`, `project:<id>`, `org:<id>`), or `None` if the layout does
/// not match. Shared by the dream review and status surfaces.
pub(super) fn scope_from_dream_path(root: &Path, path: &Path) -> Option<String> {
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
