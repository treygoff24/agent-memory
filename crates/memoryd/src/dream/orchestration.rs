use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, NaiveDate, Utc};
use memory_privacy::{
    DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyLabel, PrivacyNamespace, PrivacySpan,
    PrivacyStorageAction,
};
use memory_substrate::{
    config::PromptVersion, Author, AuthorKind, AuxScope, ClassificationOutcome, Entity, EventContext, Evidence,
    Frontmatter, Memory, MemoryStatus, MemoryType, RecallIndexQuery, RepoPath, RetrievalPolicy, Scope, Sensitivity,
    Source, SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::protocol::{CandidateWriteResult, NotificationEvent};

#[cfg(not(any(test, feature = "dev-fixtures")))]
use super::{error::HarnessCliError, harness::AuthProbeResult};
#[cfg(any(test, feature = "dev-fixtures"))]
use super::{harness::EchoCli, run::deterministic_echo_harness};
use super::{
    harness::{HarnessCli, HarnessFuture},
    registry::HarnessCliRegistry,
    run::{
        CandidateEvidenceRef, CandidateWriteRequest, CandidateWriter, DreamActiveMemoryInput, DreamRunOptions,
        DreamSubstrateFragmentInput,
    },
    scope::DreamScope,
    types::{ActiveMemory, DreamError, SubstrateFragment},
};

/// Prompt-size guardrail for Stream F §5.1.1 substrate-fragment windows; newest
/// fragments win after the spec-defined date/scope filter.
const MAX_DREAM_SUBSTRATE_FRAGMENTS: usize = 1_000;
/// Prompt-size guardrail for Stream F §5.1.2 active-memory context.
const MAX_DREAM_ACTIVE_MEMORIES: usize = 256;
/// Prompt-size guardrail for Stream F §6.4 previous-question dedupe context.
const MAX_PREVIOUS_QUESTIONS: usize = 64;
pub struct DreamRunBuild {
    pub options: DreamRunOptions,
    pub writer: SubstrateCandidateWriter,
}

pub struct DreamRunBuildRequest {
    pub scope: DreamScope,
    pub run_id: String,
    pub run_date: NaiveDate,
    pub prompt_version: PromptVersion,
    pub notifications: Option<broadcast::Sender<NotificationEvent>>,
    pub pass_timeout: Duration,
    pub pass_2_max_candidates: usize,
    pub pass_1_window_days: u32,
}

pub async fn build_dream_run(
    substrate: &Substrate,
    request: DreamRunBuildRequest,
) -> Result<DreamRunBuild, DreamError> {
    #[cfg(any(test, feature = "dev-fixtures"))]
    let placeholder = Arc::new(EchoCli::default());
    #[cfg(not(any(test, feature = "dev-fixtures")))]
    let placeholder = Arc::new(UnselectedHarness);
    let substrate_fragments = load_substrate_fragments(
        substrate.roots().repo.as_path(),
        &request.scope,
        request.run_date,
        request.pass_1_window_days,
    )?;
    let active_memories = load_active_memories(substrate, &request.scope).await?;
    let previous_questions =
        load_previous_questions(substrate.roots().repo.as_path(), &request.scope, request.run_date)?;
    let options = DreamRunOptions {
        repo_root: substrate.roots().repo.clone(),
        scope: request.scope,
        run_date: request.run_date,
        run_id: request.run_id,
        prompt_version: request.prompt_version,
        notifications: request.notifications,
        harness: placeholder,
        pass_timeout: request.pass_timeout,
        pass_2_max_candidates: request.pass_2_max_candidates,
        substrate_fragments,
        active_memories,
        previous_questions,
    };
    Ok(DreamRunBuild { options, writer: SubstrateCandidateWriter::new(substrate.clone()) })
}

pub async fn select_harness(
    cli_override: Option<&str>,
    priority: &[String],
    _options: &DreamRunOptions,
) -> Result<Arc<dyn HarnessCli>, DreamError> {
    #[cfg(any(test, feature = "dev-fixtures"))]
    if cli_override == Some("echo") && echo_cli_override_enabled() {
        return deterministic_echo_harness(_options);
    }

    let registry = HarnessCliRegistry::builtin_v0_2();
    if let Some(name) = cli_override {
        if registry.disabled_adapters().any(|adapter| adapter.name == name) {
            return Err(DreamError::unavailable(format!("harness CLI `{name}` is disabled in Stream F v0.2")));
        }
        let adapter = registry.get(name).ok_or_else(|| DreamError::unknown_harness_override(name))?;
        if !adapter.is_installed() {
            return Err(DreamError::unavailable(format!("harness CLI `{name}` is not installed")));
        }
        let probe = adapter.auth_probe().await;
        match &probe {
            super::harness::AuthProbeResult::Ok => return Ok(adapter),
            super::harness::AuthProbeResult::CliMissing { .. } => {
                return Err(DreamError::unavailable(format!("harness CLI `{name}` is not installed")));
            }
            super::harness::AuthProbeResult::AuthFailed { .. } => {
                return Err(DreamError::unavailable(format!(
                    "harness CLI `{name}` is not authenticated: {}",
                    probe.operator_message(adapter.name())
                )));
            }
            super::harness::AuthProbeResult::Timeout | super::harness::AuthProbeResult::Error { .. } => {
                return Err(DreamError::unavailable(format!(
                    "harness CLI `{name}` auth probe failed: {}",
                    probe.operator_message(adapter.name())
                )));
            }
        }
    }

    registry
        .select_first_available(priority)
        .await
        .ok_or_else(|| DreamError::unavailable("no eligible harness CLI installed and authenticated"))
}

#[cfg(any(test, feature = "dev-fixtures"))]
pub(crate) fn echo_cli_override_enabled() -> bool {
    cfg!(test)
        || (cfg!(feature = "dev-fixtures")
            && std::env::var_os("MEMORYD_ENABLE_ECHO_DREAM_HARNESS").as_deref() == Some(std::ffi::OsStr::new("1")))
}

#[cfg(not(any(test, feature = "dev-fixtures")))]
pub(crate) fn echo_cli_override_enabled() -> bool {
    false
}

#[cfg(not(any(test, feature = "dev-fixtures")))]
struct UnselectedHarness;

#[cfg(not(any(test, feature = "dev-fixtures")))]
impl HarnessCli for UnselectedHarness {
    fn name(&self) -> &'static str {
        "unselected"
    }

    fn prompt_transport(&self) -> crate::protocol::PromptTransport {
        crate::protocol::PromptTransport::Stdin
    }

    fn is_installed(&self) -> bool {
        false
    }

    fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult> {
        Box::pin(async { AuthProbeResult::Error { message: "harness not selected".to_owned() } })
    }

    fn is_authenticated(&self) -> HarnessFuture<'_, Result<bool, HarnessCliError>> {
        Box::pin(async { Ok(false) })
    }

    fn complete<'a>(
        &'a self,
        _prompt: &'a str,
        _expect_json: bool,
        _timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
        Box::pin(async {
            Err(HarnessCliError::SubprocessExit { code: Some(1), stderr_tail: "harness not selected".to_owned() })
        })
    }
}

