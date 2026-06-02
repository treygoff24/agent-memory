//! Importer pipeline — planning (T05) and execution (T06).
//!
//! This module hosts both phases. T05 (this commit) implements
//! [`ImportEngine::plan`]: source discovery → parse → per-cwd prompts →
//! state-file dedup → topological sort by wiki-link dependency. T06 will
//! extend the impl block with `execute(plan)` that walks the topo-ordered
//! actions through the daemon socket.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::Value;

use crate::import::candidate::{Harness, ParsedMemory};
use crate::import::discovery::{
    discover_claude_memory_root, discover_codex_memory_root, ClaudeMemoryRoot, CodexMemoryRoot,
};
use crate::import::project_map::{
    write_generated_project_yaml, ProjectMapper, ProjectYamlAction, PromptBackend, ResolutionKind, ScopeBinding,
};
use crate::import::report::{DedupEntry, HarnessCounters, ImportReport, RefusalEntry};
use crate::import::sources::{claude, codex};
use crate::import::state::{ImportLockGuard, ImportRecord, ImportState, SupersededRecord};
use crate::import::{ImportError, ImportResult};
use crate::protocol::{GovernanceRefusalReason, GovernanceStatus, ProtocolError, ResponsePayload, ResponseResult};

/// Caller-supplied options for the planning phase.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Override the Claude memory root (T07's `--from-claude` flag).
    pub from_claude: Option<PathBuf>,
    /// Override the Codex memory root (`--from-codex`).
    pub from_codex: Option<PathBuf>,
    /// Restrict planning to a single harness; `None` means import everything.
    pub harness_filter: Option<HarnessFilter>,
    /// Pre-loaded state for idempotency checks. Disk-backed imports should go
    /// through [`run_import_session`] so the state file is loaded under the
    /// import lock.
    pub state: ImportState,
}

/// `--harness claude|codex|all` selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessFilter {
    Claude,
    Codex,
}

impl HarnessFilter {
    /// Whether this filter accepts the given harness. T06 uses this to drop
    /// post-parse candidates that aren't in scope when the user has restricted
    /// the run; T05's planner already short-circuits at discovery so this
    /// helper is also useful for downstream report rendering.
    pub fn includes(self, harness: Harness) -> bool {
        matches!((self, harness), (Self::Claude, Harness::ClaudeCode) | (Self::Codex, Harness::Codex))
    }
}

/// One topologically-ordered write action for the execute phase.
#[derive(Debug, Clone)]
pub struct PlannedWrite {
    pub source_key: String,
    pub candidate: ParsedMemory,
    pub scope: ScopeBinding,
    pub action: PlanAction,
    /// Wiki-link aliases that the topo sort resolved against later writes.
    /// These become `related: [memory_id]` once the target write completes.
    pub wiki_link_targets_resolvable: Vec<String>,
    /// Wiki-link aliases that form a back-edge in the source-key ordering and
    /// will be left as inert `[[name]]` text in the body (per the
    /// single-pass topological-ordering decision).
    pub wiki_link_targets_back_edge: Vec<String>,
}

/// Per-source action that the execute phase will perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    /// State file already records this source with a matching content hash —
    /// skip.
    SkipUnchanged { existing_memory_id: String },
    /// State file records this source under a different content hash —
    /// supersede the prior memory.
    Supersede { prior_memory_id: String, prior_content_hash: String },
    /// First time we've seen this source — write a fresh memory.
    WriteNew,
    /// Project mapper resolved to "skip" for this candidate's cwd.
    SkipByPrompt,
}

/// A back-edge wiki link that the topo sort had to break. Surfaced in the
/// import report for transparency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLinkBackEdge {
    pub source_key: String,
    pub alias: String,
}

/// Output of the planning phase. The execute phase walks `actions` in order,
/// each one ready to be turned into a single daemon request.
#[derive(Debug)]
pub struct ImportPlan {
    pub actions: Vec<PlannedWrite>,
    pub source_discovery_summary: DiscoverySummary,
    pub unresolved_back_edges: Vec<WikiLinkBackEdge>,
    pub parse_errors: Vec<ImportError>,
    pub state: ImportState,
}

/// Summary of where the parsers read from. Surfaced in the report so the user
/// can see which precedence rung each root came from.
#[derive(Debug, Clone, Default)]
pub struct DiscoverySummary {
    pub claude_root: Option<ClaudeMemoryRoot>,
    pub codex_root: Option<CodexMemoryRoot>,
    pub claude_candidates: usize,
    pub codex_candidates: usize,
}

/// Top-level importer engine. Owns the in-memory plan state across the
/// `plan()` + `execute()` calls.
pub struct ImportEngine {
    /// State file path on disk. Persisted between runs at
    /// `$MEMORUM_REPO/.memorum/import-state.json`.
    pub state_path: PathBuf,
}

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

/// Request shape for a `WriteMemory` daemon call. Bundling these into a struct
/// keeps the trait method's argument count manageable.
#[derive(Debug, Clone)]
pub struct WriteMemoryRequest {
    pub body: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub meta: Value,
}

/// Request shape for a `Supersede` daemon call.
#[derive(Debug, Clone)]
pub struct SupersedeRequest {
    pub old_id: String,
    pub content: String,
    pub reason: String,
    pub meta: Value,
}

/// A daemon client used by the execute phase. Production uses
/// [`SocketDaemonClient`] which forwards through `crate::client::request`.
/// Tests inject a `MockDaemonClient` over an in-memory script.
#[allow(async_fn_in_trait)]
pub trait DaemonClient {
    /// Issue a `RequestPayload::WriteMemory` with the given JSON-shaped meta.
    async fn write_memory(&mut self, request: WriteMemoryRequest) -> ImportResult<WriteMemoryOutcome>;

    /// Issue a `RequestPayload::Supersede` with the given prior id.
    async fn supersede(&mut self, request: SupersedeRequest) -> ImportResult<SupersedeOutcome>;
}

/// Production daemon client backed by the existing memoryd Unix socket.
pub struct SocketDaemonClient {
    socket_path: PathBuf,
    request_counter: usize,
}

