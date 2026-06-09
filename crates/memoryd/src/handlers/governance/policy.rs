//! Policy-set loading, tombstone index loading/writing, the bounded active-memory
//! fan-out, and the governance engine adapters.
//!
//! Owns `load_policy_set` (the only `pub(crate)` surface here), the tombstone
//! index read/write helpers, the `spawn_blocking` + semaphore active-memory
//! summariser, and the `SimilaritySearch`/`ContradictionTiebreaker`/
//! `SessionSpawnResolver` implementations the contradiction engine is wired with.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use memory_governance::{
    CandidateMemory, ContradictionTiebreaker, ExistingMemorySummary, FileSourceResolver, GovernanceEngine,
    GovernanceProviders, GroundingVerifier, PolicySet, PolicySource, SessionSpawnResolver, SimilaritySearch,
    TiebreakOutcome, TombstoneIndex, TombstoneKind, TombstoneRule,
};
use memory_source::ArtifactStore;
use memory_substrate::{AuxScope, Memory, MemoryContent, MemoryStatus, RecallIndexQuery, Substrate};

use crate::handlers::{entity_ids, namespace_for_frontmatter, HandlerError};

pub(crate) fn load_policy_set(repo: &Path) -> Result<(PolicySet, PolicySource), HandlerError> {
    let policy_dir = repo.join("policies");
    let has_yaml = std::fs::read_dir(&policy_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|extension| extension == "yaml"));

    if has_yaml {
        match PolicySet::load_from_dir(&policy_dir) {
            Ok(policies) => return Ok((policies, PolicySource::Disk)),
            Err(error) => return Err(HandlerError::invalid_request(format!("invalid governance policy: {error}"))),
        }
    }

    Ok((PolicySet::builtin(), PolicySource::BuiltInFallback))
}

pub(super) fn load_tombstone_index(repo: &Path) -> Result<TombstoneIndex, HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    if !tombstone_dir.exists() {
        return Ok(TombstoneIndex::default());
    }
    TombstoneIndex::load_jsonl_dir(&tombstone_dir)
        .map_err(|error| HandlerError::invalid_request(format!("invalid tombstone rules: {error}")))
}

pub(super) fn existing_summary_from_memory(memory: Memory, body: String) -> ExistingMemorySummary {
    ExistingMemorySummary::new(
        memory.frontmatter.id.as_str().to_string(),
        namespace_for_frontmatter(&memory.frontmatter),
        body,
        1.0,
    )
    .with_entity_ids(entity_ids(&memory.frontmatter))
}

pub(super) fn write_tombstone_rule(
    repo: &Path,
    memory: &Memory,
    claim: &str,
    reason: &str,
) -> Result<(), HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    std::fs::create_dir_all(&tombstone_dir)
        .map_err(|error| HandlerError::substrate(format!("create tombstone dir: {error}")))?;
    let key = memory_governance::CandidateTombstoneKey::from_claim(claim, entity_ids(&memory.frontmatter))
        .with_target_memory_id(memory.frontmatter.id.as_str().to_string());
    let rule = TombstoneRule {
        id: format!("tomb_{}", memory.frontmatter.id.as_str()),
        target_memory_id: Some(memory.frontmatter.id.as_str().to_string()),
        content_hash: key.content_hash,
        entity_hash: key.entity_hash,
        reason: TombstoneKind::UserForget,
        reason_text: Some(reason.to_string()),
        active: true,
    };
    let path = tombstone_dir.join("memoryd-forget.jsonl");
    let mut file =
        OpenOptions::new().create(true).append(true).open(&path).map_err(|error| {
            HandlerError::substrate(format!("open tombstone rule file {}: {error}", path.display()))
        })?;
    let line = serde_json::to_string(&rule)
        .map_err(|error| HandlerError::substrate(format!("serialize tombstone rule: {error}")))?;
    writeln!(file, "{line}")
        .map_err(|error| HandlerError::substrate(format!("append tombstone rule file {}: {error}", path.display())))?;
    Ok(())
}

pub(super) struct GovernanceEngineInput {
    pub(super) policies: PolicySet,
    pub(super) active: Vec<ExistingMemorySummary>,
    pub(super) tombstones: TombstoneIndex,
    pub(super) tiebreak_mode: TiebreakMode,
    pub(super) allow_top_k: bool,
    pub(super) repo_root: PathBuf,
}

