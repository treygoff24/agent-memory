//! Execute phase: walk the topo-ordered plan, issue daemon requests, and build
//! the import report. Holds `ImportEngine::execute`, the shared supersede state
//! machine, and the bookkeeping helpers (meta construction, state recording,
//! counter mutation). Moved verbatim from the former single-file `pipeline.rs`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::Utc;
use serde_json::Value;

use crate::import::candidate::ParsedMemory;
use crate::import::project_map::{write_generated_project_yaml, ProjectYamlAction, ScopeBinding};
use crate::import::report::{
    AmbiguousImportEntry, CandidateEntry, DedupEntry, HarnessCounters, ImportReport, RefusalEntry,
};
use crate::import::sources::candidate_aliases;
use crate::import::state::{ImportRecord, ImportState, SupersededRecord};
use crate::import::{ImportError, ImportResult};
use crate::protocol::GovernanceStatus;
use memory_substrate::MemoryStatus;

use super::daemon_client::{DaemonClient, SupersedeRequest, WriteMemoryRequest};
use super::model::{ImportPlan, PlanAction, PlannedWrite};
use super::ImportEngine;

/// Caller-supplied options for the execute phase.
#[derive(Debug, Clone, Default)]
pub struct ExecuteOptions {
    /// When true, the importer logs intended requests but issues no socket
    /// calls and does not mutate the state file.
    pub dry_run: bool,
    /// When true, progress lines for refused writes go to stderr inline (e.g.
    /// `[47/500] REFUSED (privacy): ...`).
    pub verbose_progress: bool,
}

/// Successful execute result, returned alongside the import report.
#[derive(Debug)]
pub struct ExecuteResult {
    pub report: ImportReport,
    pub state: ImportState,
}

