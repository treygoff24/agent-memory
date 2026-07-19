//! Device-local W3 merge proposals and journaled apply.

use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
pub use memory_governance::MergeProposalStatus;
use memory_governance::{
    validate_merge_candidates, MergeCandidate, MergeCandidateExclusions, DEFAULT_MERGE_SIMILARITY_THRESHOLD,
};
use memory_privacy::{PrivacyNamespace, PrivacyStorageAction};
use memory_substrate::events::{EventKind, MergeAppliedSource};
use memory_substrate::{
    ClassificationOutcome, EventContext, Memory, MemoryContent, MemoryId, MemoryStatus, Sensitivity, Sha256, Substrate,
    TombstoneRequest, TrustLevel, WriteMode, WriteRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256 as Sha256Hasher};

const STAGING_POLICY: &str = "merge-staged-v1";
const MAX_RECONCILE_ATTEMPTS: u32 = 3;
static MERGE_APPLY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimilarityEvidence {
    pub left: MemoryId,
    pub right: MemoryId,
    pub cosine: f32,
    pub lane: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapturedSource {
    pub id: MemoryId,
    pub expected_base_hash: Sha256,
    pub original_status: MemoryStatus,
    pub original_trust_level: TrustLevel,
    pub original_sensitivity: Sensitivity,
    #[serde(default)]
    pub superseded_hash: Option<Sha256>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MergeProposal {
    pub proposal_id: String,
    pub source_ids: Vec<MemoryId>,
    pub replacement: Memory,
    pub provenance_overridden: bool,
    pub similarity_evidence: Vec<SimilarityEvidence>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub status: MergeProposalStatus,
    #[serde(default)]
    pub captured_sources: Vec<CapturedSource>,
    /// Failed reconcile attempts for this applying proposal. Retried at startup
    /// and dream entry; after 3 attempts we quarantine to avoid an infinite loop.
    #[serde(default)]
    pub reconcile_attempts: u32,
}

impl MergeProposal {
    pub fn new(
        source_ids: Vec<MemoryId>,
        replacement: Memory,
        similarity_evidence: Vec<SimilarityEvidence>,
        created_by: impl Into<String>,
    ) -> anyhow::Result<Self> {
        if source_ids.is_empty() {
            anyhow::bail!("merge proposal requires at least one source");
        }
        let unique = source_ids.iter().collect::<BTreeSet<_>>();
        if unique.len() != source_ids.len() {
            anyhow::bail!("merge proposal source ids must be unique");
        }
        if source_ids.contains(&replacement.frontmatter.id) {
            anyhow::bail!("merge replacement must use a new id");
        }
        Ok(Self {
            proposal_id: ulid::Ulid::new().to_string(),
            source_ids,
            replacement,
            provenance_overridden: false,
            similarity_evidence,
            created_by: created_by.into(),
            created_at: Utc::now(),
            status: MergeProposalStatus::Proposed,
            captured_sources: Vec::new(),
            reconcile_attempts: 0,
        })
    }
}

#[derive(Clone, Debug)]
pub struct VectorCandidate {
    pub memory: Memory,
    pub vector: Vec<f32>,
    pub encrypted: bool,
    pub claim_locked: bool,
}

#[derive(Clone, Debug)]
pub struct MergeCandidateConfig {
    pub cosine_threshold: f32,
    pub proposal_cap: usize,
}

impl Default for MergeCandidateConfig {
    fn default() -> Self {
        Self { cosine_threshold: DEFAULT_MERGE_SIMILARITY_THRESHOLD, proposal_cap: 10 }
    }
}

/// Dark dream-job core: pairwise abstraction-vector proposals after all fences.
pub fn near_duplicate_pairs(
    candidates: &[VectorCandidate],
    exclusions: &MergeCandidateExclusions,
    config: &MergeCandidateConfig,
) -> Vec<SimilarityEvidence> {
    let mut pairs = Vec::new();
    for (left_index, left) in candidates.iter().enumerate() {
        for right in &candidates[left_index + 1..] {
            let projected = [project_candidate(left), project_candidate(right)];
            if validate_merge_candidates(&projected, exclusions).is_err() {
                continue;
            }
            let Some(cosine) = cosine(&left.vector, &right.vector) else { continue };
            if cosine >= config.cosine_threshold {
                pairs.push(SimilarityEvidence {
                    left: left.memory.frontmatter.id.clone(),
                    right: right.memory.frontmatter.id.clone(),
                    cosine,
                    lane: "abstraction".to_string(),
                });
            }
        }
    }
    pairs.sort_by(|a, b| {
        b.cosine.total_cmp(&a.cosine).then_with(|| a.left.cmp(&b.left)).then_with(|| a.right.cmp(&b.right))
    });
    pairs.truncate(config.proposal_cap);
    pairs
}

/// Registered dark job: materialize review proposals from current W2 vectors.
/// No scheduler calls this until live backfill/eval tuning is complete.
pub struct GenerateDarkProposals<'a> {
    pub claim_locked: &'a BTreeSet<MemoryId>,
    pub import_repair_lineage: BTreeSet<String>,
    pub backfill_manifest: BTreeSet<String>,
    pub config: MergeCandidateConfig,
    pub dream_run_id: &'a str,
}

pub async fn generate_dark_proposals(
    substrate: &Substrate,
    request: GenerateDarkProposals<'_>,
) -> anyhow::Result<Vec<MergeProposal>> {
    let failures = reconcile_applying(substrate, None).await;
    if !failures.is_empty() {
        anyhow::bail!("merge reconciliation failed before candidate generation: {failures:?}");
    }
    let store = MergeProposalStore::new(&substrate.roots().runtime);
    let exclusions = MergeCandidateExclusions {
        nonterminal_proposal_sources: store.nonterminal_source_ids(None)?,
        import_repair_lineage: request.import_repair_lineage,
        backfill_manifest: request.backfill_manifest,
    };
    let vectors = substrate.all_abstraction_vectors(&substrate.active_embedding_triple()?)?;
    let mut candidates = Vec::new();
    for row in vectors {
        let envelope = substrate.read_memory_envelope(&row.memory_id).await?;
        candidates.push(VectorCandidate {
            encrypted: !matches!(envelope.content, MemoryContent::Plaintext(_)),
            claim_locked: request.claim_locked.contains(&row.memory_id),
            memory: envelope.metadata,
            vector: row.vector,
        });
    }
    let pairs = near_duplicate_pairs(&candidates, &exclusions, &request.config);
    let by_id = candidates
        .into_iter()
        .map(|candidate| (candidate.memory.frontmatter.id.clone(), candidate.memory))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut used = BTreeSet::new();
    let mut proposals = Vec::new();
    for evidence in pairs {
        if used.contains(&evidence.left) || used.contains(&evidence.right) {
            continue;
        }
        let left = by_id.get(&evidence.left).expect("pair ids originate from candidates");
        let right = by_id.get(&evidence.right).expect("pair ids originate from candidates");
        let mut replacement = if right.body.len() > left.body.len() { right.clone() } else { left.clone() };
        let replacement_id = substrate.next_memory_id().await?;
        replacement.frontmatter.id = replacement_id.clone();
        replacement.frontmatter.created_at = Utc::now();
        replacement.frontmatter.updated_at = replacement.frontmatter.created_at;
        replacement.path = Some(replacement_path(&replacement, &replacement_id)?);
        let proposal = MergeProposal::new(
            vec![evidence.left.clone(), evidence.right.clone()],
            replacement,
            vec![evidence],
            request.dream_run_id,
        )?;
        store.create(&proposal)?;
        used.extend(proposal.source_ids.iter().cloned());
        proposals.push(proposal);
    }
    Ok(proposals)
}

fn replacement_path(memory: &Memory, id: &MemoryId) -> anyhow::Result<memory_substrate::RepoPath> {
    let old = memory.path.as_ref().ok_or_else(|| anyhow::anyhow!("candidate has no canonical path"))?;
    let parent = Path::new(old.as_str()).parent().ok_or_else(|| anyhow::anyhow!("candidate path has no parent"))?;
    Ok(memory_substrate::RepoPath::new(parent.join(format!("{id}.md")).to_string_lossy().replace('\\', "/")))
}

fn project_candidate(candidate: &VectorCandidate) -> MergeCandidate {
    let fm = &candidate.memory.frontmatter;
    MergeCandidate {
        id: fm.id.to_string(),
        status: fm.status.as_db_str().to_string(),
        trust_level: fm.trust_level.as_db_str().to_string(),
        review_state: fm.review_state.clone(),
        requires_user_confirmation: fm.requires_user_confirmation,
        encrypted: candidate.encrypted,
        passive_recall: fm.retrieval_policy.passive_recall,
        scope: fm.scope.as_db_str().to_string(),
        canonical_namespace: fm.canonical_namespace_id.clone(),
        memory_type: fm.memory_type.as_db_str().to_string(),
        sensitivity: fm.sensitivity.as_db_str().to_string(),
        claim_locked: candidate.claim_locked,
    }
}

fn cosine(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>();
    let left_norm = left.iter().map(|v| v * v).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|v| v * v).sum::<f32>().sqrt();
    (left_norm > 0.0 && right_norm > 0.0).then_some(dot / (left_norm * right_norm))
}