#[derive(Clone)]
pub struct SubstrateCandidateWriter {
    substrate: Substrate,
}

impl SubstrateCandidateWriter {
    fn new(substrate: Substrate) -> Self {
        Self { substrate }
    }
}

impl CandidateWriter for SubstrateCandidateWriter {
    fn write_candidate<'a>(&'a self, request: CandidateWriteRequest) -> HarnessFuture<'a, CandidateWriteResult> {
        Box::pin(async move { self.write_candidate_inner(request).await })
    }
}

impl SubstrateCandidateWriter {
    async fn write_candidate_inner(&self, request: CandidateWriteRequest) -> CandidateWriteResult {
        match self.write_candidate_memory(request).await {
            Ok(result) => result,
            Err(reason) => {
                CandidateWriteResult { id: None, accepted: false, reason: Some(reason), source_ref_count: 0 }
            }
        }
    }

    async fn write_candidate_memory(&self, request: CandidateWriteRequest) -> Result<CandidateWriteResult, String> {
        match candidate_storage_action(&request)? {
            PrivacyStorageAction::Plaintext => {}
            // Dreaming-strict intentionally does not create encrypted canonical
            // candidates: dream passes never reveal/decrypt, and encrypt-at-rest
            // candidate review requires a user-visible product contract.
            PrivacyStorageAction::EncryptAtRest => {
                return Ok(CandidateWriteResult {
                    id: None,
                    accepted: false,
                    reason: Some("privacy_required_encryption".to_string()),
                    source_ref_count: request.evidence.len(),
                });
            }
            PrivacyStorageAction::Refuse => {
                return Ok(CandidateWriteResult {
                    id: None,
                    accepted: false,
                    reason: Some("unsafe_candidate".to_string()),
                    source_ref_count: request.evidence.len(),
                });
            }
        }

        let id = self.substrate.next_memory_id().await.map_err(|err| err.to_string())?;
        let source_ref_count = request.evidence.len();
        let memory = candidate_memory(id, &request)?;
        let id = memory.frontmatter.id.as_str().to_string();
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext {
                    actor: Some("memoryd-dreaming".to_string()),
                    reason: Some("dreaming-strict candidate write".to_string()),
                },
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .map_err(|err| err.kind.to_string())?;

        Ok(CandidateWriteResult { id: Some(id), accepted: true, reason: None, source_ref_count })
    }
}

