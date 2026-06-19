//! Import-report serialisation. Emitted to stdout (human-readable text) and,
//! when `--report <path.json>` is supplied, to a JSON file the operator can
//! diff between runs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::import::project_map::ProjectYamlAction;

/// Top-level import report. JSON-serializable; the text rendering is built
/// from the same struct so the two views stay in sync.
///
/// `Default` is derived so construction sites can spread `..Default::default()`
/// instead of naming every field — this keeps adding report fields from
/// rippling into exhaustive struct literals across the crate.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportReport {
    pub schema_version: u32,
    pub harnesses: BTreeMap<String, HarnessCounters>,
    pub refusals: Vec<RefusalEntry>,
    pub dedups: Vec<DedupEntry>,
    pub unresolved_back_edges: Vec<BackEdgeEntry>,
    pub cwd_dispositions: Vec<CwdDispositionEntry>,
    pub project_yaml_writes: Vec<PathBuf>,
    pub parse_errors: Vec<ParseErrorEntry>,
    /// Source keys whose malformed YAML frontmatter was lenient-recovered and
    /// imported (not dropped). Surfaced so the operator knows which memories
    /// landed with best-effort frontmatter and may warrant a glance.
    #[serde(default)]
    pub frontmatter_recovered: Vec<String>,
    /// Sources whose write landed in the review queue as a governance
    /// candidate. Maps each back to its harness and (when known) the assigned
    /// memory id so the operator can inspect/activate them.
    #[serde(default)]
    pub candidates: Vec<CandidateEntry>,
    /// Sources whose write was quarantined by governance. Same mapping as
    /// `candidates`; quarantined memories also surface in the review queue.
    #[serde(default)]
    pub quarantined: Vec<CandidateEntry>,
    /// The Claude profile roots the import actually covered, as string paths.
    /// Empty when only Codex was imported (or no roots were discovered).
    #[serde(default)]
    pub claude_roots_used: Vec<String>,
}