pub(super) fn governance_engine(
    input: GovernanceEngineInput,
) -> GovernanceEngine<MemorydSimilaritySearch, MemorydTiebreaker, MemorydSessionResolver, ArtifactStore> {
    GovernanceEngine::new(
        input.policies,
        GroundingVerifier::new_with_web_capture_resolver(
            FileSourceResolver,
            MemorydSessionResolver,
            ArtifactStore::new(input.repo_root),
        ),
        input.tombstones,
        GovernanceProviders::new(
            MemorydSimilaritySearch::new(input.active, input.allow_top_k),
            MemorydTiebreaker { tiebreak_mode: input.tiebreak_mode },
        ),
    )
}

/// Upper bound on active-memory envelope reads in flight at once.
///
/// `active_memory_summaries` reads one canonical file per active memory, and
/// each read is a synchronous `std::fs` read + Markdown parse moved onto the
/// blocking pool. Spawning one task per memory with no cap would flood the
/// runtime (the active set is unbounded, unlike search hits which are capped at
/// `SEARCH_LIMIT_MAX`); a fixed window keeps the fan-out wide enough to hide
/// per-read latency while bounding blocking-pool and file-descriptor pressure
/// regardless of corpus size.
const ACTIVE_SUMMARY_READ_CONCURRENCY: usize = 16;

/// Build the active-memory candidate set for governance contradiction / claim-hash
/// matching.
///
/// Index-first: the derived index already knows which memories are `Active` and
/// plaintext, so we ask it for exactly those paths instead of reading and
/// frontmatter-parsing *every* canonical file just to discard non-active /
/// encrypted ones. The candidate set the engine actually needs (claim hash,
/// entity hash, namespace) still requires each memory's body to hash, so we read
/// only the active-plaintext envelopes.
///
/// Each read is a synchronous disk read + Markdown parse, so we move the reads
/// onto the blocking pool via `spawn_blocking` (calling the synchronous
/// `read_path_envelope_blocking`) rather than occupying async worker threads,
/// and gate the fan-out with a semaphore at `ACTIVE_SUMMARY_READ_CONCURRENCY` so
/// a large active set cannot saturate the runtime. Results are reassembled by
/// position so the candidate set order still matches the index query.
///
/// Per-memory derivation (namespace, entity ids, body) is computed from the read
/// envelope's frontmatter exactly as before, so the engine sees an identical
/// candidate set; only its construction moved off the full repo walk.
pub(super) async fn active_memory_summaries(substrate: &Substrate) -> Result<Vec<ExistingMemorySummary>, HandlerError> {
    let active_rows = substrate
        .query_recall_index(RecallIndexQuery {
            statuses: vec![MemoryStatus::Active],
            hydrate: AuxScope::None,
            ..RecallIndexQuery::default()
        })
        .await
        .map_err(HandlerError::substrate)?;

    let semaphore = Arc::new(tokio::sync::Semaphore::new(ACTIVE_SUMMARY_READ_CONCURRENCY));
    let mut reads = tokio::task::JoinSet::new();
    for (position, row) in active_rows.iter().enumerate() {
        let substrate = substrate.clone();
        let path = row.path.clone();
        let semaphore = Arc::clone(&semaphore);
        reads.spawn(async move {
            // Acquire before touching disk so at most
            // `ACTIVE_SUMMARY_READ_CONCURRENCY` reads run at once. The semaphore
            // is never closed, so `acquire_owned` cannot fail.
            let _permit = semaphore.acquire_owned().await.expect("active-summary semaphore is open");
            // The read is a synchronous `std::fs` read + Markdown parse; run it
            // on the blocking pool via the dedicated sync method so it never
            // occupies an async worker thread (works on both the multi-thread
            // daemon runtime and the current-thread test/bench runtimes).
            let envelope = tokio::task::spawn_blocking(move || substrate.read_path_envelope_blocking(&path)).await;
            (position, envelope)
        });
    }

    // Collect into a position-indexed buffer so the candidate set order matches
    // the index query (deterministic by `memories.id`), independent of task
    // completion order.
    let mut buffered: Vec<Option<ExistingMemorySummary>> = (0..active_rows.len()).map(|_| None).collect();
    while let Some(joined) = reads.join_next().await {
        let (position, blocking_result) =
            joined.map_err(|err| HandlerError::substrate(format!("active-memory read task: {err}")))?;
        let envelope =
            blocking_result.map_err(|err| HandlerError::substrate(format!("active-memory read task: {err}")))?;
        let envelope = envelope.map_err(HandlerError::substrate)?;
        // The index row was `Active`; re-confirm against the read envelope and
        // require plaintext content (the encrypted body cannot be hashed), which
        // preserves the prior walk's exact membership filter.
        if !matches!(envelope.metadata.frontmatter.status, MemoryStatus::Active) {
            continue;
        }
        let MemoryContent::Plaintext(body) = envelope.content else {
            continue;
        };
        buffered[position] = Some(
            ExistingMemorySummary::new(
                envelope.metadata.frontmatter.id.as_str().to_string(),
                namespace_for_frontmatter(&envelope.metadata.frontmatter),
                body,
                1.0,
            )
            .with_entity_ids(entity_ids(&envelope.metadata.frontmatter)),
        );
    }

    Ok(buffered.into_iter().flatten().collect())
}