fn candidate_memory(id: memory_substrate::MemoryId, request: &CandidateWriteRequest) -> Result<Memory, String> {
    let now = Utc::now();
    let memory_type = candidate_memory_type(&request.kind)?;
    let scope = substrate_scope(&request.namespace)?;
    let canonical_namespace_id = canonical_namespace_id(&request.namespace);
    let path = candidate_path(&request.namespace, &memory_type, id.as_str())?;
    let mut frontmatter = Frontmatter {
        schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: id.clone(),
        memory_type,
        scope,
        summary: bounded_summary(&request.claim),
        confidence: request.confidence,
        original_confidence: None,
        trust_level: TrustLevel::Candidate,
        sensitivity: Sensitivity::Internal,
        status: MemoryStatus::Candidate,
        created_at: now,
        updated_at: now,
        observed_at: None,
        author: Author {
            kind: AuthorKind::Dreaming,
            user_handle: None,
            harness: Some("memoryd".to_string()),
            harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            session_id: None,
            subagent_id: None,
            phase: Some("pass_2".to_string()),
            component: Some("stream-f".to_string()),
        },
        namespace: canonical_namespace_id.clone(),
        canonical_namespace_id,
        tags: vec!["dreaming".to_string(), request.kind.clone()],
        entities: Vec::<Entity>::new(),
        aliases: Vec::new(),
        source: Source {
            kind: SourceKind::Synthesis,
            reference: request.evidence.first().map(|evidence| evidence.reference.clone()),
            harness: Some("memoryd".to_string()),
            harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            session_id: None,
            subagent_id: None,
            device: None,
        },
        evidence: evidence_entries(&request.evidence),
        requires_user_confirmation: true,
        review_state: Some("candidate".to_string()),
        supersedes: Vec::new(),
        superseded_by: Vec::new(),
        related: Vec::new(),
        tombstone_events: Vec::new(),
        retrieval_policy: RetrievalPolicy {
            passive_recall: true,
            max_scope: scope,
            mask_personal_for_synthesis: true,
            index_body: true,
            index_embeddings: true,
        },
        write_policy: WritePolicy {
            human_review_required: true,
            policy_applied: request.policy.clone(),
            expected_base_hash: None,
        },
        merge_diagnostics: None,
        extras: BTreeMap::new(),
    };
    frontmatter.set_grounding_rehydration_required(request.grounding_rehydration_required);

    Ok(Memory { frontmatter, body: request.claim.clone(), path: Some(path) })
}