impl ImportEngine {
    /// Execute phase: walk the topo-ordered plan, issue daemon requests, build
    /// the import report. Per the plan, errors on individual writes do not
    /// abort the whole import — refusals append to the report and the loop
    /// continues. Failures of the underlying socket transport (which the
    /// daemon client surfaces as `ImportError::Io`) do propagate and abort.
    pub async fn execute<C: DaemonClient>(
        &self,
        plan: ImportPlan,
        opts: ExecuteOptions,
        client: &mut C,
    ) -> ImportResult<ExecuteResult> {
        let mut report = ImportReport::from_plan(&plan);
        let mut state = plan.state.clone();
        let total = plan.actions.len();
        let mut alias_to_id: HashMap<String, String> = HashMap::new();
        // Seed alias_to_id from the existing state file so re-runs can resolve
        // wiki-links against previously-imported memories. Use the source's
        // harness-relative key and persisted aliases only; the stable `tuple:`
        // identity key is not a wiki-link alias.
        for record in state.imports.values() {
            if !record.source_key.is_empty() {
                alias_to_id.insert(record.source_key.to_ascii_lowercase(), record.memory_id.clone());
            }
            for alias in &record.aliases {
                alias_to_id.entry(alias.to_ascii_lowercase()).or_insert_with(|| record.memory_id.clone());
            }
        }

        for (index, action) in plan.actions.iter().enumerate() {
            let harness = action.candidate.harness;
            let harness_key = harness.as_str().to_string();
            report.harnesses.entry(harness_key.clone()).or_default();
            let progress_prefix = format!("[{}/{}]", index + 1, total);

            match &action.action {
                PlanAction::SkipUnchanged { existing_memory_id, existing_record_key } => {
                    counters_mut(&mut report, &harness_key).skipped_idempotent += 1;
                    if !opts.dry_run {
                        let new_identity = action.candidate.import_identity(action.scope.canonical_namespace_id.as_deref());
                        let new_source_key = action.source_key.clone();
                        let new_source_memory_id = action.candidate.recovered_memory_id().map(str::to_string);
                        if let Some(mut record) = state.imports.remove(existing_record_key) {
                            record.source_identity = new_identity;
                            record.source_key = new_source_key.clone();
                            record.source_memory_id = new_source_memory_id;
                            record.aliases = candidate_aliases(&action.candidate);
                            record.source_path_at_import = action.candidate.source_path.clone();
                            remove_import_record_duplicates(&mut state.imports, &record, None);
                            state.imports.insert(record.source_identity.clone(), record);
                            state.save_atomic(&self.state_path)?;
                        }
                    }
                    alias_to_id.insert(action.source_key.to_ascii_lowercase(), existing_memory_id.clone());
                    register_aliases_for(&action.candidate, existing_memory_id, &mut alias_to_id);
                    continue;
                }
                PlanAction::SkipByPrompt => {
                    counters_mut(&mut report, &harness_key).skipped_by_prompt += 1;
                    continue;
                }
                PlanAction::ReportAmbiguous { matching_memory_ids } => {
                    counters_mut(&mut report, &harness_key).ambiguous += 1;
                    report.ambiguous_historical.push(AmbiguousImportEntry {
                        source_key: action.source_key.clone(),
                        harness: harness_key.clone(),
                        matching_memory_ids: matching_memory_ids.clone(),
                    });
                    continue;
                }
                PlanAction::RepairBucket { prior_memory_id, .. } => {
                    if opts.dry_run {
                        counters_mut(&mut report, &harness_key).superseded += 1;
                        continue;
                    }
                    let related = resolve_related_ids(action, &alias_to_id);
                    self.apply_supersede(
                        client,
                        action,
                        &related,
                        prior_memory_id,
                        "import bucket repair",
                        "inspect bucket repair refusal",
                        "bucket-repair",
                        &progress_prefix,
                        &harness_key,
                        &opts,
                        &mut state,
                        &mut report,
                        &mut alias_to_id,
                    )
                    .await?;
                    continue;
                }
                PlanAction::Supersede { prior_memory_id, .. } => {
                    if opts.dry_run {
                        counters_mut(&mut report, &harness_key).superseded += 1;
                        continue;
                    }
                    let related = resolve_related_ids(action, &alias_to_id);
                    self.apply_supersede(
                        client,
                        action,
                        &related,
                        prior_memory_id,
                        "import supersede",
                        "inspect supersede refusal",
                        "supersede",
                        &progress_prefix,
                        &harness_key,
                        &opts,
                        &mut state,
                        &mut report,
                        &mut alias_to_id,
                    )
                    .await?;
                    continue;
                }
                PlanAction::WriteNew => {}
            }

            let related = resolve_related_ids(action, &alias_to_id);
            let meta = build_write_meta(action, &related, /*supersedes*/ None);

            if opts.dry_run {
                // Pretend it worked: count as written-new without touching state.
                counters_mut(&mut report, &harness_key).written_new += 1;
                continue;
            }

            let outcome = client
                .write_memory(WriteMemoryRequest {
                    body: action.candidate.body.clone(),
                    title: action.candidate.title.clone(),
                    tags: Vec::new(),
                    meta,
                })
                .await
                .map_err(|error| partial_import_error(error, &report, &action.source_key))?;

            match outcome.status {
                GovernanceStatus::Promoted => match (outcome.id, outcome.existing_id) {
                    (Some(written_id), None) => {
                        counters_mut(&mut report, &harness_key).written_new += 1;
                        record_promoted(&mut state, action, &written_id, None, &mut alias_to_id, &self.state_path)
                            .map_err(|error| partial_import_error(error, &report, &action.source_key))?;
                    }
                    (_, Some(existing_id)) => {
                        counters_mut(&mut report, &harness_key).dedup_existing += 1;
                        report.dedups.push(DedupEntry {
                            source_key: action.source_key.clone(),
                            harness: harness_key.clone(),
                            existing_memory_id: existing_id.clone(),
                        });
                        record_promoted(&mut state, action, &existing_id, None, &mut alias_to_id, &self.state_path)
                            .map_err(|error| partial_import_error(error, &report, &action.source_key))?;
                    }
                    (None, None) => {
                        record_unexpected_response(&mut report, &harness_key, action, "promoted-without-id");
                    }
                },
                GovernanceStatus::Candidate => {
                    if outcome.next_actions.iter().any(|s| s == "memory_supersede") {
                        let Some(existing_id) = outcome.existing_id.clone() else {
                            record_unexpected_response(
                                &mut report,
                                &harness_key,
                                action,
                                "candidate-supersede-without-existing-id",
                            );
                            continue;
                        };
                        self.apply_supersede(
                            client,
                            action,
                            &related,
                            &existing_id,
                            "import supersede",
                            "inspect supersede refusal",
                            "supersede",
                            &progress_prefix,
                            &harness_key,
                            &opts,
                            &mut state,
                            &mut report,
                            &mut alias_to_id,
                        )
                        .await?;
                    } else {
                        counters_mut(&mut report, &harness_key).written_candidate += 1;
                        push_candidate_entry(&mut report.candidates, action, &harness_key, outcome.id.clone());
                        if let Some(written_id) = outcome.id {
                            record_promoted(&mut state, action, &written_id, None, &mut alias_to_id, &self.state_path)
                                .map_err(|error| partial_import_error(error, &report, &action.source_key))?;
                        } else {
                            // A candidate without an id leaves the source unrecorded,
                            // so a re-run would re-write it (not idempotent). Surface
                            // it rather than silently dropping the bookkeeping, just
                            // like the promoted-without-id case above.
                            record_unexpected_response(&mut report, &harness_key, action, "candidate-without-id");
                        }
                    }
                }
                GovernanceStatus::Quarantined => {
                    counters_mut(&mut report, &harness_key).quarantined += 1;
                    push_candidate_entry(&mut report.quarantined, action, &harness_key, outcome.id.clone());
                    if let Some(written_id) = outcome.id {
                        record_promoted(&mut state, action, &written_id, None, &mut alias_to_id, &self.state_path)
                            .map_err(|error| partial_import_error(error, &report, &action.source_key))?;
                    } else {
                        record_unexpected_response(&mut report, &harness_key, action, "quarantined-without-id");
                    }
                }
                GovernanceStatus::Refused => {
                    let reason = outcome.reason.map_or("refused".to_string(), |reason| reason.as_str().to_string());
                    bump_refusal(counters_mut(&mut report, &harness_key), &reason);
                    report.refusals.push(RefusalEntry {
                        source_key: action.source_key.clone(),
                        harness: harness_key.clone(),
                        reason: reason.clone(),
                        suggested_next_action: None,
                    });
                    if opts.verbose_progress {
                        eprintln!("{progress_prefix} REFUSED ({reason}): {}", action.source_key);
                    }
                }
                GovernanceStatus::Tombstoned => {
                    // Tombstoned response on a write request — treat as refused.
                    bump_refusal(counters_mut(&mut report, &harness_key), "tombstone");
                    report.refusals.push(RefusalEntry {
                        source_key: action.source_key.clone(),
                        harness: harness_key.clone(),
                        reason: "tombstone".to_string(),
                        suggested_next_action: None,
                    });
                }
            }
        }

        if !opts.dry_run {
            if let Err(error) = materialize_planned_project_yamls(&mut report, &plan) {
                return Err(partial_import_error(error, &report, "<finalize>"));
            }
            if let Err(error) = state.save_canonical(&self.state_path) {
                return Err(partial_import_error(error, &report, "<finalize>"));
            }
        }
        Ok(ExecuteResult { report, state })
    }