#[derive(Clone, Debug)]
pub struct MergeProposalStore {
    root: PathBuf,
}

impl MergeProposalStore {
    pub fn new(runtime: &Path) -> Self {
        Self { root: runtime.join("governance/merge-proposals") }
    }

    pub fn create(&self, proposal: &MergeProposal) -> anyhow::Result<()> {
        let path = self.proposal_path(&proposal.proposal_id);
        if path.exists() {
            anyhow::bail!("merge proposal already exists: {}", proposal.proposal_id);
        }
        fs::create_dir_all(path.parent().expect("proposal path has parent"))?;
        self.save(proposal)
    }

    pub fn load(&self, proposal_id: &str) -> anyhow::Result<MergeProposal> {
        Ok(serde_json::from_slice(&fs::read(self.proposal_path(proposal_id))?)?)
    }

    pub fn list(&self) -> anyhow::Result<Vec<MergeProposal>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut proposals = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path().join("proposal.json");
            if path.is_file() {
                proposals.push(serde_json::from_slice(&fs::read(path)?)?);
            }
        }
        proposals.sort_by_key(|proposal: &MergeProposal| proposal.created_at);
        Ok(proposals)
    }

    pub fn nonterminal_source_ids(&self, excluding: Option<&str>) -> anyhow::Result<BTreeSet<String>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|proposal| Some(proposal.proposal_id.as_str()) != excluding && !proposal.status.is_terminal())
            .flat_map(|proposal| proposal.source_ids.into_iter().map(|id| id.to_string()))
            .collect())
    }

    /// Return the journal file's `modified` time, if a journal exists.
    pub fn journal_mtime(&self, proposal_id: &str) -> anyhow::Result<Option<std::time::SystemTime>> {
        let path = self.journal_path(proposal_id);
        if !path.exists() {
            return Ok(None);
        }
        let metadata = std::fs::metadata(&path)?;
        Ok(Some(metadata.modified().unwrap_or_else(|_| metadata.created().unwrap_or(std::time::UNIX_EPOCH))))
    }

    pub fn save(&self, proposal: &MergeProposal) -> anyhow::Result<()> {
        let path = self.proposal_path(&proposal.proposal_id);
        let parent = path.parent().expect("proposal path has parent").to_path_buf();
        fs::create_dir_all(&parent)?;
        let temp = parent.join("proposal.json.tmp");
        let bytes = serde_json::to_vec_pretty(proposal)?;
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }

    fn proposal_path(&self, proposal_id: &str) -> PathBuf {
        self.root.join(proposal_id).join("proposal.json")
    }

    fn journal_path(&self, proposal_id: &str) -> PathBuf {
        self.root.join(proposal_id).join("journal.jsonl")
    }
}

/// Reject a merge proposal. For `Proposed` proposals this is a simple status
/// flip to `Rejected`. For `Quarantined` or stuck `Applying` proposals this
/// performs a safe rollback: source status/trust/superseded_by fields are
/// restored (the current body is preserved verbatim), the staged replacement is
/// tombstoned if still non-servable, and the proposal is marked `RolledBack`
/// with a `MergeRolledBack` event.
///
/// If the replacement is already servable (Active/Pinned with a non-staging
/// policy), the merge has factually applied: we do not restore sources, but
/// complete the proposal bookkeeping idempotently and report a typed
/// "already applied" outcome.
pub async fn reject_proposal(
    substrate: &Substrate,
    store: &MergeProposalStore,
    proposal_id: &str,
) -> Result<MergeProposal, MergeApplyError> {
    let mut proposal = store.load(proposal_id).map_err(MergeApplyError::Other)?;
    match proposal.status {
        MergeProposalStatus::Proposed => {
            proposal.status = MergeProposalStatus::Rejected;
            store.save(&proposal).map_err(MergeApplyError::Other)?;
        }
        MergeProposalStatus::Quarantined | MergeProposalStatus::Applying => {
            if is_replacement_servable(substrate, &proposal).await? {
                proposal.status = MergeProposalStatus::Applied;
                emit_merge_applied_once(substrate, &proposal)?;
                append_journal(store, &proposal.proposal_id, "done", Value::Null)?;
                store.save(&proposal).map_err(MergeApplyError::Other)?;
                return Err(MergeApplyError::AlreadyApplied { proposal_id: proposal.proposal_id.clone() });
            }
            safe_restore_from_reject(substrate, &mut proposal).await?;
            proposal.status = MergeProposalStatus::RolledBack;
            store.save(&proposal).map_err(MergeApplyError::Other)?;
        }
        MergeProposalStatus::Applied => {
            return Err(MergeApplyError::AlreadyApplied { proposal_id: proposal.proposal_id.clone() });
        }
        _ => return Err(MergeApplyError::Other(anyhow::anyhow!("proposal cannot be rejected: {:?}", proposal.status))),
    }
    Ok(proposal)
}

async fn safe_restore_from_reject(substrate: &Substrate, proposal: &mut MergeProposal) -> Result<(), MergeApplyError> {
    let replacement_id = proposal.replacement.frontmatter.id.clone();
    let mut restored = Vec::new();
    for captured in &proposal.captured_sources {
        let mut source = match substrate.read_memory(&captured.id).await {
            Ok(source) => source,
            Err(_) => continue,
        };
        if source.frontmatter.status != MemoryStatus::Superseded
            || source.frontmatter.superseded_by != [replacement_id.clone()]
        {
            continue;
        }
        let current_hash = memory_hash(substrate, &source).map_err(MergeApplyError::Other)?;
        source.frontmatter.status = captured.original_status;
        source.frontmatter.trust_level = captured.original_trust_level;
        source.frontmatter.superseded_by.clear();
        source.frontmatter.updated_at = Utc::now();
        let classification = crate::handlers::governance::classify_plaintext_memory(&source)
            .map_err(|error| MergeApplyError::Other(anyhow::anyhow!(error.message)))?;
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: source,
                expected_base_hash: Some(current_hash),
                write_mode: WriteMode::ReplaceExisting,
                index_projection: None,
                event_context: EventContext {
                    actor: Some("memoryd-merge-reject".into()),
                    reason: Some(proposal.proposal_id.clone()),
                },
                allow_best_effort_durability: true,
                classification,
            })
            .await
            .map_err(|error| MergeApplyError::Other(anyhow::anyhow!(error.to_string())))?;
        restored.push(captured.id.clone());
    }

    if let Ok(replacement) = substrate.read_memory(&replacement_id).await {
        if replacement.frontmatter.status != MemoryStatus::Tombstoned && replacement.frontmatter.is_merge_non_servable()
        {
            substrate
                .tombstone_memory(TombstoneRequest { id: replacement_id, reason: "merge-reject".to_string() })
                .await
                .map_err(|error| MergeApplyError::Other(anyhow::anyhow!(error.to_string())))?;
        }
    }

    if !event_exists(substrate, &proposal.proposal_id, false).map_err(MergeApplyError::Other)? {
        substrate
            .record_event_best_effort(EventKind::MergeRolledBack {
                proposal_id: proposal.proposal_id.clone(),
                replacement_id: proposal.replacement.frontmatter.id.clone(),
                restored_source_ids: restored,
            })
            .map_err(|error| MergeApplyError::Other(error.into()))?;
    }
    Ok(())
}

