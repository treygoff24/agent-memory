use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

pub const CLEANUP_BOT_AUTHOR: &str = "memoryd cleanup-bot <noreply@memoryd.local>";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupReport {
    pub schema_version: u32,
    pub device_id: String,
    pub date: NaiveDate,
    pub generated_at: DateTime<Utc>,
    pub commit_deferred: bool,
    pub operations: CleanupOperationCounts,
    pub findings: Vec<CleanupFinding>,
    /// Substrate fragments whose archival was deferred this run because they are
    /// still cited by live memories and have not yet reached the immortality cap
    /// (Memory Dynamics spec v0.1 §4). Additive surface; empty when dynamics is
    /// disabled or nothing is deferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred_fragments: Vec<DeferredFragment>,
    pub mutated_files: Vec<String>,
    pub git: CleanupGitReport,
}

/// One deferred substrate fragment in a cleanup report (spec §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferredFragment {
    /// Deferred fragment id (`sub_…`).
    pub fragment_id: String,
    /// Number of distinct live memories whose `Evidence.reference` entries cite
    /// the fragment and triggered the deferral.
    pub citations: u64,
    /// Hard immortality-cap timestamp: when this fragment archives regardless of
    /// further citations. The fragment is re-evaluated on every cleanup pass
    /// before this cap and may archive sooner if distinct live citations drop
    /// below the configured threshold.
    pub cap_deadline: DateTime<Utc>,
}

impl CleanupReport {
    pub fn from_input(input: CleanupReportInput) -> Self {
        let date = input.generated_at.date_naive();
        Self {
            schema_version: 1,
            device_id: input.device_id.clone(),
            date,
            generated_at: input.generated_at,
            commit_deferred: false,
            operations: input.operations,
            findings: input.findings,
            deferred_fragments: input.deferred_fragments,
            mutated_files: input.mutated_files.clone(),
            git: CleanupGitReport {
                author: CLEANUP_BOT_AUTHOR.to_string(),
                message: cleanup_commit_subject(&input.device_id, date),
                summary: input.operations.summary_line(),
                staged_files: input.mutated_files,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReportInput {
    pub device_id: String,
    pub generated_at: DateTime<Utc>,
    pub operations: CleanupOperationCounts,
    pub findings: Vec<CleanupFinding>,
    pub deferred_fragments: Vec<DeferredFragment>,
    pub mutated_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupOperationCounts {
    pub fragments_archived: usize,
    pub candidates_archived: usize,
    pub entity_index_rebuilt: bool,
    pub entity_index_rows: usize,
    pub lint_findings: usize,
    pub tombstone_findings: usize,
    pub supersession_findings: usize,
    pub observed_at_refreshed: usize,
    pub events_compacted: usize,
    pub event_archive_files_written: usize,
}

impl CleanupOperationCounts {
    pub fn summary_line(self) -> String {
        format!(
            "fragments_archived={} candidates_archived={} lint_findings={} tombstone_findings={} supersession_findings={} observed_at_refreshed={} events_compacted={}",
            self.fragments_archived,
            self.candidates_archived,
            self.lint_findings,
            self.tombstone_findings,
            self.supersession_findings,
            self.observed_at_refreshed,
            self.events_compacted
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupFinding {
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub message: String,
}

impl CleanupFinding {
    pub fn new(
        kind: impl Into<String>,
        path: impl Into<String>,
        id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self { kind: kind.into(), path: path.into(), id, message: message.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupGitReport {
    pub author: String,
    pub message: String,
    pub summary: String,
    pub staged_files: Vec<String>,
}

pub fn cleanup_commit_subject(device_id: &str, date: NaiveDate) -> String {
    format!("dream: cleanup {device_id} {date}")
}