impl SocketDaemonClient {
    /// Build a client pointed at the given memoryd socket.
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path, request_counter: 0 }
    }

    fn next_request_id(&mut self, prefix: &str) -> String {
        self.request_counter += 1;
        format!("{prefix}-{:06}", self.request_counter)
    }
}

impl DaemonClient for SocketDaemonClient {
    async fn write_memory(&mut self, request: WriteMemoryRequest) -> ImportResult<WriteMemoryOutcome> {
        let request_id = self.next_request_id("import-write");
        let payload = crate::protocol::RequestPayload::WriteMemory {
            body: request.body,
            title: request.title,
            tags: request.tags,
            meta: request.meta,
        };
        let envelope = crate::client::request(&self.socket_path, request_id, payload)
            .await
            .map_err(|error| ImportError::io(self.socket_path.clone(), std::io::Error::other(error.to_string())))?;
        let write = match envelope.result {
            ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write,
            ResponseResult::Error(error) => return Err(daemon_protocol_error("WriteMemory", error)),
            ResponseResult::Success(payload) => return Err(unexpected_daemon_payload("WriteMemory", &payload)),
        };
        Ok(WriteMemoryOutcome {
            status: write.status,
            id: write.id,
            existing_id: write.existing_id,
            next_actions: write.next_actions,
            reason: write.reason,
        })
    }

    async fn supersede(&mut self, request: SupersedeRequest) -> ImportResult<SupersedeOutcome> {
        let request_id = self.next_request_id("import-supersede");
        let payload = crate::protocol::RequestPayload::Supersede {
            old_id: request.old_id,
            content: request.content,
            reason: request.reason,
            meta: request.meta,
        };
        let envelope = crate::client::request(&self.socket_path, request_id, payload)
            .await
            .map_err(|error| ImportError::io(self.socket_path.clone(), std::io::Error::other(error.to_string())))?;
        let supersede = match envelope.result {
            ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) => supersede,
            ResponseResult::Error(error) => return Err(daemon_protocol_error("Supersede", error)),
            ResponseResult::Success(payload) => return Err(unexpected_daemon_payload("Supersede", &payload)),
        };
        Ok(SupersedeOutcome { status: supersede.status, new_id: supersede.new_id, reason: supersede.reason })
    }
}

fn daemon_protocol_error(operation: &str, error: ProtocolError) -> ImportError {
    ImportError::Parse {
        source_key: "<daemon>".to_string(),
        reason: format!(
            "{operation} failed with daemon error {}: {} (retryable={})",
            error.code, error.message, error.retryable
        ),
    }
}

fn unexpected_daemon_payload(operation: &str, payload: &ResponsePayload) -> ImportError {
    ImportError::Parse {
        source_key: "<daemon>".to_string(),
        reason: format!("{operation} returned unexpected daemon payload {}", response_payload_kind(payload)),
    }
}

fn response_payload_kind(payload: &ResponsePayload) -> &'static str {
    match payload {
        ResponsePayload::Status(_) => "Status",
        ResponsePayload::Doctor(_) => "Doctor",
        ResponsePayload::Search(_) => "Search",
        ResponsePayload::Get(_) => "Get",
        ResponsePayload::TrustArtifact(_) => "TrustArtifact",
        ResponsePayload::CaptureSource(_) => "CaptureSource",
        ResponsePayload::DashboardRoi(_) => "DashboardRoi",
        ResponsePayload::NotificationsRecent(_) => "NotificationsRecent",
        ResponsePayload::PolicyValidate(_) => "PolicyValidate",
        ResponsePayload::PolicyWrite(_) => "PolicyWrite",
        ResponsePayload::RecallHits(_) => "RecallHits",
        ResponsePayload::Reveal(_) => "Reveal",
        ResponsePayload::WriteNote(_) => "WriteNote",
        ResponsePayload::GovernanceWrite(_) => "GovernanceWrite",
        ResponsePayload::GovernanceSupersede(_) => "GovernanceSupersede",
        ResponsePayload::GovernanceForget(_) => "GovernanceForget",
        ResponsePayload::ReviewQueue(_) => "ReviewQueue",
        ResponsePayload::ReviewApprove(_) => "ReviewApprove",
        ResponsePayload::ReviewReject(_) => "ReviewReject",
        ResponsePayload::Startup(_) => "Startup",
        ResponsePayload::Delta(_) => "Delta",
        ResponsePayload::PeerHeartbeat(_) => "PeerHeartbeat",
        ResponsePayload::PeerStatus(_) => "PeerStatus",
        ResponsePayload::PeerActivity(_) => "PeerActivity",
        ResponsePayload::PeerReleaseLock(_) => "PeerReleaseLock",
        ResponsePayload::Observe(_) => "Observe",
        ResponsePayload::DreamNow(_) => "DreamNow",
        ResponsePayload::DreamStatus(_) => "DreamStatus",
        ResponsePayload::WebStatus(_) => "WebStatus",
        ResponsePayload::RealityCheck(_) => "RealityCheck",
        ResponsePayload::InspectEntities(_) => "InspectEntities",
        ResponsePayload::EventsLogPage(_) => "EventsLogPage",
        ResponsePayload::NamespaceTree(_) => "NamespaceTree",
        ResponsePayload::GovernancePolicyDump(_) => "GovernancePolicyDump",
        ResponsePayload::ConflictsList(_) => "ConflictsList",
        ResponsePayload::TestInjectEvent(_) => "TestInjectEvent",
    }
}

/// Outcome of a `WriteMemory` daemon call, normalised so the execute loop can
/// branch on it without re-handling JSON.
#[derive(Debug, Clone)]
pub struct WriteMemoryOutcome {
    pub status: GovernanceStatus,
    pub id: Option<String>,
    pub existing_id: Option<String>,
    pub next_actions: Vec<String>,
    pub reason: Option<GovernanceRefusalReason>,
}