/// Servable replacement after a merge has factually applied: Active or Pinned
/// with a non-staging policy. The replacement may be read only once; I/O errors
/// are propagated because we cannot determine whether the merge has applied.
async fn is_replacement_servable(substrate: &Substrate, proposal: &MergeProposal) -> Result<bool, MergeApplyError> {
    let replacement = match substrate.read_memory(&proposal.replacement.frontmatter.id).await {
        Ok(replacement) => replacement,
        Err(memory_substrate::ReadError::NotFound(_)) => return Ok(false),
        Err(error) => return Err(MergeApplyError::Other(anyhow::anyhow!(error))),
    };
    Ok((replacement.frontmatter.status == MemoryStatus::Active
        || replacement.frontmatter.status == MemoryStatus::Pinned)
        && replacement.frontmatter.write_policy.policy_applied != STAGING_POLICY)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct JournalRecord {
    phase: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    data: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct JournalFrame {
    seq: u64,
    proposal_id: String,
    record: JournalRecord,
    record_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimulatedCrash {
    AfterStage,
    AfterSupersede(usize),
    BeforeActivation,
    AfterActivation,
}

#[derive(Debug, thiserror::Error)]
pub enum MergeApplyError {
    #[error("proposal invalidated: {0}")]
    Invalidated(String),
    #[error("simulated merge crash at {0:?}")]
    SimulatedCrash(SimulatedCrash),
    #[error("proposal quarantined: {0}")]
    Quarantined(String),
    #[error("retryable merge activation error: {0}")]
    Retryable(String),
    #[error("merge proposal {proposal_id} already applied; reject unavailable")]
    AlreadyApplied { proposal_id: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Activation-specific error used inside `activate_replacement` to tell the
/// caller whether the failure is transient (retry) or permanent (quarantine).
#[derive(Debug, thiserror::Error)]
enum MergeActivationError {
    #[error("retryable activation error: {0}")]
    Retryable(#[source] anyhow::Error),
    #[error("non-retryable activation error: {0}")]
    NonRetryable(#[source] anyhow::Error),
}

impl MergeActivationError {
    fn retryable(error: anyhow::Error) -> Self {
        Self::Retryable(error)
    }
    fn non_retryable(error: anyhow::Error) -> Self {
        Self::NonRetryable(error)
    }
}

pub struct MergeApplyRequest<'a> {
    pub store: &'a MergeProposalStore,
    pub proposal_id: &'a str,
    pub approved_pinned: &'a BTreeSet<MemoryId>,
    pub claim_locked: &'a BTreeSet<MemoryId>,
    pub crash: Option<SimulatedCrash>,
}

pub async fn approve_and_apply(
    substrate: &Substrate,
    request: MergeApplyRequest<'_>,
) -> Result<MergeProposal, MergeApplyError> {
    let _guard = MERGE_APPLY_LOCK.lock().await;
    let store = request.store;
    let proposal_id = request.proposal_id;
    let mut proposal = store.load(proposal_id)?;
    if proposal.status == MergeProposalStatus::Applied {
        return Ok(proposal);
    }
    if proposal.status.is_terminal() {
        return Err(MergeApplyError::Other(anyhow::anyhow!("proposal is terminal: {:?}", proposal.status)));
    }
    if proposal.status == MergeProposalStatus::Proposed {
        proposal.status = MergeProposalStatus::Approved;
        store.save(&proposal)?;
    }

    let journal = read_journal(&store.journal_path(proposal_id), proposal_id).map_err(|error| {
        proposal.status = MergeProposalStatus::Quarantined;
        let _ = store.save(&proposal);
        MergeApplyError::Quarantined(error.to_string())
    })?;
    let completed = journal.iter().map(|frame| frame.record.phase.as_str()).collect::<HashSet<_>>();
    if proposal.captured_sources.is_empty() {
        append_journal(store, proposal_id, "validated_intent", Value::Null)?;
        match preflight(substrate, &request, &proposal).await {
            Ok(captured) => proposal.captured_sources = captured,
            Err(reason) => {
                proposal.status = MergeProposalStatus::Invalidated;
                store.save(&proposal)?;
                append_journal(store, proposal_id, "invalidated", Value::String(reason.clone()))?;
                return Err(MergeApplyError::Invalidated(reason));
            }
        }
        proposal.status = MergeProposalStatus::Applying;
        store.save(&proposal)?;
        let captured = serde_json::to_value(&proposal.captured_sources).map_err(anyhow::Error::from)?;
        append_journal(store, proposal_id, "validated_complete", captured)?;
    }

    if !completed.contains("staged_complete") {
        append_journal(store, proposal_id, "staged_intent", Value::Null)?;
        stage_replacement(substrate, &mut proposal).await?;
        store.save(&proposal)?;
        append_journal(store, proposal_id, "staged_complete", Value::Null)?;
        maybe_crash(request.crash, SimulatedCrash::AfterStage)?;
    }

    for index in 0..proposal.captured_sources.len() {
        let phase = format!("superseding_{index}_complete");
        if completed.contains(phase.as_str()) {
            continue;
        }
        append_journal(store, proposal_id, &format!("superseding_{index}_intent"), Value::Null)?;
        let captured = proposal.captured_sources[index].clone();
        let superseded_hash = match supersede_source(substrate, &proposal, &captured).await {
            Ok(hash) => hash,
            Err(error) => return rollback(substrate, store, proposal, error.to_string()).await,
        };
        proposal.captured_sources[index].superseded_hash = Some(superseded_hash);
        store.save(&proposal)?;
        append_journal(store, proposal_id, &phase, Value::Null)?;
        maybe_crash(request.crash, SimulatedCrash::AfterSupersede(index))?;
    }

    maybe_crash(request.crash, SimulatedCrash::BeforeActivation)?;
    if !completed.contains("activating_complete") {
        append_journal(store, proposal_id, "activating_intent", Value::Null)?;
        match activate_replacement(substrate, &proposal).await {
            Ok(()) => {
                emit_merge_applied_once(substrate, &proposal)?;
                append_journal(store, proposal_id, "activating_complete", Value::Null)?;
                maybe_crash(request.crash, SimulatedCrash::AfterActivation)?;
            }
            Err(MergeActivationError::Retryable(error)) => {
                // The counter is incremented in memory before store.save; if
                // the subsequent save fails (e.g., disk full), the persisted
                // proposal remains at its old counter and the next reconcile
                // will re-attempt from that value. This is an accepted residual.
                proposal.reconcile_attempts += 1;
                if proposal.reconcile_attempts >= MAX_RECONCILE_ATTEMPTS {
                    proposal.status = MergeProposalStatus::Quarantined;
                    store.save(&proposal)?;
                    return Err(MergeApplyError::Quarantined(format!(
                        "activation failed after {} attempts; quarantined: {error}",
                        proposal.reconcile_attempts
                    )));
                }
                store.save(&proposal)?;
                return Err(MergeApplyError::Retryable(error.to_string()));
            }
            Err(MergeActivationError::NonRetryable(error)) => {
                proposal.status = MergeProposalStatus::Quarantined;
                store.save(&proposal)?;
                return Err(MergeApplyError::Quarantined(error.to_string()));
            }
        }
    }

    proposal.status = MergeProposalStatus::Applied;
    store.save(&proposal)?;
    append_journal(store, proposal_id, "done", Value::Null)?;
    Ok(proposal)
}

async fn preflight(
    substrate: &Substrate,
    request: &MergeApplyRequest<'_>,
    proposal: &MergeProposal,
) -> Result<Vec<CapturedSource>, String> {
    let mut projected = Vec::new();
    let mut captured = Vec::new();
    for id in &proposal.source_ids {
        let envelope = substrate.read_memory_envelope(id).await.map_err(|error| error.to_string())?;
        let encrypted = !matches!(envelope.content, MemoryContent::Plaintext(_));
        let memory = envelope.metadata;
        if memory.frontmatter.status == MemoryStatus::Pinned && !request.approved_pinned.contains(id) {
            return Err(format!("pinned source requires explicit approval: {id}"));
        }
        let hash = memory_hash(substrate, &memory).map_err(|error| error.to_string())?;
        projected.push(project_candidate(&VectorCandidate {
            memory: memory.clone(),
            vector: vec![1.0],
            encrypted,
            claim_locked: request.claim_locked.contains(id),
        }));
        captured.push(CapturedSource {
            id: id.clone(),
            expected_base_hash: hash,
            original_status: memory.frontmatter.status,
            original_trust_level: memory.frontmatter.trust_level,
            original_sensitivity: memory.frontmatter.sensitivity,
            superseded_hash: None,
        });
    }
    let exclusions = MergeCandidateExclusions {
        nonterminal_proposal_sources: request
            .store
            .nonterminal_source_ids(Some(&proposal.proposal_id))
            .map_err(|e| e.to_string())?,
        // W1/W5 hooks are present now; their manifests are not shipped yet.
        import_repair_lineage: BTreeSet::new(),
        backfill_manifest: BTreeSet::new(),
    };
    validate_merge_candidates(&projected, &exclusions).map_err(|error| error.to_string())?;

    let triple = substrate.active_embedding_triple().map_err(|error| error.to_string())?;
    let vectors = substrate.all_abstraction_vectors(&triple).map_err(|error| error.to_string())?;
    let vector_by_id =
        vectors.into_iter().map(|row| (row.memory_id, row.vector)).collect::<std::collections::HashMap<_, _>>();
    for left_id in &proposal.source_ids {
        for right_id in &proposal.source_ids {
            if left_id >= right_id {
                continue;
            }
            let left = vector_by_id
                .get(left_id)
                .ok_or_else(|| format!("source abstraction vector missing at approval time: {left_id}"))?;
            let right = vector_by_id
                .get(right_id)
                .ok_or_else(|| format!("source abstraction vector missing at approval time: {right_id}"))?;
            let score = cosine(left, right)
                .ok_or_else(|| "source abstraction vector empty or dimension mismatch".to_string())?;
            if score < DEFAULT_MERGE_SIMILARITY_THRESHOLD {
                return Err(format!("source pair cosine {score} below threshold {DEFAULT_MERGE_SIMILARITY_THRESHOLD}"));
            }
        }
    }

    let mut replacement = proposal.replacement.clone();
    generation_privacy_rebind(&mut replacement).map_err(|error| error.to_string())?;
    replacement.frontmatter.sensitivity = captured
        .iter()
        .map(|source| source.original_sensitivity)
        .max()
        .ok_or_else(|| "merge proposal requires a source classification floor".to_string())?;
    let rebound =
        crate::handlers::governance::classify_plaintext_memory(&replacement).map_err(|error| error.message)?;
    let floor = captured
        .iter()
        .map(|source| source.original_sensitivity.classification_outcome())
        .max()
        .ok_or_else(|| "merge proposal has no source classification floor".to_string())?;
    let classification = rebound.max(floor);
    if classification == ClassificationOutcome::Secret || classification == ClassificationOutcome::RequiresEncryption {
        return Err(
            "merge replacement would require encryption or refused storage after classification floor".to_string()
        );
    }
    Ok(captured)
}

fn memory_hash(substrate: &Substrate, memory: &Memory) -> anyhow::Result<Sha256> {
    let path = memory.path.as_ref().ok_or_else(|| anyhow::anyhow!("memory has no canonical path"))?;
    Ok(memory_substrate::markdown::hash_bytes(&fs::read(substrate.roots().repo.join(path.as_path()))?))
}

async fn stage_replacement(substrate: &Substrate, proposal: &mut MergeProposal) -> anyhow::Result<()> {
    if let Ok(existing) = substrate.read_memory(&proposal.replacement.frontmatter.id).await {
        if existing.frontmatter.is_merge_non_servable() && existing.frontmatter.supersedes == proposal.source_ids {
            proposal.replacement = existing;
            return Ok(());
        }
        anyhow::bail!("replacement id already exists outside this merge staging state");
    }
    if !proposal.provenance_overridden {
        union_source_provenance(substrate, proposal).await?;
    }
    generation_privacy_rebind(&mut proposal.replacement)?;
    proposal.replacement.frontmatter.sensitivity = proposal
        .captured_sources
        .iter()
        .map(|source| source.original_sensitivity)
        .max()
        .ok_or_else(|| anyhow::anyhow!("merge proposal has no source classification floor"))?;
    let fm = &mut proposal.replacement.frontmatter;
    fm.status = MemoryStatus::Candidate;
    fm.trust_level = TrustLevel::Candidate;
    fm.requires_user_confirmation = false;
    fm.review_state = None;
    fm.write_policy.human_review_required = false;
    fm.write_policy.policy_applied = STAGING_POLICY.to_string();
    fm.write_policy.expected_base_hash = None;
    fm.supersedes = proposal.source_ids.clone();
    fm.superseded_by.clear();
    let rebound = crate::handlers::governance::classify_plaintext_memory(&proposal.replacement)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let floor = proposal
        .captured_sources
        .iter()
        .map(|source| source.original_sensitivity.classification_outcome())
        .max()
        .ok_or_else(|| anyhow::anyhow!("merge proposal has no source classification floor"))?;
    let classification = rebound.max(floor);
    if classification == ClassificationOutcome::Secret || classification == ClassificationOutcome::RequiresEncryption {
        anyhow::bail!("merge replacement would require encryption or refused storage after classification floor");
    }
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: proposal.replacement.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-merge".to_string()),
                reason: Some(proposal.proposal_id.clone()),
            },
            allow_best_effort_durability: true,
            classification,
        })
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(())
}

fn generation_privacy_rebind(replacement: &mut Memory) -> anyhow::Result<()> {
    let body_text = format!("{}\n{}", replacement.frontmatter.summary, replacement.body);
    let combined = std::iter::once(body_text.as_str())
        .chain(replacement.frontmatter.abstraction.as_deref())
        .chain(replacement.frontmatter.cues.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let body = crate::handlers::governance::classify_privacy(&body_text, PrivacyNamespace::Agent, None)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let combined = crate::handlers::governance::classify_privacy(&combined, PrivacyNamespace::Agent, None)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    if body.storage_action.refuses_storage() || combined.storage_action.refuses_storage() {
        anyhow::bail!("secret refused before merge staging disk effects");
    }
    if matches!(body.storage_action, PrivacyStorageAction::Plaintext)
        && matches!(combined.storage_action, PrivacyStorageAction::EncryptAtRest)
    {
        replacement.frontmatter.abstraction = None;
        replacement.frontmatter.cues.clear();
    }
    Ok(())
}

async fn union_source_provenance(substrate: &Substrate, proposal: &mut MergeProposal) -> anyhow::Result<()> {
    for id in &proposal.source_ids {
        let source = substrate.read_memory(id).await?;
        for entity in source.frontmatter.entities {
            if !proposal.replacement.frontmatter.entities.iter().any(|existing| existing.id == entity.id) {
                proposal.replacement.frontmatter.entities.push(entity);
            }
        }
        for evidence in source.frontmatter.evidence {
            if !proposal.replacement.frontmatter.evidence.iter().any(|existing| existing.id == evidence.id) {
                proposal.replacement.frontmatter.evidence.push(evidence);
            }
        }
        for related in source.frontmatter.related {
            if !proposal.replacement.frontmatter.related.contains(&related) {
                proposal.replacement.frontmatter.related.push(related);
            }
        }
    }
    Ok(())
}

async fn supersede_source(
    substrate: &Substrate,
    proposal: &MergeProposal,
    captured: &CapturedSource,
) -> anyhow::Result<Sha256> {
    let mut source = substrate.read_memory(&captured.id).await?;
    if source.frontmatter.status == MemoryStatus::Superseded
        && source.frontmatter.superseded_by == [proposal.replacement.frontmatter.id.clone()]
    {
        return memory_hash(substrate, &source);
    }
    let current_hash = memory_hash(substrate, &source)?;
    if current_hash != captured.expected_base_hash
        || source.frontmatter.status != captured.original_status
        || !source.frontmatter.superseded_by.is_empty()
    {
        anyhow::bail!("source CAS precondition failed: {}", captured.id);
    }
    source.frontmatter.status = MemoryStatus::Superseded;
    if captured.original_trust_level == TrustLevel::Pinned {
        source.frontmatter.trust_level = TrustLevel::Trusted;
    }
    source.frontmatter.superseded_by = vec![proposal.replacement.frontmatter.id.clone()];
    source.frontmatter.updated_at = Utc::now();
    let classification = crate::handlers::governance::classify_plaintext_memory(&source)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: source,
            expected_base_hash: Some(captured.expected_base_hash.clone()),
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-merge".to_string()),
                reason: Some(proposal.proposal_id.clone()),
            },
            allow_best_effort_durability: true,
            classification,
        })
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    memory_hash(substrate, &substrate.read_memory(&captured.id).await?)
}

async fn activate_replacement(substrate: &Substrate, proposal: &MergeProposal) -> Result<(), MergeActivationError> {
    let mut replacement =
        substrate.read_memory(&proposal.replacement.frontmatter.id).await.map_err(|error| match error {
            memory_substrate::ReadError::NotFound(_) => {
                MergeActivationError::non_retryable(anyhow::anyhow!("replacement not found: {error}"))
            }
            memory_substrate::ReadError::Parse { .. }
            | memory_substrate::ReadError::Validation(_)
            | memory_substrate::ReadError::NotACanonicalMemory { .. } => {
                MergeActivationError::non_retryable(anyhow::anyhow!("replacement is unparseable: {error}"))
            }
            memory_substrate::ReadError::Io(_) => MergeActivationError::retryable(anyhow::anyhow!("{error}")),
        })?;
    if replacement.frontmatter.status == MemoryStatus::Active
        && replacement.frontmatter.write_policy.policy_applied != STAGING_POLICY
    {
        return Ok(());
    }
    if !replacement.frontmatter.is_merge_non_servable() {
        return Err(MergeActivationError::non_retryable(anyhow::anyhow!(
            "replacement activation CAS precondition failed"
        )));
    }
    let hash = memory_hash(substrate, &replacement).map_err(MergeActivationError::retryable)?;
    replacement.frontmatter.status = MemoryStatus::Active;
    replacement.frontmatter.trust_level =
        if proposal.captured_sources.iter().any(|source| source.original_trust_level == TrustLevel::Untrusted) {
            TrustLevel::Untrusted
        } else {
            TrustLevel::Trusted
        };
    replacement.frontmatter.write_policy.policy_applied = "merge-applied-v1".to_string();
    replacement.frontmatter.updated_at = Utc::now();
    let rebound = crate::handlers::governance::classify_plaintext_memory(&replacement)
        .map_err(|error| MergeActivationError::non_retryable(anyhow::anyhow!(error.message)))?;
    let floor = proposal
        .captured_sources
        .iter()
        .map(|source| source.original_sensitivity.classification_outcome())
        .max()
        .ok_or_else(|| {
            MergeActivationError::non_retryable(anyhow::anyhow!("merge proposal has no source classification floor"))
        })?;
    let classification = rebound.max(floor);
    if classification == ClassificationOutcome::Secret || classification == ClassificationOutcome::RequiresEncryption {
        return Err(MergeActivationError::non_retryable(anyhow::anyhow!(
            "merge replacement would require encryption or refused storage after classification floor"
        )));
    }
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: replacement,
            expected_base_hash: Some(hash),
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-merge".to_string()),
                reason: Some(proposal.proposal_id.clone()),
            },
            allow_best_effort_durability: true,
            classification,
        })
        .await
        .map_err(classify_write_failure_for_activation)?;
    Ok(())
}