#[derive(Clone, Debug)]
pub(super) struct MemorydSimilaritySearch {
    active: Vec<ExistingMemorySummary>,
    /// Index of the active set keyed by the exact-duplicate match key
    /// `(namespace, claim_hash, entity_hash)`, pointing at the position of the
    /// first occurrence in `active`. Turns `find_active_by_claim_hash` into an
    /// O(1) lookup instead of a linear scan over the whole active set per call,
    /// while preserving the prior "first match by candidate-set order" result —
    /// up to exact-duplicate ties. The candidate-set order itself moved from the
    /// filesystem walk to the index query (deterministic by `memories.id`), so
    /// when more than one active memory shares the exact full triple the *winning
    /// record* may differ from the old walk's pick. That is observationally safe
    /// today: a full-triple collision means the records are true exact duplicates
    /// (the dedup/contradiction decision is the same whichever wins), and the
    /// only surfaced field is `existing_id`, which names a genuine duplicate
    /// either way. A future change that reads order-sensitive *non-key* fields off
    /// the returned summary would need a stable secondary sort (e.g. by id) here.
    by_claim_key: HashMap<(String, String, String), usize>,
    allow_top_k: bool,
}

impl MemorydSimilaritySearch {
    fn new(active: Vec<ExistingMemorySummary>, allow_top_k: bool) -> Self {
        let mut by_claim_key = HashMap::with_capacity(active.len());
        for (position, memory) in active.iter().enumerate() {
            // First occurrence wins, matching the prior `Iterator::find` semantics.
            by_claim_key
                .entry((
                    memory.namespace().to_string(),
                    memory.canonical_claim_hash().to_string(),
                    memory.entity_hash().to_string(),
                ))
                .or_insert(position);
        }
        Self { active, by_claim_key, allow_top_k }
    }
}

impl SimilaritySearch for MemorydSimilaritySearch {
    fn find_active_by_claim_hash(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        let key = (
            candidate.namespace().to_string(),
            candidate.canonical_claim_hash().to_string(),
            candidate.entity_hash().to_string(),
        );
        self.by_claim_key.get(&key).and_then(|&position| self.active.get(position)).cloned()
    }

    fn top_k(&self, _candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        if !self.allow_top_k {
            return Vec::new();
        }
        self.active.iter().take(limit).cloned().collect()
    }
}

#[derive(Clone, Debug)]
pub(super) struct MemorydTiebreaker {
    tiebreak_mode: TiebreakMode,
}

#[derive(Clone, Debug)]
pub(super) enum TiebreakMode {
    Unclear,
    Contradiction { existing_id: String },
}

impl ContradictionTiebreaker for MemorydTiebreaker {
    fn tiebreak(&self, _candidate: &CandidateMemory, _hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        match &self.tiebreak_mode {
            TiebreakMode::Unclear => TiebreakOutcome::Unclear,
            TiebreakMode::Contradiction { existing_id } => {
                TiebreakOutcome::Contradiction { existing_id: existing_id.clone() }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MemorydSessionResolver;

impl SessionSpawnResolver for MemorydSessionResolver {
    fn spawned_in_session(&self, _spawn_id: &str) -> bool {
        false
    }
}