#[derive(Debug, Clone)]
pub struct SupersedeOutcome {
    pub status: GovernanceStatus,
    pub new_id: Option<String>,
    pub reason: Option<GovernanceRefusalReason>,
}

/// Successful execute result, returned alongside the import report.
#[derive(Debug)]
pub struct ExecuteResult {
    pub report: ImportReport,
    pub state: ImportState,
}

/// Run a complete disk-backed import session behind the importer invariants:
/// acquire the import lock, load state, plan, execute, and let execution perform
/// crash-safe state persistence. CLI and setup-engine callers should use this
/// runner instead of hand-rolling lock/state plumbing.
#[expect(clippy::too_many_arguments, reason = "task contract keeps session dependencies explicit")]
pub async fn run_import_session<C: DaemonClient>(
    repo_root: &Path,
    mut options: ImportOptions,
    prompts: &mut dyn PromptBackend,
    client: &mut C,
    execute_options: ExecuteOptions,
) -> ImportResult<ExecuteResult> {
    let engine = ImportEngine::new(repo_root);
    let _lock = ImportLockGuard::acquire(&engine.state_path)?;
    options.state = ImportState::load(&engine.state_path)?;
    let plan = engine.plan_with_mode(options, prompts, execute_options.dry_run).await?;
    engine.execute(plan, execute_options, client).await
}

impl ImportEngine {
    /// Build an engine pointed at the conventional state-file path.
    pub fn new(repo_root: &Path) -> Self {
        Self { state_path: repo_root.join(".memorum").join("import-state.json") }
    }

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
        // wiki-links against previously-imported memories.
        for (source_key, record) in &state.imports {
            alias_to_id.insert(source_key.to_ascii_lowercase(), record.memory_id.clone());
        }