fn classify_write_failure_for_activation(failure: memory_substrate::WriteFailure) -> MergeActivationError {
    use memory_substrate::WriteFailureKind;
    match failure.kind {
        WriteFailureKind::IoTyped { .. }
        | WriteFailureKind::DurabilityUnavailable
        | WriteFailureKind::IndexAfterCommitFailed
        | WriteFailureKind::RepairQueueFailed
        | WriteFailureKind::RepairStateNotDurable => MergeActivationError::retryable(anyhow::Error::from(failure)),
        _ => MergeActivationError::non_retryable(anyhow::Error::from(failure)),
    }
}

async fn rollback(
    substrate: &Substrate,
    store: &MergeProposalStore,
    mut proposal: MergeProposal,
    cause: String,
) -> Result<MergeProposal, MergeApplyError> {
    append_journal(store, &proposal.proposal_id, "rolling_back", Value::String(cause))?;
    let mut restored = Vec::new();
    for captured in &proposal.captured_sources {
        let mut source = substrate.read_memory(&captured.id).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if source.frontmatter.status == captured.original_status
            && source.frontmatter.trust_level == captured.original_trust_level
            && source.frontmatter.superseded_by.is_empty()
        {
            continue;
        }
        let current_hash = memory_hash(substrate, &source)?;
        if source.frontmatter.status != MemoryStatus::Superseded
            || source.frontmatter.superseded_by != [proposal.replacement.frontmatter.id.clone()]
            || captured.superseded_hash.as_ref() != Some(&current_hash)
        {
            proposal.status = MergeProposalStatus::Quarantined;
            store.save(&proposal)?;
            append_journal(store, &proposal.proposal_id, "quarantined", Value::String("rollback CAS failed".into()))?;
            return Err(MergeApplyError::Quarantined("rollback CAS failed".to_string()));
        }
        source.frontmatter.status = captured.original_status;
        source.frontmatter.trust_level = captured.original_trust_level;
        source.frontmatter.superseded_by.clear();
        source.frontmatter.updated_at = Utc::now();
        let classification = crate::handlers::governance::classify_plaintext_memory(&source)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: source,
                expected_base_hash: Some(current_hash),
                write_mode: WriteMode::ReplaceExisting,
                index_projection: None,
                event_context: EventContext {
                    actor: Some("memoryd-merge-rollback".into()),
                    reason: Some(proposal.proposal_id.clone()),
                },
                allow_best_effort_durability: true,
                classification,
            })
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        restored.push(captured.id.clone());
    }
    if let Ok(replacement) = substrate.read_memory(&proposal.replacement.frontmatter.id).await {
        if replacement.frontmatter.status != MemoryStatus::Tombstoned {
            substrate
                .tombstone_memory(TombstoneRequest {
                    id: proposal.replacement.frontmatter.id.clone(),
                    reason: "merge-rollback".to_string(),
                })
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
    }
    if !event_exists(substrate, &proposal.proposal_id, false)? {
        substrate
            .record_event_best_effort(EventKind::MergeRolledBack {
                proposal_id: proposal.proposal_id.clone(),
                replacement_id: proposal.replacement.frontmatter.id.clone(),
                restored_source_ids: restored,
            })
            .map_err(anyhow::Error::from)?;
    }
    proposal.status = MergeProposalStatus::RolledBack;
    store.save(&proposal)?;
    append_journal(store, &proposal.proposal_id, "rolled_back", Value::Null)?;
    Ok(proposal)
}