    /// Issue one supersede write against `old_id` and reconcile its outcome.
    ///
    /// Both the bucket-repair path (a content-identical re-import into a new
    /// namespace bucket) and the candidate-supersede path (the daemon asked us
    /// to supersede an existing memory) run the identical Promoted/Refused/other
    /// state machine; only the `reason` sent to the daemon, the
    /// `suggested_next_action` recorded on a refusal, and the verbose-progress
    /// label differ per call site. Callers gate `dry_run` themselves before
    /// reaching here — this method always issues the daemon write.
    ///
    /// Before issuing a new `supersede`, this checks the daemon's supersession
    /// chain for `old_id`. If a memory in that chain already has the same
    /// content as the candidate, the candidate is adopted as the replacement
    /// and no new write is issued.
    #[expect(clippy::too_many_arguments, reason = "shared supersede state machine threads both call sites' context")]
    async fn apply_supersede<C: DaemonClient>(
        &self,
        client: &mut C,
        action: &PlannedWrite,
        related: &[String],
        old_id: &str,
        reason: &str,
        suggested_action: &str,
        verbose_label: &str,
        progress_prefix: &str,
        harness_key: &str,
        opts: &ExecuteOptions,
        state: &mut ImportState,
        report: &mut ImportReport,
        alias_to_id: &mut HashMap<String, String>,
    ) -> ImportResult<()> {
        let old_id = old_id.to_string();

        // F10: crash-mitigation. If a previous run superseded `old_id` but the
        // importer crashed before it could record the new id, the chain already
        // contains a replacement with the same content. Adopt it rather than
        // writing a duplicate. F15/F16/F17: walk the chain transitively, fail
        // closed on lookup errors, and only adopt active/pinned replacements.
        let chain = client
            .get_superseded_by_chain(&old_id)
            .await
            .map_err(|error| partial_import_error(error, report, &action.source_key))?;
        for new_id in chain {
            let get = client
                .get_memory(&new_id, true)
                .await
                .map_err(|error| partial_import_error(error, report, &action.source_key))?;
            if get.truncated {
                return Err(partial_import_error(
                    ImportError::Parse {
                        source_key: new_id.clone(),
                        reason: "supersede adoption requires full body; get response was truncated".to_string(),
                    },
                    report,
                    &action.source_key,
                ));
            }
            if !matches!(get.status, Some(MemoryStatus::Active) | Some(MemoryStatus::Pinned)) {
                continue;
            }
            let computed = ParsedMemory::compute_content_hash(&action.candidate.frontmatter_hint, &get.body);
            if computed == action.candidate.content_hash {
                counters_mut(report, harness_key).superseded += 1;
                record_promoted(state, action, &new_id, Some(old_id.as_str()), alias_to_id, &self.state_path)
                    .map_err(|error| partial_import_error(error, report, &action.source_key))?;
                return Ok(());
            }
        }

        let supersede_meta = build_write_meta(action, related, Some(std::slice::from_ref(&old_id)));
        let supersede = client
            .supersede(SupersedeRequest {
                old_id: old_id.clone(),
                content: action.candidate.body.clone(),
                reason: reason.to_string(),
                meta: supersede_meta,
            })
            .await
            .map_err(|error| partial_import_error(error, report, &action.source_key))?;
        match supersede.status {
            GovernanceStatus::Promoted => {
                let new_id = supersede.new_id.unwrap_or_else(|| old_id.clone());
                counters_mut(report, harness_key).superseded += 1;
                record_promoted(state, action, &new_id, Some(old_id.as_str()), alias_to_id, &self.state_path)
                    .map_err(|error| partial_import_error(error, report, &action.source_key))?;
            }
            GovernanceStatus::Refused => {
                let reason = supersede.reason.map_or("refused".to_string(), |reason| reason.as_str().to_string());
                bump_refusal(counters_mut(report, harness_key), &reason);
                report.refusals.push(RefusalEntry {
                    source_key: action.source_key.clone(),
                    harness: harness_key.to_string(),
                    reason,
                    suggested_next_action: Some(suggested_action.to_string()),
                });
            }
            other => {
                if opts.verbose_progress {
                    eprintln!("{progress_prefix} {verbose_label}-{other:?}: {}", action.source_key);
                }
                record_supersede_secondary(report, action, other, supersede.new_id.clone());
            }
        }
        Ok(())
    }
}

