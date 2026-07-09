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
use memory_substrate::{
    AuxScope, EmbeddingTriple, Memory, MemoryContent, MemoryStatus, RecallIndexQuery, Scope, Sensitivity, Substrate,
    VectorError,
};

use crate::embedding::{is_gemini_api_triple, EmbeddingProviderAcquire, EmbeddingProviderSlot, ProviderGuard};
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
    pub(super) top_k_source: TopKSource,
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
            MemorydSimilaritySearch::new(input.active, input.top_k_source),
            MemorydTiebreaker { tiebreak_mode: input.tiebreak_mode },
        ),
    )
}

/// Where `SimilaritySearch::top_k` draws its candidate set from for a given write.
///
/// The two governance write paths feed the contradiction engine differently:
///
/// - The **write path** runs production embedding-backed KNN: the candidate text
///   is embedded and matched against the active triple's vector table. The hits
///   carry real cosine similarities, so the engine's threshold gate is
///   meaningful. When embedding is unavailable (no provider loaded, triple
///   mismatch, empty vec table) the source is [`TopKSource::Degraded`] — top_k
///   returns nothing and the handler surfaces the degradation in the decision
///   trace (invariant 3: visible, not silent).
///
/// - The **supersede path** (plaintext old) deliberately forces the named old
///   memory into the tiebreaker without embedding anything: it carries
///   [`TopKSource::ActiveSet`], preserving the prior `allow_top_k = true`
///   behavior of returning the (single-element) active set.
#[derive(Clone, Debug)]
pub(super) enum TopKSource {
    /// Embedding-backed KNN hits with real similarities (write path).
    Knn(Vec<ExistingMemorySummary>),
    /// Return the active set directly (supersede path's old-memory forcing).
    ActiveSet,
    /// Embedding similarity was requested but unavailable; top_k yields nothing.
    Degraded,
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
            source_identity: false,
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

/// Outcome of resolving the write-path top-K similarity candidates.
///
/// The handler threads this into both the engine ([`TopKSource`]) and the
/// response: a `Degraded` outcome both makes `top_k` yield nothing *and* leaves
/// a marker in the decision trace so the operator sees that contradiction
/// detection ran without an embedding backend (invariant 3: visible, not silent).
pub(super) struct SimilarityResolution {
    pub(super) source: TopKSource,
    pub(super) degradation: Option<&'static str>,
}

impl SimilarityResolution {
    fn available(hits: Vec<ExistingMemorySummary>) -> Self {
        Self { source: TopKSource::Knn(hits), degradation: None }
    }

    fn degraded(reason: &'static str) -> Self {
        Self { source: TopKSource::Degraded, degradation: Some(reason) }
    }