        for (index, action) in plan.actions.iter().enumerate() {
            let harness = action.candidate.harness;
            let harness_key = harness.as_str().to_string();
            report.harnesses.entry(harness_key.clone()).or_default();
            let progress_prefix = format!("[{}/{}]", index + 1, total);

            match &action.action {
                PlanAction::SkipUnchanged { existing_memory_id } => {
                    counters_mut(&mut report, &harness_key).skipped_idempotent += 1;
                    alias_to_id.insert(action.source_key.to_ascii_lowercase(), existing_memory_id.clone());
                    register_aliases_for(&action.candidate, existing_memory_id, &mut alias_to_id);
                    continue;
                }
                PlanAction::SkipByPrompt => {
                    counters_mut(&mut report, &harness_key).skipped_by_prompt += 1;
                    continue;
                }
                PlanAction::WriteNew | PlanAction::Supersede { .. } => {}
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
                .await?;

            match outcome.status {
                GovernanceStatus::Promoted => match (outcome.id, outcome.existing_id) {
                    (Some(written_id), None) => {
                        counters_mut(&mut report, &harness_key).written_new += 1;
                        record_promoted(&mut state, action, &written_id, None, &mut alias_to_id, &self.state_path)?;
                    }
                    (_, Some(existing_id)) => {
                        counters_mut(&mut report, &harness_key).dedup_existing += 1;
                        report.dedups.push(DedupEntry {
                            source_key: action.source_key.clone(),
                            harness: harness_key.clone(),
                            existing_memory_id: existing_id.clone(),
                        });
                        record_promoted(&mut state, action, &existing_id, None, &mut alias_to_id, &self.state_path)?;
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
                        let supersede_meta =
                            build_write_meta(action, &related, Some(std::slice::from_ref(&existing_id)));
                        let supersede = client
                            .supersede(SupersedeRequest {
                                old_id: existing_id.clone(),
                                content: action.candidate.body.clone(),
                                reason: "import supersede".to_string(),
                                meta: supersede_meta,
                            })
                            .await?;
                        match supersede.status {
                            GovernanceStatus::Promoted => {
                                let new_id = supersede.new_id.unwrap_or_else(|| existing_id.clone());
                                counters_mut(&mut report, &harness_key).superseded += 1;
                                record_promoted(
                                    &mut state,
                                    action,
                                    &new_id,
                                    Some(existing_id.as_str()),
                                    &mut alias_to_id,
                                    &self.state_path,
                                )?;
                            }
                            GovernanceStatus::Refused => {
                                let reason = supersede.reason.map_or("refused".to_string(), refusal_label);
                                bump_refusal(counters_mut(&mut report, &harness_key), &reason);
                                report.refusals.push(RefusalEntry {
                                    source_key: action.source_key.clone(),
                                    harness: harness_key.clone(),
                                    reason,
                                    suggested_next_action: Some("inspect supersede refusal".to_string()),
                                });
                            }
                            other => {
                                counters_mut(&mut report, &harness_key).written_candidate += 1;
                                if opts.verbose_progress {
                                    eprintln!("{progress_prefix} supersede-{:?}: {}", other, action.source_key);
                                }
                            }
                        }
                    } else {
                        counters_mut(&mut report, &harness_key).written_candidate += 1;
                        if let Some(written_id) = outcome.id {
                            record_promoted(&mut state, action, &written_id, None, &mut alias_to_id, &self.state_path)?;
                        }
                    }
                }
                GovernanceStatus::Quarantined => {
                    counters_mut(&mut report, &harness_key).quarantined += 1;
                    if let Some(written_id) = outcome.id {
                        record_promoted(&mut state, action, &written_id, None, &mut alias_to_id, &self.state_path)?;
                    }
                }
                GovernanceStatus::Refused => {
                    let reason = outcome.reason.map_or("refused".to_string(), refusal_label);
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
            materialize_planned_project_yamls(&mut report, &plan)?;
            state.save_canonical(&self.state_path)?;
        }
        Ok(ExecuteResult { report, state })
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
    if let Some(title) = &candidate.title {
        alias_to_id.entry(title.to_ascii_lowercase()).or_insert_with(|| id.to_string());
    }
    if let Some(short) = candidate.source_key.rsplit('/').next() {
        alias_to_id.entry(short.to_ascii_lowercase()).or_insert_with(|| id.to_string());
        if let Some(stem) = short.rsplit_once('.').map(|(s, _)| s) {
            alias_to_id.entry(stem.to_ascii_lowercase()).or_insert_with(|| id.to_string());
        }
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
        std::fs::canonicalize(source_path).unwrap_or_else(|_| source_path.to_path_buf())
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
    } else if let PlanAction::Supersede { prior_memory_id, .. } = &action.action {
        meta.insert("supersedes".to_string(), Value::Array(vec![Value::String(prior_memory_id.clone())]));
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
    let mut record = ImportRecord {
        memory_id: new_id.to_string(),
        content_hash: action.candidate.content_hash.clone(),
        imported_at: Utc::now(),
        harness: action.candidate.harness.as_str().to_string(),
        source_path_at_import: action.candidate.source_path.clone(),
        supersession_chain: Vec::new(),
    };
    if let Some(prior_id) = superseded {
        if let Some(existing) = state.imports.get(&action.source_key) {
            record.supersession_chain = existing.supersession_chain.clone();
            record.supersession_chain.push(SupersededRecord {
                memory_id: prior_id.to_string(),
                content_hash: existing.content_hash.clone(),
                imported_at: existing.imported_at,
            });
        }
    }
    state.imports.insert(action.source_key.clone(), record);
    state.save_atomic(state_path)?;
    alias_to_id.insert(action.source_key.to_ascii_lowercase(), new_id.to_string());
    register_aliases_for(&action.candidate, new_id, alias_to_id);
    Ok(())
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

fn refusal_label(reason: GovernanceRefusalReason) -> String {
    match reason {
        GovernanceRefusalReason::Privacy => "privacy".to_string(),
        GovernanceRefusalReason::Contradiction => "contradiction".to_string(),
        GovernanceRefusalReason::Tombstone => "tombstone".to_string(),
        GovernanceRefusalReason::Grounding => "grounding".to_string(),
        GovernanceRefusalReason::Policy => "policy".to_string(),
        GovernanceRefusalReason::Superseded => "superseded".to_string(),
        GovernanceRefusalReason::ReviewRequired => "review_required".to_string(),
    }
}

impl ImportEngine {
    /// Planning phase. Discovers sources, parses, asks the project mapper for
    /// each unique non-git cwd, applies state-file dedup, topologically sorts
    /// the resulting actions by wiki-link dependency.
    pub async fn plan(&self, options: ImportOptions, prompts: &mut dyn PromptBackend) -> ImportResult<ImportPlan> {
        self.plan_with_mode(options, prompts, false).await
    }

    async fn plan_with_mode(
        &self,
        options: ImportOptions,
        prompts: &mut dyn PromptBackend,
        plan_only: bool,
    ) -> ImportResult<ImportPlan> {
        let mut parse_errors = Vec::new();
        let mut candidates: Vec<ParsedMemory> = Vec::new();

        // Pass 0: discovery.
        let claude_root = if options.harness_filter.is_none_or(|f| matches!(f, HarnessFilter::Claude)) {
            discover_claude_memory_root(options.from_claude.as_deref())?
        } else {
            None
        };
        let codex_root = if options.harness_filter.is_none_or(|f| matches!(f, HarnessFilter::Codex)) {
            discover_codex_memory_root(options.from_codex.as_deref())?
        } else {
            None
        };

        // Pass 1: parse.
        let claude_count = if let Some(root) = &claude_root {
            let output = claude::parse(&root.path)?;
            let count = output.candidates.len();
            candidates.extend(output.candidates);
            parse_errors.extend(output.errors);
            count
        } else {
            0
        };
        let codex_count = if let Some(root) = &codex_root {
            let output = codex::parse(&root.path)?;
            let count = output.candidates.len();
            candidates.extend(output.candidates);
            parse_errors.extend(output.errors);
            count
        } else {
            0
        };

        // Pass 2: per-cwd project mapping. Walk unique cwds in deterministic
        // order so prompt order is stable across runs.
        let mut mapper = ProjectMapper::new(plan_only);
        let mut cwd_to_scope: HashMap<Option<PathBuf>, ScopeBinding> = HashMap::new();
        let mut ordered_cwds: Vec<Option<PathBuf>> = Vec::new();
        let mut seen_cwds: HashSet<Option<PathBuf>> = HashSet::new();
        for candidate in &candidates {
            if seen_cwds.insert(candidate.cwd.clone()) {
                ordered_cwds.push(candidate.cwd.clone());
            }
        }
        for cwd in ordered_cwds {
            let scope = mapper.resolve(cwd.as_deref(), prompts).await.map_err(|error| ImportError::Parse {
                source_key: cwd.as_deref().map_or("<none>".to_string(), |c| c.display().to_string()),
                reason: format!("project mapping: {error}"),
            })?;
            cwd_to_scope.insert(cwd, scope);
        }

        // Pass 3: state-file dedup. Determine each candidate's action.
        let state = options.state;
        let mut prelim: Vec<PlannedWrite> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let scope = cwd_to_scope.get(&candidate.cwd).cloned().unwrap_or_else(|| ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            });
            let action = if matches!(scope.resolution, ResolutionKind::PromptedSkip) {
                PlanAction::SkipByPrompt
            } else {
                match state.imports.get(&candidate.source_key) {
                    Some(record) if record.content_hash == candidate.content_hash => {
                        PlanAction::SkipUnchanged { existing_memory_id: record.memory_id.clone() }
                    }
                    Some(record) => PlanAction::Supersede {
                        prior_memory_id: record.memory_id.clone(),
                        prior_content_hash: record.content_hash.clone(),
                    },
                    None => PlanAction::WriteNew,
                }
            };
            prelim.push(PlannedWrite {
                source_key: candidate.source_key.clone(),
                candidate,
                scope,
                action,
                wiki_link_targets_resolvable: Vec::new(),
                wiki_link_targets_back_edge: Vec::new(),
            });
        }

        // Pass 4: topological sort by wiki-link dependency. Edges go from
        // source memory → wiki-link target memory; we sort so each write
        // happens after its dependencies. Back-edges in cycles get marked
        // and the alias is preserved as inert text in the body.
        let (actions, back_edges) = topo_sort(prelim);

        Ok(ImportPlan {
            actions,
            source_discovery_summary: DiscoverySummary {
                claude_root,
                codex_root,
                claude_candidates: claude_count,
                codex_candidates: codex_count,
            },
            unresolved_back_edges: back_edges,
            parse_errors,
            state,
        })
    }
}

/// Topological sort over wiki-link dependencies. Returns the sorted action list
/// (each write follows its wiki-link targets) and a list of back-edges that
/// were broken to resolve cycles.
fn topo_sort(actions: Vec<PlannedWrite>) -> (Vec<PlannedWrite>, Vec<WikiLinkBackEdge>) {
    // Build an alias → source_key index. Aliases come from each candidate's
    // title (for Codex Task Groups: the header; for Claude: the topic name)
    // plus the candidate's source_key suffix. We index by lowercased alias so
    // wiki-link matching is case-insensitive.
    let mut alias_to_key: HashMap<String, String> = HashMap::new();
    let mut sorted_keys: Vec<String> = actions.iter().map(|w| w.source_key.clone()).collect();
    sorted_keys.sort();
    let key_order: HashMap<String, usize> = sorted_keys.iter().enumerate().map(|(i, k)| (k.clone(), i)).collect();
    for write in &actions {
        if let Some(title) = &write.candidate.title {
            alias_to_key.entry(title.to_ascii_lowercase()).or_insert_with(|| write.source_key.clone());
        }
        // Also index by short source-key segment so `[[file.md]]` style links
        // resolve against file-named candidates.
        if let Some(short) = write.source_key.rsplit('/').next() {
            alias_to_key.entry(short.to_ascii_lowercase()).or_insert_with(|| write.source_key.clone());
            if let Some(stem) = short.rsplit_once('.').map(|(s, _)| s) {
                alias_to_key.entry(stem.to_ascii_lowercase()).or_insert_with(|| write.source_key.clone());
            }
        }
    }

    // Edges: source_key → set of target source_keys it depends on.
    let mut deps: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut resolvable_aliases: HashMap<String, BTreeMap<String, String>> = HashMap::new();
    let mut back_edge_aliases: HashMap<String, Vec<String>> = HashMap::new();
    let mut back_edges = Vec::new();

    for write in &actions {
        let from_key = write.source_key.clone();
        let from_order = key_order.get(&from_key).copied().unwrap_or(usize::MAX);
        for alias in &write.candidate.wiki_links {
            let lowered = alias.to_ascii_lowercase();
            if let Some(target_key) = alias_to_key.get(&lowered).cloned() {
                if target_key == from_key {
                    // self-link; treat as back-edge so we don't loop
                    back_edge_aliases.entry(from_key.clone()).or_default().push(alias.clone());
                    back_edges.push(WikiLinkBackEdge { source_key: from_key.clone(), alias: alias.clone() });
                    continue;
                }
                let target_order = key_order.get(&target_key).copied().unwrap_or(usize::MAX);
                if from_order < target_order {
                    // Forward edge: write must come after the target.
                    deps.entry(from_key.clone()).or_default().insert(target_key.clone());
                    resolvable_aliases.entry(from_key.clone()).or_default().insert(alias.clone(), target_key);
                } else {
                    // Back-edge in source-key order — break deterministically.
                    back_edge_aliases.entry(from_key.clone()).or_default().push(alias.clone());
                    back_edges.push(WikiLinkBackEdge { source_key: from_key.clone(), alias: alias.clone() });
                }
            }
            // Unresolvable aliases (no candidate to link to) stay as inert
            // body text — they're not back-edges, they're just dangling.
        }
    }

    // Kahn's algorithm: in-degree sort over the forward-edge DAG. Tiebreak
    // ties by source-key for determinism.
    let mut in_degree: BTreeMap<String, usize> = actions.iter().map(|w| (w.source_key.clone(), 0)).collect();
    let mut reverse_adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (from, targets) in &deps {
        for target in targets {
            *in_degree.entry(from.clone()).or_default() += 1;
            reverse_adj.entry(target.clone()).or_default().insert(from.clone());
        }
    }

    let mut queue: VecDeque<String> = in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(k, _)| k.clone()).collect();
    let mut sorted_order: Vec<String> = Vec::with_capacity(actions.len());
    while let Some(key) = queue.pop_front() {
        sorted_order.push(key.clone());
        let dependents = reverse_adj.remove(&key).unwrap_or_default();
        let mut newly_zero: Vec<String> = Vec::new();
        for dependent in dependents {
            let entry = in_degree.entry(dependent.clone()).or_default();
            if *entry > 0 {
                *entry -= 1;
            }
            if *entry == 0 {
                newly_zero.push(dependent);
            }
        }
        newly_zero.sort();
        for next in newly_zero {
            queue.push_back(next);
        }
    }

    // Any unconsumed nodes form a cycle. Walk them in source-key order, mark
    // their lowest-index incoming edge as a back-edge, and add them last.
    if sorted_order.len() < actions.len() {
        let remaining: Vec<String> =
            actions.iter().map(|w| w.source_key.clone()).filter(|k| !sorted_order.contains(k)).collect();
        for key in remaining {
            // Find the back-edge to break: a dep this key still has whose
            // target is also unfinished.
            if let Some(targets) = deps.get(&key) {
                for target in targets {
                    if !sorted_order.contains(target) {
                        back_edge_aliases.entry(key.clone()).or_default().push(format!("<cycle:{target}>"));
                        back_edges
                            .push(WikiLinkBackEdge { source_key: key.clone(), alias: format!("<cycle:{target}>") });
                    }
                }
            }
            sorted_order.push(key);
        }
    }

    let mut by_key: HashMap<String, PlannedWrite> = actions.into_iter().map(|w| (w.source_key.clone(), w)).collect();
    let mut output = Vec::with_capacity(sorted_order.len());
    for key in sorted_order {
        if let Some(mut write) = by_key.remove(&key) {
            let resolvable = resolvable_aliases.remove(&write.source_key).unwrap_or_default();
            write.wiki_link_targets_resolvable = resolvable.into_keys().collect();
            write.wiki_link_targets_back_edge = back_edge_aliases.remove(&write.source_key).unwrap_or_default();
            output.push(write);
        }
    }
    (output, back_edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::candidate::Harness;
    use crate::import::project_map::{PromptResult, PromptedDisposition};
    use crate::import::state::ImportRecord;
    use chrono::Utc;

    struct NoPrompts;
    impl PromptBackend for NoPrompts {
        fn prompt_non_git_cwd(&mut self, _cwd: &Path, _synced_dir: Option<&'static str>) -> PromptResult {
            PromptResult { disposition: PromptedDisposition::DropToMe, synced_dir_confirmed: None }
        }
    }

    fn make_candidate(source_key: &str, body: &str, wiki_links: Vec<String>) -> ParsedMemory {
        let mut hint = BTreeMap::new();
        hint.insert("name".to_string(), serde_json::Value::String(source_key.to_string()));
        let content_hash = ParsedMemory::compute_content_hash(&hint, body);
        ParsedMemory {
            source_key: source_key.to_string(),
            source_path: PathBuf::from(format!("/fixture/{source_key}")),
            content_hash,
            harness: Harness::ClaudeCode,
            frontmatter_hint: hint,
            body: body.to_string(),
            wiki_links,
            cwd: None,
            title: Some(source_key.to_string()),
        }
    }

    fn make_planned(source_key: &str, body: &str, wiki_links: Vec<String>, action: PlanAction) -> PlannedWrite {
        PlannedWrite {
            source_key: source_key.to_string(),
            candidate: make_candidate(source_key, body, wiki_links),
            scope: ScopeBinding {
                scope: memory_substrate::Scope::User,
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
                project_yaml: None,
            },
            action,
            wiki_link_targets_resolvable: Vec::new(),
            wiki_link_targets_back_edge: Vec::new(),
        }
    }

    #[test]
    fn daemon_protocol_error_keeps_code_message_and_retryability() {
        let error = daemon_protocol_error(
            "WriteMemory",
            ProtocolError {
                code: "invalid_request".to_string(),
                message: "title is required".to_string(),
                retryable: false,
            },
        );

        let ImportError::Parse { source_key, reason } = error else {
            panic!("daemon errors should stay reportable parse-style failures");
        };
        assert_eq!(source_key, "<daemon>");
        assert!(reason.contains("WriteMemory"));
        assert!(reason.contains("invalid_request"));
        assert!(reason.contains("title is required"));
        assert!(reason.contains("retryable=false"));
    }

    #[test]
    fn unexpected_daemon_payload_names_operation_and_payload() {
        let error = unexpected_daemon_payload(
            "Supersede",
            &ResponsePayload::Status(crate::protocol::StatusResponse::default()),
        );

        let ImportError::Parse { reason, .. } = error else {
            panic!("unexpected payloads should stay reportable parse-style failures");
        };
        assert!(reason.contains("Supersede"));
        assert!(reason.contains("Status"));
    }

    #[test]
    fn unexpected_daemon_payload_omits_payload_body_content() {
        let error = unexpected_daemon_payload(
            "WriteMemory",
            &ResponsePayload::Get(crate::protocol::GetResponse {
                id: "mem-secret".to_string(),
                summary: "private summary".to_string(),
                body: "SECRET MEMORY BODY".to_string(),
                truncated: false,
                provenance: None,
                guidance: "private guidance".to_string(),
            }),
        );

        let ImportError::Parse { reason, .. } = error else {
            panic!("unexpected payloads should stay reportable parse-style failures");
        };
        assert!(reason.contains("WriteMemory"));
        assert!(reason.contains("Get"));
        assert!(!reason.contains("SECRET MEMORY BODY"));
        assert!(!reason.contains("private summary"));
        assert!(!reason.contains("private guidance"));
    }

    #[test]
    fn topo_sort_orders_a_to_b_to_c_as_c_b_a() {
        // A links to B, B links to C; so we should write C first, then B, then A.
        let actions = vec![
            make_planned("a", "see [[b]]", vec!["b".to_string()], PlanAction::WriteNew),
            make_planned("b", "see [[c]]", vec!["c".to_string()], PlanAction::WriteNew),
            make_planned("c", "leaf", Vec::new(), PlanAction::WriteNew),
        ];
        let (sorted, back_edges) = topo_sort(actions);
        let order: Vec<&str> = sorted.iter().map(|w| w.source_key.as_str()).collect();
        assert_eq!(order, vec!["c", "b", "a"]);
        assert!(back_edges.is_empty(), "no back-edges in a tree");
        let a = sorted.iter().find(|w| w.source_key == "a").unwrap();
        assert_eq!(a.wiki_link_targets_resolvable, vec!["b".to_string()]);
    }

    #[test]
    fn topo_sort_breaks_cycle_at_higher_source_key_outgoing_edge() {
        // A→B→A cycle. The deterministic break rule (in topo_sort) cuts the
        // outgoing edge from the higher source-key node (`b → a`), leaving
        // `a → b` as the only remaining forward edge. So `b` writes first
        // (leaf in the DAG after the cut), `a` writes second carrying
        // `related: [b]`, and `b`'s body keeps `[[a]]` as inert text.
        let actions = vec![
            make_planned("a", "see [[b]]", vec!["b".to_string()], PlanAction::WriteNew),
            make_planned("b", "see [[a]]", vec!["a".to_string()], PlanAction::WriteNew),
        ];
        let (sorted, back_edges) = topo_sort(actions);
        let order: Vec<&str> = sorted.iter().map(|w| w.source_key.as_str()).collect();
        assert_eq!(order, vec!["b", "a"]);
        assert_eq!(back_edges.len(), 1);
        assert_eq!(back_edges[0].source_key, "b");
        assert_eq!(back_edges[0].alias, "a");
        let a = sorted.iter().find(|w| w.source_key == "a").unwrap();
        assert_eq!(a.wiki_link_targets_resolvable, vec!["b".to_string()]);
        let b = sorted.iter().find(|w| w.source_key == "b").unwrap();
        assert_eq!(b.wiki_link_targets_back_edge, vec!["a".to_string()]);
    }

    #[test]
    fn topo_sort_unresolvable_alias_stays_inert_not_back_edge() {
        let actions = vec![make_planned("a", "see [[missing]]", vec!["missing".to_string()], PlanAction::WriteNew)];
        let (sorted, back_edges) = topo_sort(actions);
        assert_eq!(sorted.len(), 1);
        assert!(sorted[0].wiki_link_targets_resolvable.is_empty(), "unresolved alias is not a forward link");
        assert!(sorted[0].wiki_link_targets_back_edge.is_empty(), "unresolved alias is not a back-edge");
        assert!(back_edges.is_empty(), "unresolved aliases are inert body text");
    }

    #[tokio::test]
    async fn first_run_produces_all_write_new_actions() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nbody a\n").expect("write");
        std::fs::write(claude_root.join("proj/memory/b.md"), b"---\nname: B\n---\nbody b\n").expect("write");

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: Some(claude_root),
                    from_codex: Some(PathBuf::from("/does/not/exist")),
                    harness_filter: None,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("plan ok");
        assert_eq!(plan.actions.len(), 2);
        for action in &plan.actions {
            assert!(matches!(action.action, PlanAction::WriteNew));
        }
    }

    #[tokio::test]
    async fn second_run_with_unchanged_content_produces_skip_unchanged() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        let body = b"---\nname: A\n---\nbody a\n";
        std::fs::write(claude_root.join("proj/memory/a.md"), body).expect("write");

        // Construct a state record with the matching content hash.
        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let first_plan = engine
            .plan(
                ImportOptions {
                    from_claude: Some(claude_root.clone()),
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("first plan");
        let source_key = first_plan.actions[0].source_key.clone();
        let content_hash = first_plan.actions[0].candidate.content_hash.clone();

        let mut state = ImportState::default();
        state.imports.insert(
            source_key.clone(),
            ImportRecord {
                memory_id: "mem_existing".to_string(),
                content_hash,
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("ignored"),
                supersession_chain: Vec::new(),
            },
        );

        let mut prompts2 = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: Some(claude_root),
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    state,
                },
                &mut prompts2,
            )
            .await
            .expect("second plan");
        assert_eq!(plan.actions.len(), 1);
        assert!(
            matches!(plan.actions[0].action, PlanAction::SkipUnchanged { ref existing_memory_id } if existing_memory_id == "mem_existing")
        );
    }

    #[tokio::test]
    async fn second_run_with_changed_content_produces_supersede() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nNEW body\n").expect("write");

        let mut state = ImportState::default();
        // The candidate source key will be "claude:proj/memory/a.md"; we
        // record a stale hash.
        state.imports.insert(
            "claude:proj/memory/a.md".to_string(),
            ImportRecord {
                memory_id: "mem_old".to_string(),
                content_hash: "sha256:STALE".to_string(),
                imported_at: Utc::now(),
                harness: "claude-code".to_string(),
                source_path_at_import: PathBuf::from("ignored"),
                supersession_chain: Vec::new(),
            },
        );

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: Some(claude_root),
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: None,
                    state,
                },
                &mut prompts,
            )
            .await
            .expect("plan");
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0].action {
            PlanAction::Supersede { prior_memory_id, prior_content_hash } => {
                assert_eq!(prior_memory_id, "mem_old");
                assert_eq!(prior_content_hash, "sha256:STALE");
            }
            other => panic!("expected Supersede, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn harness_filter_excludes_other_harness() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_root = tmp.path().join("claude");
        std::fs::create_dir_all(claude_root.join("proj/memory")).expect("mkdir");
        std::fs::write(claude_root.join("proj/memory/a.md"), b"---\nname: A\n---\nbody\n").expect("write");

        let engine = ImportEngine::new(tmp.path());
        let mut prompts = NoPrompts;
        let plan = engine
            .plan(
                ImportOptions {
                    from_claude: Some(claude_root),
                    from_codex: Some(PathBuf::from("/no")),
                    harness_filter: Some(HarnessFilter::Codex),
                    state: ImportState::default(),
                },
                &mut prompts,
            )
            .await
            .expect("plan");
        assert!(plan.actions.is_empty(), "Claude root not scanned under --harness codex");
        assert!(plan.source_discovery_summary.claude_root.is_none());
    }

    // -----------------------------------------------------------------------------------
    // T06: execute-phase tests. The MockDaemonClient lets each test script the
    // daemon's response per request, so we can exercise the branching matrix
    // (status × existing_id × next_actions) and assert the importer's bookkeeping
    // without spinning up a real daemon.
    // -----------------------------------------------------------------------------------

    #[derive(Debug, Default)]
    struct MockDaemonClient {
        write_responses: std::collections::VecDeque<WriteMemoryOutcome>,
        supersede_responses: std::collections::VecDeque<SupersedeOutcome>,
        write_calls: Vec<MockWriteCall>,
        supersede_calls: Vec<MockSupersedeCall>,
    }

    #[derive(Debug, Clone)]
    struct MockWriteCall {
        #[allow(dead_code)]
        body: String,
        #[allow(dead_code)]
        title: Option<String>,
        meta: Value,
    }

    #[derive(Debug, Clone)]
    struct MockSupersedeCall {
        old_id: String,
        #[allow(dead_code)]
        content: String,
        #[allow(dead_code)]
        meta: Value,
    }

    impl MockDaemonClient {
        fn push_write(mut self, outcome: WriteMemoryOutcome) -> Self {
            self.write_responses.push_back(outcome);
            self
        }
        fn push_supersede(mut self, outcome: SupersedeOutcome) -> Self {
            self.supersede_responses.push_back(outcome);
            self
        }
    }

    impl DaemonClient for MockDaemonClient {
        async fn write_memory(&mut self, request: WriteMemoryRequest) -> ImportResult<WriteMemoryOutcome> {
            self.write_calls.push(MockWriteCall {
                body: request.body.clone(),
                title: request.title.clone(),
                meta: request.meta.clone(),
            });
            Ok(self.write_responses.pop_front().unwrap_or(WriteMemoryOutcome {
                status: GovernanceStatus::Promoted,
                id: Some(format!("mem_mock_{:04}", self.write_calls.len())),
                existing_id: None,
                next_actions: Vec::new(),
                reason: None,
            }))
        }

        async fn supersede(&mut self, request: SupersedeRequest) -> ImportResult<SupersedeOutcome> {
            self.supersede_calls.push(MockSupersedeCall {
                old_id: request.old_id,
                content: request.content,
                meta: request.meta,
            });
            Ok(self.supersede_responses.pop_front().unwrap_or(SupersedeOutcome {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_supersede".to_string()),
                reason: None,
            }))
        }
    }

    fn plan_with_actions(actions: Vec<PlannedWrite>) -> ImportPlan {
        ImportPlan {
            actions,
            source_discovery_summary: DiscoverySummary::default(),
            unresolved_back_edges: Vec::new(),
            parse_errors: Vec::new(),
            state: ImportState::default(),
        }
    }

    #[tokio::test]
    async fn execute_records_promoted_id_and_increments_written_new() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: Some("mem_abc".to_string()),
            existing_id: None,
            next_actions: Vec::new(),
            reason: None,
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("claude bucket");
        assert_eq!(claude.written_new, 1);
        assert_eq!(claude.refused_other, 0);
        assert_eq!(result.state.imports.get("a").map(|r| r.memory_id.clone()), Some("mem_abc".to_string()));
        assert_eq!(client.write_calls.len(), 1, "single socket call");
    }

    #[tokio::test]
    async fn execute_promoted_with_existing_id_counts_as_dedup_not_new() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Promoted,
            id: None,
            existing_id: Some("mem_existing".to_string()),
            next_actions: Vec::new(),
            reason: None,
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.dedup_existing, 1);
        assert_eq!(claude.written_new, 0);
        assert_eq!(result.report.dedups.len(), 1);
        assert_eq!(result.state.imports.get("a").map(|r| r.memory_id.clone()), Some("mem_existing".to_string()));
    }

    #[tokio::test]
    async fn execute_candidate_with_supersede_next_action_issues_followup_supersede() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default()
            .push_write(WriteMemoryOutcome {
                status: GovernanceStatus::Candidate,
                id: None,
                existing_id: Some("mem_prior".to_string()),
                next_actions: vec!["memory_supersede".to_string()],
                reason: None,
            })
            .push_supersede(SupersedeOutcome {
                status: GovernanceStatus::Promoted,
                new_id: Some("mem_new".to_string()),
                reason: None,
            });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        assert_eq!(client.supersede_calls.len(), 1);
        assert_eq!(client.supersede_calls[0].old_id, "mem_prior");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.superseded, 1);
        let record = result.state.imports.get("a").expect("state record");
        assert_eq!(record.memory_id, "mem_new");
        assert_eq!(record.supersession_chain.len(), 0, "supersession chain only carries prior state-file records");
    }

    #[tokio::test]
    async fn execute_refusal_appends_to_report_and_does_not_record_state() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default().push_write(WriteMemoryOutcome {
            status: GovernanceStatus::Refused,
            id: None,
            existing_id: None,
            next_actions: Vec::new(),
            reason: Some(GovernanceRefusalReason::Privacy),
        });
        let result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.refused_privacy, 1);
        assert_eq!(result.report.refusals.len(), 1);
        assert_eq!(result.report.refusals[0].reason, "privacy");
        assert!(!result.state.imports.contains_key("a"), "refused writes never land in state file");
    }

    #[tokio::test]
    async fn execute_dry_run_performs_zero_socket_calls_and_zero_state_writes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        let actions = vec![make_planned("a", "body a", Vec::new(), PlanAction::WriteNew)];
        let mut client = MockDaemonClient::default();
        let opts = ExecuteOptions { dry_run: true, verbose_progress: false };
        let result = engine.execute(plan_with_actions(actions), opts, &mut client).await.expect("dry-run ok");
        assert_eq!(client.write_calls.len(), 0);
        assert_eq!(client.supersede_calls.len(), 0);
        // Dry-run still counts the intended write so the report previews the scope.
        let claude = result.report.harnesses.get("claude-code").expect("bucket");
        assert_eq!(claude.written_new, 1);
        // No state file written to disk.
        assert!(!engine.state_path.exists());
    }

    #[tokio::test]
    async fn execute_resolves_wiki_links_against_in_flight_alias_map() {
        let tmp = tempfile::tempdir().expect("tmp");
        let engine = ImportEngine::new(tmp.path());
        // Topo-sorted: b first (leaf), a second (depends on b).
        let mut actions = vec![
            make_planned("a", "see [[b]]", vec!["b".to_string()], PlanAction::WriteNew),
            make_planned("b", "leaf", Vec::new(), PlanAction::WriteNew),
        ];
        let (sorted, _back_edges) = topo_sort(actions.clone());
        actions = sorted;
        let mut client = MockDaemonClient::default()
            .push_write(WriteMemoryOutcome {
                status: GovernanceStatus::Promoted,
                id: Some("mem_b".to_string()),
                existing_id: None,
                next_actions: Vec::new(),
                reason: None,
            })
            .push_write(WriteMemoryOutcome {
                status: GovernanceStatus::Promoted,
                id: Some("mem_a".to_string()),
                existing_id: None,
                next_actions: Vec::new(),
                reason: None,
            });
        let _result = engine
            .execute(plan_with_actions(actions), ExecuteOptions::default(), &mut client)
            .await
            .expect("execute ok");
        // The second write (a) must have carried related=[mem_b] in its meta.
        let a_call = client.write_calls.last().expect("at least one call");
        let related = a_call
            .meta
            .get("related")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        assert_eq!(related, vec!["mem_b"], "wiki-link target resolved against in-flight alias map");
    }
}