fn materialize_planned_project_yamls(report: &mut ImportReport, plan: &ImportPlan) -> ImportResult<()> {
    let mut seen = HashSet::new();
    for action in &plan.actions {
        let Some(project_yaml) = &action.scope.project_yaml else {
            continue;
        };
        if !matches!(project_yaml.action, ProjectYamlAction::PlannedWrite) || !seen.insert(project_yaml.path.clone()) {
            continue;
        }

        let cwd = action.candidate.cwd.as_deref().ok_or_else(|| ImportError::Parse {
            source_key: action.source_key.clone(),
            reason: "generated project yaml is missing cwd".to_string(),
        })?;
        let canonical_id = action.scope.canonical_namespace_id.as_deref().ok_or_else(|| ImportError::Parse {
            source_key: action.source_key.clone(),
            reason: "generated project yaml is missing canonical namespace id".to_string(),
        })?;
        write_generated_project_yaml(cwd, &project_yaml.path, canonical_id).map_err(|error| ImportError::Parse {
            source_key: action.source_key.clone(),
            reason: format!("write generated project yaml: {error}"),
        })?;
        report.mark_project_yaml_written(&project_yaml.path);
    }
    Ok(())
}

fn resolve_related_ids(action: &PlannedWrite, alias_to_id: &HashMap<String, String>) -> Vec<String> {
    action
        .wiki_link_targets_resolvable
        .iter()
        .filter_map(|alias| alias_to_id.get(&alias.to_ascii_lowercase()).cloned())
        .collect()
}

