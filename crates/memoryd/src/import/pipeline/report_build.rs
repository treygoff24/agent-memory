//! Report construction: `ImportReport::from_plan` seeds an empty report from a
//! plan's discovery + parse-error data, and the parse-error classification
//! helpers it relies on. Moved verbatim from the former single-file
//! `pipeline.rs`.

use std::collections::{BTreeMap, BTreeSet};

use crate::import::candidate::Harness;
use crate::import::project_map::ProjectYamlAction;
use crate::import::report::{
    BackEdgeEntry, CwdDispositionEntry, CwdProjectYamlEntry, HarnessCounters, ImportReport, ParseErrorEntry,
};
use crate::import::ImportError;

use super::model::{ImportPlan, WikiLinkBackEdge};

impl ImportReport {
    /// Build an empty report seeded from a plan's discovery + parse-error data.
    /// The execute phase fills in counters as writes complete.
    pub fn from_plan(plan: &ImportPlan) -> Self {
        let mut harnesses: BTreeMap<String, HarnessCounters> = BTreeMap::new();
        harnesses.insert(
            Harness::ClaudeCode.as_str().to_string(),
            HarnessCounters { parsed: plan.source_discovery_summary.claude_candidates, ..Default::default() },
        );
        harnesses.insert(
            Harness::Codex.as_str().to_string(),
            HarnessCounters { parsed: plan.source_discovery_summary.codex_candidates, ..Default::default() },
        );

        let mut seen_cwds = BTreeSet::new();
        let mut cwd_dispositions = Vec::new();
        let mut project_yaml_writes = Vec::new();
        for action in &plan.actions {
            let cwd = action.candidate.cwd.clone();
            if !seen_cwds.insert(cwd.clone()) {
                continue;
            }
            if let Some(project_yaml) = &action.scope.project_yaml {
                if matches!(project_yaml.action, ProjectYamlAction::Written) {
                    project_yaml_writes.push(project_yaml.path.clone());
                }
            }
            cwd_dispositions.push(CwdDispositionEntry {
                cwd,
                resolution: action.scope.resolution.as_report_str().to_string(),
                canonical_namespace_id: action.scope.canonical_namespace_id.clone(),
                project_yaml: action.scope.project_yaml.as_ref().map(|project_yaml| CwdProjectYamlEntry {
                    path: project_yaml.path.clone(),
                    action: project_yaml.action.as_report_str().to_string(),
                }),
            });
        }

        let unresolved_back_edges = plan
            .unresolved_back_edges
            .iter()
            .map(WikiLinkBackEdge::clone)
            .map(|edge| BackEdgeEntry { source_key: edge.source_key, alias: edge.alias })
            .collect();

        let parse_errors = plan
            .parse_errors
            .iter()
            .map(|error| ParseErrorEntry {
                source_key: parse_error_source_key(error),
                kind: parse_error_kind(error),
                message: error.to_string(),
            })
            .collect();

        Self {
            schema_version: 1,
            harnesses,
            refusals: Vec::new(),
            dedups: Vec::new(),
            unresolved_back_edges,
            cwd_dispositions,
            project_yaml_writes,
            parse_errors,
            frontmatter_recovered: plan.frontmatter_recovered.clone(),
            candidates: Vec::new(),
            quarantined: Vec::new(),
            ambiguous_historical: Vec::new(),
            claude_roots_used: plan.claude_roots_used.clone(),
        }
    }
}

fn parse_error_source_key(error: &ImportError) -> String {
    match error {
        ImportError::Parse { source_key, .. } => source_key.clone(),
        ImportError::Encoding { source_key, .. } => source_key.clone(),
        ImportError::Io { path, .. } => path.display().to_string(),
        ImportError::AnotherImportInProgress { .. } => "<lock>".to_string(),
        ImportError::CorruptState { path, .. } => path.display().to_string(),
        ImportError::Json(_) => "<json>".to_string(),
        ImportError::PartialExecute { source_key, .. } => source_key.clone(),
    }
}

fn parse_error_kind(error: &ImportError) -> String {
    match error {
        ImportError::Parse { .. } => "parse".to_string(),
        ImportError::Encoding { .. } => "encoding".to_string(),
        ImportError::Io { .. } => "io".to_string(),
        ImportError::AnotherImportInProgress { .. } => "lock".to_string(),
        ImportError::CorruptState { .. } => "corrupt_state".to_string(),
        ImportError::Json(_) => "json".to_string(),
        ImportError::PartialExecute { .. } => "partial_execute".to_string(),
    }
}
