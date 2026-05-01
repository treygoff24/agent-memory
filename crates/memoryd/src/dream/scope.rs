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