fn register_aliases_for(candidate: &ParsedMemory, id: &str, alias_to_id: &mut HashMap<String, String>) {
    for alias in candidate_aliases(candidate) {
        alias_to_id.entry(alias).or_insert_with(|| id.to_string());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportBucket {
    namespace: Option<String>,
    canonical_namespace_id: Option<String>,
}

fn target_import_bucket(scope: &ScopeBinding) -> ImportBucket {
    let namespace = match scope.scope {
        memory_substrate::Scope::Project | memory_substrate::Scope::Org => scope
            .namespace_alias
            .clone()
            .or_else(|| scope.canonical_namespace_id.clone())
            .or_else(|| scope.namespace.clone()),
        _ => scope.namespace.clone(),
    };
    ImportBucket { namespace, canonical_namespace_id: scope.canonical_namespace_id.clone() }
}

fn import_bucket_matches(record: &ImportRecord, scope: &ScopeBinding) -> bool {
    let expected = target_import_bucket(scope);
    if record.namespace.is_none() && record.canonical_namespace_id.is_none() {
        return expected.canonical_namespace_id.is_none();
    }
    record.namespace == expected.namespace && record.canonical_namespace_id == expected.canonical_namespace_id
}

pub(super) fn plan_action_for_record(
    record: &ImportRecord,
    record_key: &str,
    candidate_content_hash: &str,
    scope: &ScopeBinding,
) -> PlanAction {
    if record.content_hash == candidate_content_hash && import_bucket_matches(record, scope) {
        return PlanAction::SkipUnchanged {
            existing_memory_id: record.memory_id.clone(),
            existing_record_key: record_key.to_string(),
        };
    }
    if record.content_hash == candidate_content_hash {
        return bucket_repair_action(record);
    }
    PlanAction::Supersede {
        prior_memory_id: record.memory_id.clone(),
        prior_content_hash: record.content_hash.clone(),
    }
}

pub(super) fn bucket_repair_action(record: &ImportRecord) -> PlanAction {
    PlanAction::RepairBucket {
        prior_memory_id: record.memory_id.clone(),
        prior_content_hash: record.content_hash.clone(),
    }
}

/// Format a candidate's on-disk source path as a governance-groundable `file:`
/// reference.
///
/// Imported memories are promoted as `AgentPrimary`/`File` sources (see
/// `handlers::governance`), and the built-in `*-strict` policies require
/// grounding. `memory_governance::FileSourceResolver` only resolves a
/// `file:`-prefixed ABSOLUTE path that exists on disk; a bare path is
/// `Unsupported`, so every import write would be refused for grounding. The
/// harness source file exists at import time, so prefix `file:` and ensure the
/// path is absolute — canonicalizing a relative path against the importer's cwd
/// so the daemon can resolve it regardless of its own working directory.
fn groundable_source_ref(source_path: &Path) -> String {
    let absolute = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        // Prefer canonicalization (resolves symlinks and `..`), but if the file
        // is missing/inaccessible at import time, fall back to joining against
        // the current working directory so the ref stays *absolute*. A bare
        // relative `file:` ref is `Unsupported` to the grounding resolver and
        // would silently drop the candidate, so never emit one.
        std::fs::canonicalize(source_path).unwrap_or_else(|_| {
            std::env::current_dir().map(|cwd| cwd.join(source_path)).unwrap_or_else(|_| source_path.to_path_buf())
        })
    };
    format!("file:{}", absolute.display())
}

fn build_write_meta(action: &PlannedWrite, related: &[String], supersedes: Option<&[String]>) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "namespace".to_string(),
        Value::String(match action.scope.namespace.as_deref() {
            Some("project") => "project".to_string(),
            _ => "me".to_string(),
        }),
    );
    meta.insert("type".to_string(), Value::String("claim".to_string()));
    meta.insert("source_kind".to_string(), Value::String("import".to_string()));
    meta.insert("source_ref".to_string(), Value::String(groundable_source_ref(&action.candidate.source_path)));
    // Imported memories carry a confidence of 0.7 (plan R1 bump from 0.5) so
    // they stay above the Reality Check review threshold while still ranking
    // below hand-written `0.85` memories.
    meta.insert("confidence".to_string(), serde_json::json!(0.7));
    meta.insert("requires_user_confirmation".to_string(), Value::Bool(false));
    meta.insert("explicit_user_context".to_string(), Value::Bool(false));
    if let Some(canon) = &action.scope.canonical_namespace_id {
        meta.insert("canonical_namespace_id".to_string(), Value::String(canon.clone()));
    }
    if let Some(alias) = target_import_bucket(&action.scope).namespace {
        if action.scope.canonical_namespace_id.is_some() {
            meta.insert("namespace_alias".to_string(), Value::String(alias));
        }
    }
    // Entity & alias surface forms are derived from the parser hints. For
    // Codex Task Groups, `tags` already lives in frontmatter_hint as keywords;
    // for Claude topics, `name` lives there.
    if let Some(Value::Array(tags)) = action.candidate.frontmatter_hint.get("tags") {
        meta.insert("aliases".to_string(), Value::Array(tags.clone()));
    } else if let Some(title) = &action.candidate.title {
        meta.insert("aliases".to_string(), Value::Array(vec![Value::String(title.clone())]));
    }
    if !related.is_empty() {
        meta.insert("related".to_string(), Value::Array(related.iter().cloned().map(Value::String).collect()));
    }
    if let Some(prior) = supersedes {
        meta.insert("supersedes".to_string(), Value::Array(prior.iter().cloned().map(Value::String).collect()));
    } else {
        match &action.action {
            PlanAction::Supersede { prior_memory_id, .. } | PlanAction::RepairBucket { prior_memory_id, .. } => {
                meta.insert("supersedes".to_string(), Value::Array(vec![Value::String(prior_memory_id.clone())]));
            }
            PlanAction::SkipUnchanged { .. }
            | PlanAction::WriteNew
            | PlanAction::SkipByPrompt
            | PlanAction::ReportAmbiguous { .. } => {}
        }
    }
    if let Some(Value::Array(evidence_refs)) = action.candidate.frontmatter_hint.get("evidence_refs") {
        let evidence: Vec<Value> = evidence_refs
            .iter()
            .filter_map(|entry| {
                let obj = entry.as_object()?;
                let rollout_path = obj.get("rollout_path").and_then(Value::as_str)?;
                let mut out = serde_json::Map::new();
                out.insert("ref".to_string(), Value::String(format!("file://{rollout_path}")));
                if let Some(updated_at) = obj.get("updated_at").and_then(Value::as_str) {
                    out.insert("observed_at".to_string(), Value::String(updated_at.to_string()));
                }
                if let Some(thread_id) = obj.get("thread_id").and_then(Value::as_str) {
                    out.insert("quote".to_string(), Value::String(format!("rollout thread {thread_id}")));
                }
                Some(Value::Object(out))
            })
            .collect();
        if !evidence.is_empty() {
            meta.insert("evidence".to_string(), Value::Array(evidence));
        }
    }
    Value::Object(meta)
}