    /// Similarity detection was not run at all (the legacy stateless handler
    /// path, which carries no embedding backend by design). No top-K candidates
    /// and no degradation marker — this path simply does not participate in
    /// embedding-backed contradiction detection.
    pub(super) fn not_attempted() -> Self {
        Self { source: TopKSource::Knn(Vec::new()), degradation: None }
    }
}

/// Map a governance namespace label (`me`/`project`/`agent`) to the substrate
/// scopes that share it, matching `namespace_for_frontmatter`'s inverse.
fn scopes_for_namespace(namespace: &str) -> Vec<Scope> {
    match namespace {
        "me" => vec![Scope::User],
        "project" => vec![Scope::Project, Scope::Org],
        "agent" => vec![Scope::Agent, Scope::Subagent],
        _ => Vec::new(),
    }
}

/// Resolve the write-path top-K similarity candidates by embedding the candidate
/// text and running KNN against the active embedding triple's vector table.
///
/// ## Embed side: `embed_query`
///
/// The asymmetric Qwen3 pair embeds *queries* with the model-card instruction
/// prompt and *documents* plainly. A write candidate compared against the corpus
/// of already-embedded memories is the *query* side of that pair — the stored
/// memory chunks were embedded with `embed_document` by the drain worker, so the
/// candidate must use `embed_query` to land in the same asymmetric space the
/// retrieval was tuned for. (Collapsing the two measurably degrades retrieval;
/// that is the whole point of the asymmetric contract.)
///
/// ## Degradation (invariant 3)
///
/// Any condition that means "no real similarity backend" returns
/// [`SimilarityResolution::degraded`] with a stable reason rather than a silent
/// empty set:
/// - API-lane candidate text is not conclusively plaintext-eligible,
/// - no provider loaded (model not up / load failed / worker disabled),
/// - the provider's triple disagrees with the substrate's active triple,
/// - the active triple has no vector table yet (`UnknownEmbeddingTriple`),
/// - embedding inference itself failed.
///
/// A genuinely-empty namespace (provider up, triple matches, but no in-scope
/// neighbours) is *not* degraded — it is a real "no conflict" answer.
#[allow(clippy::too_many_arguments)]
pub(super) async fn resolve_similarity_candidates(
    substrate: &Substrate,
    provider_slot: &EmbeddingProviderSlot,
    candidate_body: &str,
    namespace: &str,
    candidate_sensitivity: Option<Sensitivity>,
    active: &[ExistingMemorySummary],
    limit: usize,
) -> SimilarityResolution {
    let active_triple = match active_triple_or_degradation(substrate.active_embedding_triple()) {
        Ok(triple) => triple,
        Err(degradation) => return degradation,
    };
    if is_gemini_api_triple(&active_triple)
        && !candidate_sensitivity.is_some_and(|sensitivity| sensitivity.api_lane_eligible())
    {
        return SimilarityResolution::degraded("similarity_degraded:sensitive_held_local");
    }

    let scopes = scopes_for_namespace(namespace);
    if scopes.is_empty() {
        // No resolvable scope → no in-scope neighbours by definition. Treated as
        // a real (empty) answer, not a backend degradation.
        return SimilarityResolution::available(Vec::new());
    }

    // Embed off the async runtime: candle compute blocks. Candidate text is the
    // *query* side of the asymmetric pair (see doc comment).
    let body = candidate_body.to_string();
    let guard = match provider_slot.acquire_or_trigger_load() {
        EmbeddingProviderAcquire::Active(guard) => guard,
        EmbeddingProviderAcquire::Dormant | EmbeddingProviderAcquire::Loading => {
            return SimilarityResolution::degraded("similarity_degraded:embedding_dormant");
        }
        EmbeddingProviderAcquire::Failed { .. } => {
            return SimilarityResolution::degraded("similarity_degraded:no_embedding_provider");
        }
    };
    if guard.triple() != &active_triple {
        return degraded_guard_triple_mismatch(&guard, &active_triple);
    }
    let vector = match embed_candidate_body_guard(guard, body).await {
        Ok(vector) => vector,
        Err(degradation) => return degradation,
    };

    let neighbours = match substrate.knn_active_memories(&active_triple, &vector, &scopes, limit).await {
        Ok(neighbours) => neighbours,
        Err(memory_substrate::VectorError::UnknownEmbeddingTriple(_)) => {
            // Vec table absent/dropped — embedding is configured but the corpus
            // has nothing indexed against this triple yet.
            return SimilarityResolution::degraded("similarity_degraded:no_vector_table");
        }
        Err(error) => {
            tracing::warn!(%error, "contradiction similarity degraded: KNN query failed");
            return SimilarityResolution::degraded("similarity_degraded:knn_failed");
        }
    };

    // Map each KNN neighbour back to its already-computed active summary (claim /
    // entity hashes, namespace) and re-stamp the measured similarity. Neighbours
    // absent from the active set (e.g. a row indexed but not yet in the active
    // snapshot) are skipped rather than fabricated.
    let by_id: HashMap<&str, &ExistingMemorySummary> = active.iter().map(|summary| (summary.id(), summary)).collect();
    let hits = neighbours
        .into_iter()
        .filter_map(|neighbour| {
            by_id
                .get(neighbour.memory_id.as_str())
                .map(|summary| (*summary).clone().with_similarity(neighbour.similarity))
        })
        .collect::<Vec<_>>();
    SimilarityResolution::available(hits)
}

fn degraded_triple_mismatch(
    provider_triple: &EmbeddingTriple,
    active_triple: &EmbeddingTriple,
) -> SimilarityResolution {
    tracing::warn!(
        provider_triple = ?provider_triple,
        active_triple = ?active_triple,
        "contradiction similarity degraded: provider triple does not match active triple"
    );
    SimilarityResolution::degraded("similarity_degraded:triple_mismatch")
}

fn degraded_guard_triple_mismatch(provider: &ProviderGuard, active_triple: &EmbeddingTriple) -> SimilarityResolution {
    degraded_triple_mismatch(provider.triple(), active_triple)
}

async fn embed_candidate_body_guard(provider: ProviderGuard, body: String) -> Result<Vec<f32>, SimilarityResolution> {
    match tokio::task::spawn_blocking(move || provider.embed_query(&body)).await {
        Ok(Ok(vector)) => Ok(vector),
        Ok(Err(error)) => {
            tracing::warn!(%error, "contradiction similarity degraded: candidate embedding failed");
            Err(SimilarityResolution::degraded("similarity_degraded:embedding_failed"))
        }
        Err(join_error) => {
            tracing::warn!(%join_error, "contradiction similarity degraded: embedding task panicked");
            Err(SimilarityResolution::degraded("similarity_degraded:embedding_failed"))
        }
    }
}

fn active_triple_or_degradation(
    result: Result<EmbeddingTriple, VectorError>,
) -> Result<EmbeddingTriple, SimilarityResolution> {
    match result {
        Ok(triple) => Ok(triple),
        Err(error) => {
            tracing::warn!(%error, "contradiction similarity degraded: cannot read active embedding triple");
            Err(SimilarityResolution::degraded("similarity_degraded:no_active_triple"))
        }
    }
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
    top_k_source: TopKSource,
}

impl MemorydSimilaritySearch {
    fn new(active: Vec<ExistingMemorySummary>, top_k_source: TopKSource) -> Self {
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
        Self { active, by_claim_key, top_k_source }
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
        match &self.top_k_source {
            // KNN hits arrive pre-sorted by descending similarity from the
            // substrate query; truncate to the engine's width.
            TopKSource::Knn(hits) => hits.iter().take(limit).cloned().collect(),
            // Supersede path: surface the (single) active old memory so the
            // explicit-supersede tiebreaker can force a contradiction.
            TopKSource::ActiveSet => self.active.iter().take(limit).cloned().collect(),
            // Embedding unavailable: no similarity candidates. The handler has
            // already recorded the degradation in the decision trace.
            TopKSource::Degraded => Vec::new(),
        }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use memory_privacy::FileKeyProvider;
    use memory_substrate::{InitOptions, MemoryContent, MemoryId, Roots};
    use serde_json::json;

    use super::*;
    use crate::embedding::{
        api_test_support::{MockGeminiServer, MockResponse},
        ApiEmbeddingProvider, EmbeddingError, EmbeddingIdleWindow, EmbeddingProvider, FixtureProvider,
        GEMINI_API_PROVIDER,
    };
    use crate::handlers::{handle_request_with_state, HandlerState};
    use crate::protocol::{GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

    const TEST_PROJECT_CANONICAL_ID: &str = "proj_governance_api_lane_fence";
    const TEST_PROJECT_ALIAS: &str = "governance-api-lane-fence";

    async fn init_substrate() -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate =
            Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_policy".into()) })
                .await
                .expect("init substrate");
        (temp, substrate)
    }

    struct StubProvider {
        triple: EmbeddingTriple,
        query: QueryBehavior,
    }

    enum QueryBehavior {
        Vector(Vec<f32>),
        Error,
    }

    impl EmbeddingProvider for StubProvider {
        fn triple(&self) -> &EmbeddingTriple {
            &self.triple
        }

        fn embed_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            match &self.query {
                QueryBehavior::Vector(vector) => Ok(vector.clone()),
                QueryBehavior::Error => Err(EmbeddingError::Inference("forced query failure".to_string())),
            }
        }

        fn embed_document(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(vec![0.0; self.triple.dimension as usize])
        }
    }

