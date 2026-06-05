//! Import-report serialisation. Emitted to stdout (human-readable text) and,
//! when `--report <path.json>` is supplied, to a JSON file the operator can
//! diff between runs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::import::project_map::ProjectYamlAction;

/// Top-level import report. JSON-serializable; the text rendering is built
/// from the same struct so the two views stay in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub schema_version: u32,
    pub harnesses: BTreeMap<String, HarnessCounters>,
    pub refusals: Vec<RefusalEntry>,
    pub dedups: Vec<DedupEntry>,
    pub unresolved_back_edges: Vec<BackEdgeEntry>,
    pub cwd_dispositions: Vec<CwdDispositionEntry>,
    pub project_yaml_writes: Vec<PathBuf>,
    pub parse_errors: Vec<ParseErrorEntry>,
}

/// Per-harness rollup. Counts are tracked separately so the user can see
/// at-a-glance how many candidates came from each side.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessCounters {
    pub parsed: usize,
    pub written_new: usize,
    pub dedup_existing: usize,
    pub superseded: usize,
    pub written_candidate: usize,
    pub quarantined: usize,
    pub skipped_idempotent: usize,
    pub skipped_by_prompt: usize,
    pub refused_privacy: usize,
    pub refused_contradiction: usize,
    pub refused_tombstone: usize,
    pub refused_grounding: usize,
    pub refused_policy: usize,
    pub refused_other: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefusalEntry {
    pub source_key: String,
    pub harness: String,
    pub reason: String,
    pub suggested_next_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupEntry {
    pub source_key: String,
    pub harness: String,
    pub existing_memory_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackEdgeEntry {
    pub source_key: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdDispositionEntry {
    pub cwd: Option<PathBuf>,
    pub resolution: String,
    pub canonical_namespace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_yaml: Option<CwdProjectYamlEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CwdProjectYamlEntry {
    pub path: PathBuf,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseErrorEntry {
    pub source_key: String,
    pub kind: String,
    pub message: String,
}

impl ImportReport {
    /// Render the report as a JSON string. Used for `--report <path.json>`.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn mark_project_yaml_written(&mut self, path: &Path) {
        if !self.project_yaml_writes.iter().any(|written| written == path) {
            self.project_yaml_writes.push(path.to_path_buf());
        }
        for disposition in &mut self.cwd_dispositions {
            if let Some(project_yaml) = &mut disposition.project_yaml {
                if project_yaml.path == path {
                    project_yaml.action = ProjectYamlAction::Written.as_report_str().to_string();
                }
            }
        }
    }

    /// Render the report as a human-readable summary for stdout.
    pub fn to_text(&self) -> String {
        let mut buf = String::new();
        buf.push_str("Import report\n");
        for (harness, counters) in &self.harnesses {
            buf.push_str(&format!(
                "  {harness}: parsed={p} written={w} dedup={d} superseded={s} candidate={c} quarantined={q} skipped_idempotent={si} skipped_by_prompt={sp} refused={r}\n",
                p = counters.parsed,
                w = counters.written_new,
                d = counters.dedup_existing,
                s = counters.superseded,
                c = counters.written_candidate,
                q = counters.quarantined,
                si = counters.skipped_idempotent,
                sp = counters.skipped_by_prompt,
                r = counters.refused_privacy
                    + counters.refused_contradiction
                    + counters.refused_tombstone
                    + counters.refused_grounding
                    + counters.refused_policy
                    + counters.refused_other,
            ));
        }
        if !self.refusals.is_empty() {
            buf.push_str("\nRefusals:\n");
            for refusal in &self.refusals {
                buf.push_str(&format!("  [{}] {}: {}\n", refusal.harness, refusal.source_key, refusal.reason));
            }
        }
        let skipped_by_prompt: usize = self.harnesses.values().map(|counters| counters.skipped_by_prompt).sum();
        if skipped_by_prompt > 0 {
            buf.push_str(&format!(
                "\n{skipped_by_prompt} memories skipped (non-git cwd); re-run with --non-git-cwd-default {{me|generate}} to place them\n"
            ));
            for disposition in self.cwd_dispositions.iter().filter(|entry| entry.resolution == "prompted_skip") {
                if let Some(cwd) = &disposition.cwd {
                    buf.push_str(&format!("  {}\n", cwd.display()));
                }
            }
        }
        if !self.unresolved_back_edges.is_empty() {
            buf.push_str("\nUnresolved wiki-link back-edges (inert in body):\n");
            for edge in &self.unresolved_back_edges {
                buf.push_str(&format!("  {} → [[{}]]\n", edge.source_key, edge.alias));
            }
        }
        if !self.project_yaml_writes.is_empty() {
            buf.push_str("\n.memory-project.yaml files written:\n");
            for path in &self.project_yaml_writes {
                buf.push_str(&format!("  {}\n", path.display()));
            }
        }
        if !self.parse_errors.is_empty() {
            buf.push_str("\nParse errors (skipped):\n");
            for error in &self.parse_errors {
                buf.push_str(&format!("  [{}] {}: {}\n", error.kind, error.source_key, error.message));
            }
        }
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_json_round_trips() {
        let report = ImportReport {
            schema_version: 1,
            harnesses: BTreeMap::new(),
            refusals: Vec::new(),
            dedups: Vec::new(),
            unresolved_back_edges: Vec::new(),
            cwd_dispositions: Vec::new(),
            project_yaml_writes: Vec::new(),
            parse_errors: Vec::new(),
        };
        let json = report.to_json().expect("json");
        let parsed: ImportReport = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(parsed.schema_version, report.schema_version);
    }
}