#[allow(clippy::too_many_arguments)]
fn record_promoted(
    state: &mut ImportState,
    action: &PlannedWrite,
    new_id: &str,
    superseded: Option<&str>,
    alias_to_id: &mut HashMap<String, String>,
    state_path: &Path,
) -> ImportResult<()> {
    let bucket = target_import_bucket(&action.scope);
    let mut record = ImportRecord {
        source_identity: action.candidate.import_identity(action.scope.canonical_namespace_id.as_deref()),
        source_key: action.source_key.clone(),
        source_memory_id: action.candidate.recovered_memory_id().map(str::to_string),
        memory_id: new_id.to_string(),
        content_hash: action.candidate.content_hash.clone(),
        imported_at: Utc::now(),
        harness: action.candidate.harness.as_str().to_string(),
        source_path_at_import: action.candidate.source_path.clone(),
        namespace: bucket.namespace,
        canonical_namespace_id: bucket.canonical_namespace_id,
        aliases: candidate_aliases(&action.candidate),
        supersession_chain: Vec::new(),
    };

    // Build the supersession chain from the prior record, if one exists.
    if let Some(prior_id) = superseded {
        if let Some(existing) = state.imports.values().find(|r| r.memory_id == prior_id) {
            record.supersession_chain = existing.supersession_chain.clone();
            record.supersession_chain.push(SupersededRecord {
                memory_id: prior_id.to_string(),
                content_hash: existing.content_hash.clone(),
                imported_at: existing.imported_at,
            });
        }
    }

    remove_import_record_duplicates(&mut state.imports, &record, superseded);
    state.imports.insert(record.source_identity.clone(), record);
    state.save_atomic(state_path)?;
    alias_to_id.insert(action.source_key.to_ascii_lowercase(), new_id.to_string());
    register_aliases_for(&action.candidate, new_id, alias_to_id);
    Ok(())
}