fn candidate_memory_type(kind: &str) -> Result<MemoryType, String> {
    match kind {
        "claim" => Ok(MemoryType::Claim),
        "decision" => Ok(MemoryType::Decision),
        "pattern" => Ok(MemoryType::Pattern),
        "playbook" => Ok(MemoryType::Playbook),
        "procedure" => Ok(MemoryType::Procedure),
        "artifact" | "project" => Ok(MemoryType::Artifact),
        other => Err(format!("unsupported dream candidate kind `{other}`")),
    }
}

fn substrate_scope(namespace: &str) -> Result<Scope, String> {
    match DreamScope::parse(namespace).map_err(|err| err.to_string())? {
        DreamScope::Me => Ok(Scope::User),
        DreamScope::Agent => Ok(Scope::Agent),
        DreamScope::Project(_) => Ok(Scope::Project),
        DreamScope::Org(_) => Ok(Scope::Org),
    }
}

fn canonical_namespace_id(namespace: &str) -> Option<String> {
    namespace.strip_prefix("project:").or_else(|| namespace.strip_prefix("org:")).map(str::to_string)
}

fn candidate_path(namespace: &str, memory_type: &MemoryType, id: &str) -> Result<RepoPath, String> {
    let kind_dir = match memory_type {
        MemoryType::Decision => "decisions",
        MemoryType::Claim => "claims",
        MemoryType::Playbook => "playbooks",
        MemoryType::Procedure => "procedures",
        MemoryType::Artifact => "artifacts",
        _ => "patterns",
    };
    let path = match DreamScope::parse(namespace).map_err(|err| err.to_string())? {
        DreamScope::Me => format!("me/knowledge/{id}.md"),
        DreamScope::Agent => format!("agent/{kind_dir}/{id}.md"),
        DreamScope::Project(project_id) => format!("projects/{project_id}/{kind_dir}/{id}.md"),
        DreamScope::Org(org_id) => format!("projects/{org_id}/{kind_dir}/{id}.md"),
    };
    RepoPath::try_new(path)
}

fn evidence_entries(evidence: &[CandidateEvidenceRef]) -> Vec<Evidence> {
    evidence
        .iter()
        .enumerate()
        .map(|(index, evidence)| Evidence {
            id: format!("ev_{}_{index:02}", short_hash(&evidence.reference)),
            quote: evidence.excerpt.clone().unwrap_or_default(),
            quote_norm_hash: evidence.excerpt.as_ref().map(|quote| format!("sha256:{}", short_hash(quote))),
            reference: evidence.reference.clone(),
            weight: 1.0,
            observed_at: Some(Utc::now()),
            source: Some(evidence.kind.clone()),
        })
        .collect()
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    hex::encode(&digest[..8])
}

fn candidate_storage_action(request: &CandidateWriteRequest) -> Result<PrivacyStorageAction, String> {
    let mut text = String::new();
    text.push_str(&request.claim);
    text.push('\n');
    text.push_str(&request.rationale);
    for evidence in &request.evidence {
        text.push('\n');
        text.push_str(&evidence.reference);
        if let Some(excerpt) = evidence.excerpt.as_ref() {
            text.push('\n');
            text.push_str(excerpt);
        }
    }
    let namespace = candidate_privacy_namespace(&request.namespace)?;
    DeterministicPrivacyClassifier::new()
        .classify(&text, namespace, None)
        .map(|decision| decision.storage_action)
        .map_err(|err| err.to_string())
}

fn candidate_privacy_namespace(namespace: &str) -> Result<PrivacyNamespace, String> {
    match DreamScope::parse(namespace).map_err(|err| err.to_string())? {
        DreamScope::Me => Ok(PrivacyNamespace::Me),
        DreamScope::Agent => Ok(PrivacyNamespace::Agent),
        DreamScope::Project(_) | DreamScope::Org(_) => Ok(PrivacyNamespace::Project),
    }
}

fn bounded_summary(text: &str) -> String {
    const MAX: usize = 120;
    let trimmed = text.trim();
    if trimmed.len() <= MAX {
        return trimmed.to_string();
    }
    let prefix = trimmed
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= MAX.saturating_sub(3))
        .last()
        .unwrap_or(0);
    format!("{}...", &trimmed[..prefix])
}