fn emit_merge_applied_once(substrate: &Substrate, proposal: &MergeProposal) -> anyhow::Result<()> {
    if event_exists(substrate, &proposal.proposal_id, true)? {
        return Ok(());
    }
    substrate.record_event_best_effort(EventKind::MergeApplied {
        proposal_id: proposal.proposal_id.clone(),
        replacement_id: proposal.replacement.frontmatter.id.clone(),
        source_ids: proposal.source_ids.clone(),
        per_source: proposal
            .captured_sources
            .iter()
            .map(|source| MergeAppliedSource {
                id: source.id.clone(),
                base_hash: source.expected_base_hash.to_string(),
                original_status: source.original_status.as_db_str().to_string(),
            })
            .collect(),
        created_by_dream_run: proposal.created_by.clone(),
    })?;
    Ok(())
}

fn event_exists(substrate: &Substrate, proposal_id: &str, applied: bool) -> anyhow::Result<bool> {
    Ok(substrate.events()?.iter().any(|event| match &event.kind {
        EventKind::MergeApplied { proposal_id: existing, .. } if applied => existing == proposal_id,
        EventKind::MergeRolledBack { proposal_id: existing, .. } if !applied => existing == proposal_id,
        _ => false,
    }))
}

fn maybe_crash(actual: Option<SimulatedCrash>, point: SimulatedCrash) -> Result<(), MergeApplyError> {
    if actual == Some(point) {
        Err(MergeApplyError::SimulatedCrash(point))
    } else {
        Ok(())
    }
}

fn append_journal(store: &MergeProposalStore, proposal_id: &str, phase: &str, data: Value) -> anyhow::Result<()> {
    let path = store.journal_path(proposal_id);
    fs::create_dir_all(path.parent().expect("journal path has parent"))?;
    let frames = read_journal(&path, proposal_id)?;
    let record = JournalRecord { phase: phase.to_string(), data };
    let record_bytes = serde_json::to_vec(&record)?;
    let frame = JournalFrame {
        seq: frames.len() as u64 + 1,
        proposal_id: proposal_id.to_string(),
        record,
        record_sha256: hex::encode(Sha256Hasher::digest(record_bytes)),
    };
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, &frame)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn read_journal(path: &Path, proposal_id: &str) -> anyhow::Result<Vec<JournalFrame>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut frames = Vec::new();
    let mut offset = 0_u64;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        let next_offset = offset + read as u64;
        let final_line = next_offset == file_len;
        let frame: JournalFrame = match serde_json::from_str(line.trim_end()) {
            Ok(frame) => frame,
            Err(_error) if final_line => {
                drop(reader);
                let mut file = OpenOptions::new().write(true).open(path)?;
                file.set_len(offset)?;
                file.seek(SeekFrom::Start(offset))?;
                file.sync_all()?;
                break;
            }
            Err(error) => anyhow::bail!("corrupt merge journal interior at seq {}: {error}", frames.len() + 1),
        };
        if frame.proposal_id != proposal_id || frame.seq != frames.len() as u64 + 1 {
            anyhow::bail!("merge journal identity/sequence mismatch");
        }
        let expected = hex::encode(Sha256Hasher::digest(serde_json::to_vec(&frame.record)?));
        if frame.record_sha256 != expected {
            anyhow::bail!("merge journal checksum mismatch at seq {}", frame.seq);
        }
        frames.push(frame);
        offset = next_offset;
    }
    Ok(frames)
}

/// Startup/dream-entry reconciliation. Call before exposing read surfaces.
///
/// `state` is used to emit `OperatorActionRequired` notifications for proposals
/// that are quarantined during reconciliation. Callers without a `HandlerState`
/// (e.g., `dream` entry) can pass `None`.
pub async fn reconcile_applying(
    substrate: &Substrate,
    state: Option<&crate::handlers::HandlerState>,
) -> Vec<(String, String)> {
    let store = MergeProposalStore::new(&substrate.roots().runtime);
    let proposals = match store.list() {
        Ok(proposals) => proposals,
        Err(error) => return vec![("store".to_string(), error.to_string())],
    };
    let mut failures = Vec::new();
    for proposal in proposals.into_iter().filter(|proposal| proposal.status == MergeProposalStatus::Applying) {
        let approved_pinned = proposal
            .captured_sources
            .iter()
            .filter(|source| source.original_status == MemoryStatus::Pinned)
            .map(|source| source.id.clone())
            .collect();
        let proposal_id = proposal.proposal_id.clone();
        match approve_and_apply(
            substrate,
            MergeApplyRequest {
                store: &store,
                proposal_id: &proposal_id,
                approved_pinned: &approved_pinned,
                claim_locked: &BTreeSet::new(),
                crash: None,
            },
        )
        .await
        {
            Ok(_) => {}
            Err(MergeApplyError::Retryable(_)) => {}
            Err(MergeApplyError::Quarantined(reason)) => {
                if let Some(state) = state {
                    state.emit_notification(crate::protocol::NotificationEvent::OperatorActionRequired {
                        message: format!("merge proposal {proposal_id} is quarantined: {reason}"),
                    });
                }
            }
            Err(error) => {
                failures.push((proposal_id, error.to_string()));
            }
        }
    }
    failures
}