fn remove_import_record_duplicates(
    imports: &mut std::collections::BTreeMap<String, ImportRecord>,
    record: &ImportRecord,
    superseded: Option<&str>,
) {
    imports.retain(|key, existing| {
        if key == &record.source_identity {
            return false;
        }
        if existing.memory_id == record.memory_id {
            return false;
        }
        if let Some(id) = &record.source_memory_id {
            if existing.source_memory_id.as_deref() == Some(id) {
                return false;
            }
        }
        if let Some(old_id) = superseded {
            if existing.memory_id == old_id {
                return false;
            }
        }
        true
    });
}

fn bump_refusal(counters: &mut HarnessCounters, reason: &str) {
    match reason {
        "privacy" => counters.refused_privacy += 1,
        "contradiction" => counters.refused_contradiction += 1,
        "tombstone" => counters.refused_tombstone += 1,
        "grounding" => counters.refused_grounding += 1,
        "policy" => counters.refused_policy += 1,
        _ => counters.refused_other += 1,
    }
}

fn partial_import_error(error: ImportError, report: &ImportReport, source_key: &str) -> ImportError {
    ImportError::PartialExecute {
        source_key: source_key.to_string(),
        completed_writes: completed_write_count(report),
        source: Box::new(error),
    }
}

fn completed_write_count(report: &ImportReport) -> usize {
    report
        .harnesses
        .values()
        .map(|counters| counters.written_new + counters.superseded + counters.written_candidate + counters.quarantined)
        .sum()
}

/// Record a review-queue disposition (candidate or quarantine) so the JSON
/// report maps the source back to its harness and the daemon-assigned memory
/// id. `memory_id` is `None` when no id came back (e.g. a candidate-without-id
/// fallback); the counter and the list stay one-to-one regardless.
fn push_candidate_entry(
    entries: &mut Vec<CandidateEntry>,
    action: &PlannedWrite,
    harness_key: &str,
    memory_id: Option<String>,
) {
    entries.push(CandidateEntry { source_key: action.source_key.clone(), harness: harness_key.to_string(), memory_id });
}

/// Route a supersede / bucket-repair outcome whose status is neither `Promoted`
/// nor `Refused` to the correct reconciliation bucket, so the counters and the
/// `candidates[]` / `quarantined[]` lists stay one-to-one with the per-status
/// arms on the primary write path. `Quarantined` lands in the quarantine list,
/// `Tombstoned` is recorded as a refusal, and everything else (`Candidate`) is a
/// written candidate.
fn record_supersede_secondary(
    report: &mut ImportReport,
    action: &PlannedWrite,
    status: GovernanceStatus,
    new_id: Option<String>,
) {
    let harness_key = action.candidate.harness.as_str();
    match status {
        GovernanceStatus::Quarantined => {
            counters_mut(report, harness_key).quarantined += 1;
            push_candidate_entry(&mut report.quarantined, action, harness_key, new_id);
        }
        GovernanceStatus::Tombstoned => {
            bump_refusal(counters_mut(report, harness_key), "tombstone");
            report.refusals.push(RefusalEntry {
                source_key: action.source_key.clone(),
                harness: harness_key.to_string(),
                reason: "tombstone".to_string(),
                suggested_next_action: None,
            });
        }
        _ => {
            counters_mut(report, harness_key).written_candidate += 1;
            push_candidate_entry(&mut report.candidates, action, harness_key, new_id);
        }
    }
}

fn record_unexpected_response(report: &mut ImportReport, harness_key: &str, action: &PlannedWrite, reason: &str) {
    counters_mut(report, harness_key).refused_other += 1;
    report.refusals.push(RefusalEntry {
        source_key: action.source_key.clone(),
        harness: action.candidate.harness.as_str().to_string(),
        reason: reason.to_string(),
        suggested_next_action: Some("inspect daemon response".to_string()),
    });
}

fn counters_mut<'a>(report: &'a mut ImportReport, harness_key: &str) -> &'a mut HarnessCounters {
    report.harnesses.entry(harness_key.to_string()).or_default()
}