/// A source that became a review-queue candidate or was quarantined. Carries
/// enough to map the disposition back to the originating source and the
/// daemon-assigned memory id (absent in dry-run, where no write is issued).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateEntry {
    pub source_key: String,
    pub harness: String,
    pub memory_id: Option<String>,
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

    /// Append the cross-harness `Reconciliation` block so "imported" is never
    /// ambiguous: it separates memories that are active and recall-visible now
    /// from those parked in the review queue, blocked by privacy, recovered
    /// with best-effort frontmatter, dropped as unreadable, or skipped as
    /// already-present. Each non-active line renders only when its count is
    /// positive so a clean import stays terse.
    fn push_reconciliation(&self, buf: &mut String) {
        let mut written_new = 0usize;
        let mut superseded = 0usize;
        let mut written_candidate = 0usize;
        let mut quarantined = 0usize;
        let mut refused = 0usize;
        let mut dedup_existing = 0usize;
        let mut skipped_idempotent = 0usize;
        for counters in self.harnesses.values() {
            written_new += counters.written_new;
            superseded += counters.superseded;
            written_candidate += counters.written_candidate;
            quarantined += counters.quarantined;
            refused += counters.refused_privacy
                + counters.refused_contradiction
                + counters.refused_tombstone
                + counters.refused_grounding
                + counters.refused_policy
                + counters.refused_other;
            dedup_existing += counters.dedup_existing;
            skipped_idempotent += counters.skipped_idempotent;
        }

        buf.push_str("\nReconciliation\n");
        buf.push_str(&format!("  imported (active & recall-visible): {}\n", written_new + superseded));
        let queued = written_candidate + quarantined;
        buf.push_str(&format!("  queued for review: {queued}\n"));
        if queued > 0 {
            buf.push_str("    → activate/inspect: memoryd review queue --socket <sock>\n");
        }
        if refused > 0 {
            buf.push_str(&format!("  privacy-blocked: {refused}\n"));
        }
        if !self.frontmatter_recovered.is_empty() {
            buf.push_str(&format!(
                "  frontmatter-recovered (imported with best-effort frontmatter): {}\n",
                self.frontmatter_recovered.len()
            ));
        }
        if !self.parse_errors.is_empty() {
            buf.push_str(&format!("  dropped (unreadable): {}\n", self.parse_errors.len()));
        }
        if dedup_existing > 0 {
            buf.push_str(&format!("  deduped (already present): {dedup_existing}\n"));
        }
        if skipped_idempotent > 0 {
            buf.push_str(&format!("  unchanged since last import: {skipped_idempotent}\n"));
        }
        if !self.claude_roots_used.is_empty() {
            buf.push_str(&format!("  Claude profile roots covered: {}\n", self.claude_roots_used.len()));
            for root in &self.claude_roots_used {
                buf.push_str(&format!("    {root}\n"));
            }
        }
    }

    /// Render the report as a human-readable summary for stdout.
    pub fn to_text(&self) -> String {
        let mut buf = String::new();
        buf.push_str("Import report\n");
        self.push_reconciliation(&mut buf);
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
            frontmatter_recovered: vec!["claude:proj/memory/bad.md".to_string()],
            candidates: vec![CandidateEntry {
                source_key: "claude:proj/memory/cand.md".to_string(),
                harness: "claude-code".to_string(),
                memory_id: Some("mem_cand".to_string()),
            }],
            quarantined: vec![CandidateEntry {
                source_key: "codex:quarantined.md".to_string(),
                harness: "codex".to_string(),
                memory_id: None,
            }],
            claude_roots_used: vec!["/home/u/.claude/projects".to_string()],
            ..Default::default()
        };
        let json = report.to_json().expect("json");
        let parsed: ImportReport = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(parsed.schema_version, report.schema_version);
        assert_eq!(parsed.frontmatter_recovered, report.frontmatter_recovered);
        assert_eq!(parsed.candidates, report.candidates);
        assert_eq!(parsed.quarantined, report.quarantined);
        assert_eq!(parsed.claude_roots_used, report.claude_roots_used);
    }

    /// Older report JSON (pre-reconciliation) lacks the new fields entirely;
    /// `#[serde(default)]` must let it deserialize to empty vecs, not error.
    #[test]
    fn legacy_json_without_new_fields_still_round_trips() {
        let legacy = r#"{
            "schema_version": 1,
            "harnesses": {},
            "refusals": [],
            "dedups": [],
            "unresolved_back_edges": [],
            "cwd_dispositions": [],
            "project_yaml_writes": [],
            "parse_errors": []
        }"#;
        let parsed: ImportReport = serde_json::from_str(legacy).expect("legacy round-trip");
        assert!(parsed.frontmatter_recovered.is_empty());
        assert!(parsed.candidates.is_empty());
        assert!(parsed.quarantined.is_empty());
        assert!(parsed.claude_roots_used.is_empty());
    }

    #[test]
    fn reconciliation_block_renders_active_and_queued_lines() {
        let mut report = ImportReport { schema_version: 1, ..Default::default() };
        report
            .harnesses
            .insert("claude-code".to_string(), HarnessCounters { written_new: 3, superseded: 1, ..Default::default() });
        let text = report.to_text();
        assert!(text.contains("Reconciliation"), "text: {text}");
        // written_new (3) + superseded (1) = 4 active.
        assert!(text.contains("imported (active & recall-visible): 4"), "text: {text}");
        assert!(text.contains("queued for review: 0"), "text: {text}");
        // No queued items → no review-queue hint.
        assert!(!text.contains("memoryd review queue"), "hint must not appear when queued==0: {text}");
    }

    #[test]
    fn reconciliation_review_queue_hint_appears_only_when_queued_positive() {
        let mut report = ImportReport { schema_version: 1, ..Default::default() };
        report.harnesses.insert(
            "claude-code".to_string(),
            HarnessCounters { written_candidate: 2, quarantined: 1, ..Default::default() },
        );
        let text = report.to_text();
        assert!(text.contains("queued for review: 3"), "text: {text}");
        assert!(
            text.contains("→ activate/inspect: memoryd review queue --socket <sock>"),
            "hint must appear when queued>0: {text}"
        );
    }

    #[test]
    fn reconciliation_optional_lines_render_only_when_positive() {
        let mut report = ImportReport {
            schema_version: 1,
            frontmatter_recovered: vec!["claude:proj/memory/bad.md".to_string()],
            claude_roots_used: vec!["/home/u/.claude/projects".to_string()],
            parse_errors: vec![ParseErrorEntry {
                source_key: "claude:proj/memory/broken.md".to_string(),
                kind: "encoding".to_string(),
                message: "non-utf8".to_string(),
            }],
            ..Default::default()
        };
        report.harnesses.insert(
            "claude-code".to_string(),
            HarnessCounters { dedup_existing: 2, skipped_idempotent: 5, refused_privacy: 1, ..Default::default() },
        );
        let text = report.to_text();
        assert!(text.contains("privacy-blocked: 1"), "text: {text}");
        assert!(text.contains("frontmatter-recovered (imported with best-effort frontmatter): 1"), "text: {text}");
        assert!(text.contains("dropped (unreadable): 1"), "text: {text}");
        assert!(text.contains("deduped (already present): 2"), "text: {text}");
        assert!(text.contains("unchanged since last import: 5"), "text: {text}");
        assert!(text.contains("Claude profile roots covered: 1"), "text: {text}");
        assert!(text.contains("/home/u/.claude/projects"), "text: {text}");
    }
}