#[cfg(test)]
pub(crate) fn memory(id: &str) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: memory_substrate::Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: memory_substrate::MemoryType::Procedure,
            scope: memory_substrate::Scope::Agent,
            summary: "summary".into(),
            confidence: 0.8,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: memory_substrate::Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: memory_substrate::Author {
                kind: memory_substrate::AuthorKind::Agent,
                user_handle: None,
                harness: Some("test".into()),
                harness_version: None,
                session_id: Some("merge-test".into()),
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: memory_substrate::Source {
                kind: memory_substrate::SourceKind::AgentPrimary,
                reference: None,
                session_id: Some("merge-test".into()),
                harness: Some("test".into()),
                harness_version: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: memory_substrate::RetrievalPolicy {
                passive_recall: true,
                max_scope: memory_substrate::Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: memory_substrate::WritePolicy {
                human_review_required: false,
                policy_applied: "test".into(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: Some("summary".into()),
            cues: Vec::new(),
            extras: Default::default(),
        },
        body: "body".into(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_substrate::{ClassificationOutcome, InitOptions, Roots};

    macro_rules! apply {
        ($substrate:expr, $store:expr, $proposal_id:expr, $pinned:expr, $locked:expr, $crash:expr $(,)?) => {
            approve_and_apply(
                $substrate,
                MergeApplyRequest {
                    store: $store,
                    proposal_id: $proposal_id,
                    approved_pinned: $pinned,
                    claim_locked: $locked,
                    crash: $crash,
                },
            )
        };
    }

    #[test]
    fn cosine_pairs_are_fenced_ranked_and_capped() {
        let mut first = memory("mem_20260711_aaaaaaaaaaaaaaaa_000001");
        let second = memory("mem_20260711_aaaaaaaaaaaaaaaa_000002");
        let third = memory("mem_20260711_aaaaaaaaaaaaaaaa_000003");
        first.frontmatter.review_state = Some("pending-review".into());
        let candidates = vec![
            VectorCandidate { memory: first, vector: vec![1.0, 0.0], encrypted: false, claim_locked: false },
            VectorCandidate { memory: second, vector: vec![1.0, 0.0], encrypted: false, claim_locked: false },
            VectorCandidate { memory: third, vector: vec![0.9, 0.1], encrypted: false, claim_locked: false },
        ];
        let pairs = near_duplicate_pairs(
            &candidates,
            &MergeCandidateExclusions::default(),
            &MergeCandidateConfig { cosine_threshold: 0.8, proposal_cap: 1 },
        );
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].left, candidates[1].memory.frontmatter.id);
    }

    #[test]
    fn sensitive_generation_drop_remains_limited_to_merge_staging() {
        let mut replacement = memory("mem_20260711_aaaaaaaaaaaaaaaa_000004");
        replacement.frontmatter.sensitivity = Sensitivity::Public;
        replacement.frontmatter.abstraction = Some("Contact reviewer@example.com".to_string());
        replacement.frontmatter.cues = vec!["Review contact".to_string()];
        let body = replacement.body.clone();

        generation_privacy_rebind(&mut replacement).expect("merge staging rebind");

        assert_eq!(replacement.body, body);
        assert!(replacement.frontmatter.abstraction.is_none());
        assert!(replacement.frontmatter.cues.is_empty());
    }

    #[test]
    fn torn_tail_is_truncated_but_checksum_failure_is_corruption() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = MergeProposalStore::new(temp.path());
        append_journal(&store, "proposal", "validated", Value::Null).expect("journal");
        let path = store.journal_path("proposal");
        OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{\"seq\":2").unwrap();
        assert_eq!(read_journal(&path, "proposal").unwrap().len(), 1);
        let mut bytes = fs::read(&path).unwrap();
        let position = bytes.windows(b"validated".len()).position(|window| window == b"validated").expect("phase");
        bytes[position] = b'w';
        fs::write(&path, bytes).unwrap();
        assert!(read_journal(&path, "proposal").is_err());

        let interior_store = MergeProposalStore::new(&temp.path().join("interior"));
        append_journal(&interior_store, "proposal", "first", Value::Null).unwrap();
        append_journal(&interior_store, "proposal", "second", Value::Null).unwrap();
        let interior_path = interior_store.journal_path("proposal");
        let mut bytes = fs::read(&interior_path).unwrap();
        let position = bytes.windows(b"first".len()).position(|window| window == b"first").unwrap();
        bytes[position] = b'w';
        fs::write(&interior_path, bytes).unwrap();
        assert!(read_journal(&interior_path, "proposal").is_err());
    }

    #[tokio::test]
    async fn crash_between_supersedes_resumes_and_replay_is_idempotent() {
        let (temp, substrate) = substrate().await;
        let first = memory("mem_20260711_aaaaaaaaaaaaaaaa_000011");
        let second = memory("mem_20260711_aaaaaaaaaaaaaaaa_000012");
        write(&substrate, first.clone()).await;
        write(&substrate, second.clone()).await;
        let mut replacement = memory("mem_20260711_aaaaaaaaaaaaaaaa_000013");
        replacement.frontmatter.sensitivity = Sensitivity::Public;
        let proposal = MergeProposal::new(
            vec![first.frontmatter.id.clone(), second.frontmatter.id.clone()],
            replacement,
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let crash = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterSupersede(0)),
        )
        .await;
        assert!(matches!(crash, Err(MergeApplyError::SimulatedCrash(SimulatedCrash::AfterSupersede(0)))));
        assert_eq!(
            substrate.read_memory(&first.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Superseded
        );
        assert_eq!(
            substrate.read_memory(&second.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Active
        );
        let staged = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        assert!(staged.frontmatter.is_merge_non_servable());
        let fts = substrate
            .query_chunks(memory_substrate::ChunkQuery { text: Some("body".into()), triple: None, vector: None, namespaces: None })
            .await
            .unwrap();
        assert!(fts.iter().all(|hit| hit.memory_id != proposal.replacement.frontmatter.id));

        let applied =
            apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await.unwrap();
        assert_eq!(applied.status, MergeProposalStatus::Applied);
        assert_eq!(
            substrate.read_memory(&second.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Superseded
        );
        assert_eq!(
            substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Active
        );
        assert_eq!(
            substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.sensitivity,
            Sensitivity::Internal
        );

        apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await.unwrap();
        assert_eq!(
            substrate
                .events()
                .unwrap()
                .iter()
                .filter(|event| matches!(&event.kind, EventKind::MergeApplied { proposal_id, .. } if proposal_id == &proposal.proposal_id))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn activation_gap_recovers_before_reads_are_reopened() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000021");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000022"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();
        let result = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::BeforeActivation),
        )
        .await;
        assert!(matches!(result, Err(MergeApplyError::SimulatedCrash(SimulatedCrash::BeforeActivation))));
        assert_eq!(
            substrate.read_memory(&source.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Superseded
        );
        assert!(substrate
            .read_memory(&proposal.replacement.frontmatter.id)
            .await
            .unwrap()
            .frontmatter
            .is_merge_non_servable());
        assert!(reconcile_applying(&substrate, None).await.is_empty());
        assert_eq!(
            substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Active
        );
    }

    #[tokio::test]
    async fn stage_and_post_activation_crashes_resume_forward() {
        for (ordinal, crash) in [SimulatedCrash::AfterStage, SimulatedCrash::AfterActivation].into_iter().enumerate() {
            let (temp, substrate) = substrate().await;
            let source = memory(&format!("mem_20260711_bbbbbbbbbbbbbbbb_{:06}", ordinal * 2 + 1));
            write(&substrate, source.clone()).await;
            let proposal = MergeProposal::new(
                vec![source.frontmatter.id.clone()],
                memory(&format!("mem_20260711_bbbbbbbbbbbbbbbb_{:06}", ordinal * 2 + 2)),
                Vec::new(),
                "dream-test",
            )
            .unwrap();
            let store = MergeProposalStore::new(&temp.path().join("runtime"));
            store.create(&proposal).unwrap();
            assert!(matches!(
                apply!(
                    &substrate,
                    &store,
                    &proposal.proposal_id,
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                    Some(crash)
                )
                .await,
                Err(MergeApplyError::SimulatedCrash(point)) if point == crash
            ));
            assert!(reconcile_applying(&substrate, None).await.is_empty());
            assert_eq!(
                substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.status,
                MemoryStatus::Active
            );
        }
    }

    #[tokio::test]
    async fn source_cas_failure_rolls_back_owned_sources() {
        let (temp, substrate) = substrate().await;
        let first = memory("mem_20260711_aaaaaaaaaaaaaaaa_000031");
        let second = memory("mem_20260711_aaaaaaaaaaaaaaaa_000032");
        write(&substrate, first.clone()).await;
        write(&substrate, second.clone()).await;
        let proposal = MergeProposal::new(
            vec![first.frontmatter.id.clone(), second.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000033"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();
        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterSupersede(0)),
        )
        .await;
        let mut edited = substrate.read_memory(&second.frontmatter.id).await.unwrap();
        let hash = memory_hash(&substrate, &edited).unwrap();
        edited.body.push_str(" concurrent edit");
        replace(&substrate, edited, hash).await;
        let outcome =
            apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await.unwrap();
        assert_eq!(outcome.status, MergeProposalStatus::RolledBack);
        assert_eq!(
            substrate.read_memory(&first.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Active
        );
        assert_eq!(substrate.read_memory(&second.frontmatter.id).await.unwrap().body, "body concurrent edit");
        assert_eq!(
            substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Tombstoned
        );
    }

    #[tokio::test]
    async fn rollback_hash_race_quarantines_without_overwrite() {
        let (temp, substrate) = substrate().await;
        let first = memory("mem_20260711_aaaaaaaaaaaaaaaa_000051");
        let second = memory("mem_20260711_aaaaaaaaaaaaaaaa_000052");
        write(&substrate, first.clone()).await;
        write(&substrate, second.clone()).await;
        let proposal = MergeProposal::new(
            vec![first.frontmatter.id.clone(), second.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000053"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();
        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterSupersede(0))
        )
        .await;
        for id in [&first.frontmatter.id, &second.frontmatter.id] {
            let mut edited = substrate.read_memory(id).await.unwrap();
            let hash = memory_hash(&substrate, &edited).unwrap();
            edited.body.push_str(" newer human edit");
            replace(&substrate, edited, hash).await;
        }
        let result = apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await;
        assert!(matches!(result, Err(MergeApplyError::Quarantined(_))));
        assert_eq!(store.load(&proposal.proposal_id).unwrap().status, MergeProposalStatus::Quarantined);
        assert!(substrate.read_memory(&first.frontmatter.id).await.unwrap().body.ends_with("newer human edit"));
    }

    #[tokio::test]
    async fn pinned_source_requires_named_approval_and_activates_as_trusted() {
        let (temp, substrate) = substrate().await;
        let mut source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000041");
        source.frontmatter.status = MemoryStatus::Pinned;
        source.frontmatter.trust_level = TrustLevel::Pinned;
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000042"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();
        let refused = apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await;
        assert!(matches!(refused, Err(MergeApplyError::Invalidated(_))));

        let mut second = proposal.clone();
        second.proposal_id = ulid::Ulid::new().to_string();
        second.replacement.frontmatter.id = MemoryId::new("mem_20260711_aaaaaaaaaaaaaaaa_000043");
        second.replacement.path = Some(memory_substrate::RepoPath::new("agent/patterns/pinned-replacement.md"));
        store.create(&second).unwrap();
        let approved = BTreeSet::from([source.frontmatter.id.clone()]);
        apply!(&substrate, &store, &second.proposal_id, &approved, &BTreeSet::new(), None).await.unwrap();
        let old = substrate.read_memory(&source.frontmatter.id).await.unwrap();
        assert_eq!(
            (old.frontmatter.status, old.frontmatter.trust_level),
            (MemoryStatus::Superseded, TrustLevel::Trusted)
        );
        assert_eq!(
            substrate.read_memory(&second.replacement.frontmatter.id).await.unwrap().frontmatter.trust_level,
            TrustLevel::Trusted
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn retryable_activation_failure_increments_counter_and_quarantines_after_three() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000061");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000062"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterStage)
        )
        .await;

        // Make the staged replacement unreadable so activation fails with a
        // retryable IO error.
        let replacement_path = substrate
            .roots()
            .repo
            .join(substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().path.unwrap().as_path());
        std::fs::set_permissions(&replacement_path, std::fs::Permissions::from_mode(0o000)).unwrap();

        for attempt in 1..=3 {
            let result =
                apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await;
            let proposal = store.load(&proposal.proposal_id).unwrap();
            if attempt < 3 {
                assert!(matches!(result, Err(MergeApplyError::Retryable(_))), "attempt {attempt} should be retryable");
                assert_eq!(proposal.reconcile_attempts, attempt);
                assert_eq!(proposal.status, MergeProposalStatus::Applying);
            } else {
                assert!(matches!(result, Err(MergeApplyError::Quarantined(_))), "attempt 3 should quarantine");
                assert_eq!(proposal.status, MergeProposalStatus::Quarantined);
                assert!(proposal.reconcile_attempts >= 3);
            }
        }
    }

    #[tokio::test]
    async fn non_retryable_activation_failure_quarantines_without_incrementing_counter() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000071");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000072"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterStage)
        )
        .await;

        // Replace the staged replacement with a plain Active memory so the merge
        // non-servable precondition fails permanently (non-retryable).
        let mut active = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        let hash = memory_hash(&substrate, &active).unwrap();
        active.frontmatter.status = MemoryStatus::Active;
        active.frontmatter.trust_level = TrustLevel::Trusted;
        replace(&substrate, active, hash).await;

        let result = apply!(&substrate, &store, &proposal.proposal_id, &BTreeSet::new(), &BTreeSet::new(), None).await;
        assert!(matches!(result, Err(MergeApplyError::Quarantined(_))));
        let proposal = store.load(&proposal.proposal_id).unwrap();
        assert_eq!(proposal.status, MergeProposalStatus::Quarantined);
        assert_eq!(proposal.reconcile_attempts, 0);
    }

    #[tokio::test]
    async fn reject_restores_stuck_applying_sources_and_tombstones_replacement() {
        let (temp, substrate) = substrate().await;
        let first = memory("mem_20260711_aaaaaaaaaaaaaaaa_000081");
        let second = memory("mem_20260711_aaaaaaaaaaaaaaaa_000082");
        write(&substrate, first.clone()).await;
        write(&substrate, second.clone()).await;
        let proposal = MergeProposal::new(
            vec![first.frontmatter.id.clone(), second.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000083"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterSupersede(0))
        )
        .await;

        // Concurrently edit the second source body so the rollback/restore preserves it.
        let mut edited_second = substrate.read_memory(&second.frontmatter.id).await.unwrap();
        let hash = memory_hash(&substrate, &edited_second).unwrap();
        edited_second.body.push_str(" untouched body");
        replace(&substrate, edited_second, hash).await;

        let proposal = reject_proposal(&substrate, &store, &proposal.proposal_id).await.unwrap();
        assert_eq!(proposal.status, MergeProposalStatus::RolledBack);

        let first_restored = substrate.read_memory(&first.frontmatter.id).await.unwrap();
        assert_eq!(first_restored.frontmatter.status, MemoryStatus::Active);
        assert_eq!(first_restored.frontmatter.trust_level, TrustLevel::Trusted);
        assert!(first_restored.frontmatter.superseded_by.is_empty());
        assert_eq!(first_restored.body, first.body);

        let second_restored = substrate.read_memory(&second.frontmatter.id).await.unwrap();
        assert_eq!(second_restored.body, "body untouched body");
        assert_eq!(second_restored.frontmatter.status, MemoryStatus::Active);

        assert_eq!(
            substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Tombstoned
        );

        assert!(substrate
            .events()
            .unwrap()
            .iter()
            .any(|event| matches!(&event.kind, EventKind::MergeRolledBack { proposal_id, .. } if proposal_id == &proposal.proposal_id)));
    }

    #[tokio::test]
    async fn reject_proposed_sets_rejected_without_restoring() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000091");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000092"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let proposal = reject_proposal(&substrate, &store, &proposal.proposal_id).await.unwrap();
        assert_eq!(proposal.status, MergeProposalStatus::Rejected);
        assert_eq!(
            substrate.read_memory(&source.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Active
        );
        assert!(substrate.read_memory(&proposal.replacement.frontmatter.id).await.is_err());
    }

    #[tokio::test]
    async fn reconcile_applying_emits_notification_for_quarantined_and_skips_retryable() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000111");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000112"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterStage)
        )
        .await;

        // Force a non-retryable activation precondition failure.
        let mut active = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        let hash = memory_hash(&substrate, &active).unwrap();
        active.frontmatter.status = MemoryStatus::Active;
        active.frontmatter.trust_level = TrustLevel::Trusted;
        replace(&substrate, active, hash).await;

        let state = crate::handlers::HandlerState::new();
        let mut rx = state.subscribe_notifications();
        let failures = reconcile_applying(&substrate, Some(&state)).await;
        assert!(failures.is_empty(), "quarantined proposal should not be a startup failure");

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("notification should be emitted")
            .expect("channel should be open");
        assert!(matches!(event, crate::protocol::NotificationEvent::OperatorActionRequired { .. }));
        assert_eq!(store.load(&proposal.proposal_id).unwrap().status, MergeProposalStatus::Quarantined);
    }

    #[tokio::test]
    async fn count_candidate_attention_excludes_merge_staged_candidates() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000101");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000102"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterStage)
        )
        .await;

        let staged = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        assert!(staged.frontmatter.is_merge_non_servable());

        let count =
            crate::recall::startup::count_candidate_attention(&substrate, &["agent".to_string()]).await.unwrap();
        assert_eq!(count, 0, "merge-staged replacement must not count as pending attention");

        // A plain candidate requiring confirmation does count.
        let mut candidate = memory("mem_20260711_aaaaaaaaaaaaaaaa_000103");
        candidate.frontmatter.status = MemoryStatus::Candidate;
        candidate.frontmatter.trust_level = TrustLevel::Candidate;
        candidate.frontmatter.requires_user_confirmation = true;
        write(&substrate, candidate).await;

        let count =
            crate::recall::startup::count_candidate_attention(&substrate, &["agent".to_string()]).await.unwrap();
        assert_eq!(count, 1, "candidate requiring confirmation must count");
    }

    #[tokio::test]
    async fn staged_replacement_invisible_to_gated_lanes() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000201");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000202"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let _ = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterStage)
        )
        .await;

        let staged = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        let staged_id = staged.frontmatter.id.clone();
        assert!(staged.frontmatter.is_merge_non_servable());

        // Gated recall surfaces exclude merge-staged candidates.
        let rows = substrate
            .query_recall_index(memory_substrate::RecallIndexQuery {
                statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned, MemoryStatus::Candidate],
                exclude_merge_non_servable: true,
                ..memory_substrate::RecallIndexQuery::default()
            })
            .await
            .unwrap();
        assert!(
            rows.iter().all(|row| row.id != staged_id),
            "merge-staged replacement must not appear in gated recall index"
        );

        // Counting through the gated helper must also exclude it.
        let count = substrate
            .count_recall_index_excluding_merge_staged(memory_substrate::RecallIndexQuery {
                statuses: vec![MemoryStatus::Candidate],
                ..memory_substrate::RecallIndexQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(count, 0, "merge-staged candidate must not count as candidate");

        // Review queue membership must not include the staged row.
        let queue = substrate.review_queue(10).await.unwrap();
        assert!(queue.rows.iter().all(|row| row.id != staged_id.as_str()));
        assert_eq!(queue.total, 0);

        // Chunk retrieval paths only cover active/pinned rows.
        let hits = substrate
            .query_chunks(memory_substrate::ChunkQuery {
                text: Some("body".into()),
                ..memory_substrate::ChunkQuery::default()
            })
            .await
            .unwrap();
        assert!(hits.iter().all(|hit| hit.memory_id != staged_id));

        let hybrid = substrate.query_hybrid_chunks("body", None, 10, None).await.unwrap();
        assert!(hybrid.iter().all(|hit| hit.memory_id != staged_id));

        // Backup/export surfaces with the exclusion flag disabled still see the staged row.
        let count = substrate
            .count_recall_index(memory_substrate::RecallIndexQuery {
                statuses: vec![MemoryStatus::Candidate],
                exclude_merge_non_servable: false,
                ..memory_substrate::RecallIndexQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(count, 1, "merge-staged candidate is visible when exclusion is disabled");
    }

    #[tokio::test]
    async fn reject_after_activation_crash_completes_applied() {
        let (temp, substrate) = substrate().await;
        let source = memory("mem_20260711_aaaaaaaaaaaaaaaa_000201");
        write(&substrate, source.clone()).await;
        let proposal = MergeProposal::new(
            vec![source.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000202"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let result = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterActivation)
        )
        .await;
        assert!(matches!(result, Err(MergeApplyError::SimulatedCrash(SimulatedCrash::AfterActivation))));

        let replacement = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        assert_eq!(replacement.frontmatter.status, MemoryStatus::Active);
        assert_eq!(replacement.frontmatter.write_policy.policy_applied, "merge-applied-v1");
        assert_eq!(
            substrate.read_memory(&source.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Superseded
        );

        let merge_applied_count = substrate
            .events()
            .unwrap()
            .iter()
            .filter(|event| matches!(&event.kind, EventKind::MergeApplied { proposal_id, .. } if proposal_id == &proposal.proposal_id))
            .count();
        assert_eq!(merge_applied_count, 1, "merge_applied event emitted exactly once");

        let result = reject_proposal(&substrate, &store, &proposal.proposal_id).await;
        assert!(matches!(result, Err(MergeApplyError::AlreadyApplied { .. })));

        let proposal = store.load(&proposal.proposal_id).unwrap();
        assert_eq!(proposal.status, MergeProposalStatus::Applied);

        let replacement = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        assert_eq!(replacement.frontmatter.status, MemoryStatus::Active);
        assert_eq!(
            substrate.read_memory(&source.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Superseded
        );

        let merge_applied_count = substrate
            .events()
            .unwrap()
            .iter()
            .filter(|event| matches!(&event.kind, EventKind::MergeApplied { proposal_id, .. } if proposal_id == &proposal.proposal_id))
            .count();
        assert_eq!(merge_applied_count, 1, "reject must not emit a second merge_applied event");

        let journal = read_journal(&store.journal_path(&proposal.proposal_id), &proposal.proposal_id).unwrap();
        assert!(journal.iter().any(|frame| frame.record.phase == "done"));
        assert!(!journal.iter().any(|frame| frame.record.phase == "rolled_back"));
    }

    #[tokio::test]
    async fn reject_pre_activation_quarantine_still_restores() {
        let (temp, substrate) = substrate().await;
        let first = memory("mem_20260711_aaaaaaaaaaaaaaaa_000211");
        let second = memory("mem_20260711_aaaaaaaaaaaaaaaa_000212");
        write(&substrate, first.clone()).await;
        write(&substrate, second.clone()).await;
        let proposal = MergeProposal::new(
            vec![first.frontmatter.id.clone(), second.frontmatter.id.clone()],
            memory("mem_20260711_aaaaaaaaaaaaaaaa_000213"),
            Vec::new(),
            "dream-test",
        )
        .unwrap();
        let store = MergeProposalStore::new(&temp.path().join("runtime"));
        store.create(&proposal).unwrap();

        let result = apply!(
            &substrate,
            &store,
            &proposal.proposal_id,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some(SimulatedCrash::AfterSupersede(0))
        )
        .await;
        assert!(matches!(result, Err(MergeApplyError::SimulatedCrash(SimulatedCrash::AfterSupersede(0)))));

        assert_eq!(
            substrate.read_memory(&first.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Superseded
        );
        let replacement = substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap();
        assert!(replacement.frontmatter.is_merge_non_servable());

        // Simulate a pre-activation quarantine (e.g., a crash before activation).
        let mut proposal = store.load(&proposal.proposal_id).unwrap();
        proposal.status = MergeProposalStatus::Quarantined;
        store.save(&proposal).unwrap();

        let proposal = reject_proposal(&substrate, &store, &proposal.proposal_id).await.unwrap();
        assert_eq!(proposal.status, MergeProposalStatus::RolledBack);

        let first_restored = substrate.read_memory(&first.frontmatter.id).await.unwrap();
        assert_eq!(first_restored.frontmatter.status, MemoryStatus::Active);
        assert_eq!(first_restored.frontmatter.trust_level, TrustLevel::Trusted);
        assert!(first_restored.frontmatter.superseded_by.is_empty());
        assert_eq!(first_restored.body, first.body);

        assert_eq!(
            substrate.read_memory(&proposal.replacement.frontmatter.id).await.unwrap().frontmatter.status,
            MemoryStatus::Tombstoned
        );

        assert!(substrate.events().unwrap().iter().any(|event| matches!(
            &event.kind,
            EventKind::MergeRolledBack { proposal_id, .. } if proposal_id == &proposal.proposal_id
        )));
    }

    async fn substrate() -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().unwrap();
        let substrate = Substrate::init(
            Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_mergetest".into()) },
        )
        .await
        .unwrap();
        (temp, substrate)
    }

    async fn write(substrate: &Substrate, memory: Memory) {
        let id = memory.frontmatter.id.clone();
        let status = memory.frontmatter.status;
        let index_embeddings = memory.frontmatter.retrieval_policy.index_embeddings;
        let abstraction = memory.frontmatter.abstraction.clone();
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .unwrap();
        if index_embeddings && matches!(status, MemoryStatus::Active | MemoryStatus::Pinned) {
            if let Some(abstraction) = abstraction {
                let triple = substrate.active_embedding_triple().expect("active triple");
                let vector = vec![1.0; triple.dimension as usize];
                substrate
                    .update_aux_embedding(memory_substrate::AuxEmbeddingUpdate {
                        row_kind: memory_substrate::AuxRowKind::Abstraction,
                        target_id: id.as_str().to_string(),
                        expected_content_hash: memory_substrate::markdown::hash_bytes(abstraction.as_bytes()),
                        triple,
                        vector,
                    })
                    .await
                    .unwrap();
            }
        }
    }

    async fn replace(substrate: &Substrate, memory: Memory, hash: Sha256) {
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: Some(hash),
                write_mode: WriteMode::ReplaceExisting,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .unwrap();
    }
}