    async fn degradation_for_provider(
        substrate: &Substrate,
        provider: Arc<dyn EmbeddingProvider>,
    ) -> Option<&'static str> {
        let slot = EmbeddingProviderSlot::empty();
        slot.set(provider);
        resolve_similarity_candidates(
            substrate,
            &slot,
            "candidate claim",
            "project",
            Some(Sensitivity::Internal),
            &[],
            5,
        )
        .await
        .degradation
    }

    #[test]
    fn active_triple_read_error_maps_to_no_active_triple_marker() {
        let result = active_triple_or_degradation(Err(VectorError::IndexUnavailable("poisoned".to_string())));
        let degradation = result.expect_err("degraded").degradation;
        assert_eq!(degradation, Some("similarity_degraded:no_active_triple"));
    }

    #[tokio::test]
    async fn triple_mismatch_maps_to_degraded_marker() {
        let (_temp, substrate) = init_substrate().await;
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::synthetic_test_triple());

        assert_eq!(degradation_for_provider(&substrate, provider).await, Some("similarity_degraded:triple_mismatch"));
    }

    #[tokio::test]
    async fn no_vector_table_maps_to_degraded_marker() {
        let (_temp, substrate) = init_substrate().await;
        let provider: Arc<dyn EmbeddingProvider> =
            Arc::new(FixtureProvider::new(substrate.active_embedding_triple().expect("triple")));

        assert_eq!(degradation_for_provider(&substrate, provider).await, Some("similarity_degraded:no_vector_table"));
    }

    #[tokio::test]
    async fn embedding_failure_maps_to_degraded_marker() {
        let (_temp, substrate) = init_substrate().await;
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(StubProvider {
            triple: substrate.active_embedding_triple().expect("triple"),
            query: QueryBehavior::Error,
        });

        assert_eq!(degradation_for_provider(&substrate, provider).await, Some("similarity_degraded:embedding_failed"));
    }

    #[tokio::test]
    async fn knn_failure_maps_to_degraded_marker() {
        let (_temp, substrate) = init_substrate().await;
        let triple = substrate.active_embedding_triple().expect("triple");
        let provider: Arc<dyn EmbeddingProvider> =
            Arc::new(StubProvider { triple, query: QueryBehavior::Vector(vec![0.0; 1]) });

        assert_eq!(degradation_for_provider(&substrate, provider).await, Some("similarity_degraded:knn_failed"));
    }

    #[tokio::test]
    async fn local_lane_does_not_apply_api_sensitive_candidate_fence() {
        let (_temp, substrate) = init_substrate().await;
        let slot = EmbeddingProviderSlot::empty();
        slot.set(Arc::new(FixtureProvider::new(substrate.active_embedding_triple().expect("triple"))));

        let degradation = resolve_similarity_candidates(&substrate, &slot, "candidate claim", "project", None, &[], 5)
            .await
            .degradation;

        assert_ne!(degradation, Some("similarity_degraded:sensitive_held_local"));
    }

    #[tokio::test]
    async fn api_lane_confidential_governed_write_degrades_without_http_and_writes_ciphertext() {
        let (_temp, substrate) = init_substrate_with_active_triple(api_triple()).await;
        FileKeyProvider::runtime_default(&substrate.roots().runtime).onboard_local_file().expect("privacy key");
        let server = MockGeminiServer::panic_on_any_request();
        let state = HandlerState::new();
        state.embedding_provider_slot().set_idle_window_for_tests(EmbeddingIdleWindow::from_duration(None, "test"));
        publish_api_provider(&state, server.base_url()).await;

        let write = write_project_memory(
            &substrate,
            &state,
            "confidential-api-fence",
            "confidential acquisition note",
            "The confidential acquisition target for this project is Northstar.",
            "confidential",
        )
        .await;

        assert_eq!(write.status, GovernanceStatus::Promoted, "sensitive write must complete, not refuse");
        assert_eq!(
            write.similarity_degraded.as_deref(),
            Some("similarity_degraded:sensitive_held_local"),
            "API-lane sensitive candidate must degrade before embedding"
        );
        assert!(server.requests().is_empty(), "sensitive candidate must make zero Gemini HTTP requests");
        let id = write.id.expect("promoted encrypted write id");
        let envelope = substrate.read_memory_envelope(&MemoryId::new(&id)).await.expect("read encrypted envelope");
        assert!(matches!(envelope.content, MemoryContent::Ciphertext { .. }), "sensitive write must be encrypted");
        assert_eq!(envelope.metadata.frontmatter.sensitivity, Sensitivity::Confidential);
        drop_api_test_resources(state, server).await;
    }

    #[tokio::test]
    async fn api_lane_public_governed_write_embeds_candidate_claim() {
        let (_temp, substrate) = init_substrate_with_active_triple(api_triple()).await;
        let server = MockGeminiServer::new(vec![MockResponse::json(200, embedding_response(vec![vec![1.0, 0.0]]))]);
        let state = HandlerState::new();
        state.embedding_provider_slot().set_idle_window_for_tests(EmbeddingIdleWindow::from_duration(None, "test"));
        publish_api_provider(&state, server.base_url()).await;

        let write = write_project_memory(
            &substrate,
            &state,
            "public-api-fence",
            "public API lane note",
            "The public API lane test canary for this project is Atlas.",
            "public",
        )
        .await;

        assert_eq!(write.status, GovernanceStatus::Promoted, "public write must complete");
        let requests = server.requests();
        assert_eq!(requests.len(), 1, "public candidate should embed exactly once under the API lane");
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/models/gemini-embedding-2:embedContent");
        assert!(
            requests[0].body.contains("The public API lane test canary for this project is Atlas."),
            "request should be the candidate query embedding, got {}",
            requests[0].body
        );
        drop_api_test_resources(state, server).await;
    }

    async fn init_substrate_with_active_triple(triple: EmbeddingTriple) -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo dir");
        std::fs::write(
            repo.join("config.yaml"),
            format!(
                "schema_version: 1\nactive_embedding:\n  provider: {}\n  model_ref: {}\n  dimension: {}\n",
                triple.provider, triple.model_ref, triple.dimension
            ),
        )
        .expect("write config");
        let substrate = Substrate::init(
            Roots::new(repo, runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_governanceapifence".into()) },
        )
        .await
        .expect("init substrate");
        (temp, substrate)
    }

    fn api_triple() -> EmbeddingTriple {
        EmbeddingTriple {
            provider: GEMINI_API_PROVIDER.to_string(),
            model_ref: "gemini-embedding-2".to_string(),
            dimension: 2,
        }
    }

    fn api_provider(base_url: String) -> Arc<dyn EmbeddingProvider> {
        Arc::new(ApiEmbeddingProvider::new_for_test(api_triple(), "test-api-key", base_url).expect("api provider"))
    }

    async fn publish_api_provider(state: &HandlerState, base_url: String) {
        let provider = tokio::task::spawn_blocking(move || api_provider(base_url))
            .await
            .expect("construct API provider off async runtime");
        state.embedding_provider_slot().set(provider);
    }

    fn embedding_response(vectors: Vec<Vec<f32>>) -> String {
        json!({
            "embeddings": vectors.into_iter().map(|values| json!({ "values": values })).collect::<Vec<_>>()
        })
        .to_string()
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_project_memory(
        substrate: &Substrate,
        state: &HandlerState,
        request_id: &str,
        summary: &str,
        body: &str,
        sensitivity: &str,
    ) -> crate::protocol::GovernanceWriteResponse {
        let response = handle_request_with_state(
            substrate,
            state,
            RequestEnvelope::new(
                request_id,
                RequestPayload::WriteMemory {
                    body: body.to_string(),
                    title: Some(summary.to_string()),
                    tags: vec!["api-lane-fence".to_string()],
                    meta: json!({
                        "namespace": "project",
                        "type": "claim",
                        "summary": summary,
                        "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                        "namespace_alias": TEST_PROJECT_ALIAS,
                        "confidence": 0.95,
                        "sensitivity": sensitivity,
                        "source_kind": "user",
                        "explicit_user_context": true
                    }),
                },
            ),
        )
        .await;

        match response.result {
            ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write,
            other => panic!("expected governed write success, got {other:?}"),
        }
    }

    async fn drop_api_test_resources(state: HandlerState, server: MockGeminiServer) {
        tokio::task::spawn_blocking(move || {
            drop(state);
            drop(server);
        })
        .await
        .expect("drop API test resources off async runtime");
    }
}