async fn load_active_memories(
    substrate: &Substrate,
    scope: &DreamScope,
) -> Result<Vec<DreamActiveMemoryInput>, DreamError> {
    let mut rows = Vec::new();
    for status in [MemoryStatus::Pinned, MemoryStatus::Active] {
        rows.extend(
            substrate
                .query_recall_index(RecallIndexQuery {
                    namespace_prefix: Some(scope.as_str()),
                    statuses: vec![status],
                    passive_recall_only: true,
                    updated_since: None,
                    match_terms: Vec::new(),
                    // Dream input reads `row.entities` (plus scalar id/summary);
                    // tags and aliases are never read, so hydrate entities only.
                    hydrate: AuxScope::Entities,
                    source_identity: false,
                })
                .await
                .map_err(|err| DreamError::invalid_request(format!("failed to load active dream memories: {err}")))?,
        );
    }
    rows.sort_by(|left, right| right.updated_at.cmp(&left.updated_at).then_with(|| left.id.cmp(&right.id)));
    rows.truncate(MAX_DREAM_ACTIVE_MEMORIES);

    Ok(rows
        .into_iter()
        .map(|row| DreamActiveMemoryInput {
            memory: ActiveMemory {
                id: row.id.as_str().to_string(),
                namespace: scope.as_str(),
                kind: "memory".to_string(),
                entities: row.entities.into_iter().map(|entity| entity.id).collect(),
                summary: row.summary,
            },
            summary_spans: Vec::new(),
        })
        .collect())
}

fn load_substrate_fragments(
    repo: &Path,
    scope: &DreamScope,
    run_date: NaiveDate,
    window_days: u32,
) -> Result<Vec<DreamSubstrateFragmentInput>, DreamError> {
    let cutoff = run_date.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc()
        - chrono::Duration::days(i64::from(window_days));
    let mut fragments = Vec::new();
    collect_plaintext_fragments(repo, scope, cutoff, &mut fragments)?;
    collect_encrypted_descriptors(repo, scope, cutoff, &mut fragments)?;
    fragments.sort_by(|left, right| {
        left.fragment.ts.cmp(&right.fragment.ts).then_with(|| left.fragment.id.cmp(&right.fragment.id))
    });
    fragments.truncate(MAX_DREAM_SUBSTRATE_FRAGMENTS);
    Ok(fragments)
}

fn collect_plaintext_fragments(
    repo: &Path,
    scope: &DreamScope,
    cutoff: DateTime<Utc>,
    output: &mut Vec<DreamSubstrateFragmentInput>,
) -> Result<(), DreamError> {
    for value in read_jsonl_values(repo.join("substrate"))? {
        let Some((record, spans)) = parse_plaintext_fragment(&value, scope, cutoff) else {
            continue;
        };
        output.push(DreamSubstrateFragmentInput { fragment: record, text_spans: spans });
    }
    Ok(())
}

fn collect_encrypted_descriptors(
    repo: &Path,
    scope: &DreamScope,
    cutoff: DateTime<Utc>,
    output: &mut Vec<DreamSubstrateFragmentInput>,
) -> Result<(), DreamError> {
    for value in read_jsonl_values(repo.join("encrypted/substrate"))? {
        let Some(record) = parse_encrypted_descriptor(&value, scope, cutoff) else {
            continue;
        };
        output.push(DreamSubstrateFragmentInput { fragment: record, text_spans: Vec::new() });
    }
    Ok(())
}

fn parse_plaintext_fragment(
    value: &Value,
    scope: &DreamScope,
    cutoff: DateTime<Utc>,
) -> Option<(SubstrateFragment, Vec<PrivacySpan>)> {
    let ts = parse_ts(value)?;
    if ts < cutoff || value.get("scope").and_then(Value::as_str)? != scope.as_str() {
        return None;
    }
    let text = value.get("text")?.as_str()?.to_string();
    let spans = privacy_spans(value.get("privacy_spans"), text.len());
    Some((
        SubstrateFragment {
            id: value.get("id")?.as_str()?.to_string(),
            kind: value.get("kind").and_then(Value::as_str).unwrap_or("observation").to_string(),
            ts: ts.to_rfc3339(),
            entities: string_array(value.get("entities")),
            text,
        },
        spans,
    ))
}

fn parse_encrypted_descriptor(value: &Value, scope: &DreamScope, cutoff: DateTime<Utc>) -> Option<SubstrateFragment> {
    let ts = parse_ts(value)?;
    if ts < cutoff || value.get("scope").and_then(Value::as_str)? != scope.as_str() {
        return None;
    }
    let text = value.get("descriptor")?.get("summary_safe")?.as_str()?.to_string();
    Some(SubstrateFragment {
        id: value.get("id")?.as_str()?.to_string(),
        kind: value.get("kind").and_then(Value::as_str).unwrap_or("observation").to_string(),
        ts: ts.to_rfc3339(),
        entities: string_array(value.get("entities")),
        text,
    })
}

fn parse_ts(value: &Value) -> Option<DateTime<Utc>> {
    value.get("ts")?.as_str()?.parse::<DateTime<Utc>>().ok()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

fn privacy_spans(value: Option<&Value>, text_len: usize) -> Vec<PrivacySpan> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let label = value.get("label").and_then(Value::as_str).and_then(privacy_label)?;
            let start = value.get("start").and_then(Value::as_u64)? as usize;
            let end = value.get("end").and_then(Value::as_u64)? as usize;
            if start >= end || end > text_len {
                return None;
            }
            Some(PrivacySpan::new(label, start, end, 1.0))
        })
        .collect()
}

fn privacy_label(value: &str) -> Option<PrivacyLabel> {
    serde_json::from_value(Value::String(value.to_string())).ok()
}

fn read_jsonl_values(root: PathBuf) -> Result<Vec<Value>, DreamError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for path in jsonl_files(&root)? {
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let text = fs::read_to_string(path)
            .map_err(|err| DreamError::invalid_request(format!("failed to read substrate fragment file: {err}")))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                values.push(value);
            }
        }
    }
    Ok(values)
}

fn jsonl_files(root: &Path) -> Result<Vec<PathBuf>, DreamError> {
    let mut files = Vec::new();
    collect_jsonl_files(root, &mut files)?;
    Ok(files)
}

fn collect_jsonl_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), DreamError> {
    for entry in fs::read_dir(path)
        .map_err(|err| DreamError::invalid_request(format!("failed to list substrate fragment directory: {err}")))?
    {
        let entry = entry.map_err(|err| {
            DreamError::invalid_request(format!("failed to read substrate fragment directory: {err}"))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn load_previous_questions(repo: &Path, scope: &DreamScope, run_date: NaiveDate) -> Result<Vec<String>, DreamError> {
    let current_path = scope.questions_path(run_date);
    let Some(dir) = repo.join(&current_path).parent().map(Path::to_path_buf) else {
        return Ok(Vec::new());
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut dated_files = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|err| DreamError::invalid_request(format!("failed to list previous questions: {err}")))?
    {
        let entry = entry
            .map_err(|err| DreamError::invalid_request(format!("failed to read previous question entry: {err}")))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
            continue;
        };
        if date < run_date {
            dated_files.push((date, path));
        }
    }
    dated_files.sort_by(|left, right| right.0.cmp(&left.0));

    let mut questions = Vec::new();
    for (_date, path) in dated_files {
        let text = fs::read_to_string(path)
            .map_err(|err| DreamError::invalid_request(format!("failed to read previous questions: {err}")))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(record) = serde_json::from_str::<QuestionRecord>(line) else {
                continue;
            };
            questions.push(record.question);
            if questions.len() >= MAX_PREVIOUS_QUESTIONS {
                return Ok(questions);
            }
        }
    }
    Ok(questions)
}

#[derive(Deserialize)]
struct QuestionRecord {
    question: String,
}
