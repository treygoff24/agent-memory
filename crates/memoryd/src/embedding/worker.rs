//! Background embedding worker: drains `pending_embedding_jobs` into vectors.
//!
//! Every write enqueues a pending embedding job per chunk against the active
//! triple. This task is the consumer: it embeds each pending chunk with the
//! configured [`EmbeddingProvider`] and writes the vector via
//! `Substrate::update_embedding`, which resolves the job. Without it the backlog
//! grows unbounded and vector search stays empty.
//!
//! ## Stale-job safety
//!
//! [`Substrate::pending_embedding_jobs`] only returns jobs whose `content_hash`
//! still matches the live chunk `body_hash`, and `update_embedding` re-checks
//! the same hash at commit. A chunk edited between drain and write is rejected
//! with `StaleChunk` and left for `reconcile_active_embedding_jobs` to re-enqueue
//! — never written for content that no longer exists.
//!
//! ## Provider/triple agreement
//!
//! The provider's triple must equal the substrate's active triple; otherwise the
//! worker would embed against one model and write into another table. A mismatch
//! is logged once and the worker stays idle rather than corrupting the index
//! (invariant 3: triple is identity, never silent fallback).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use memory_substrate::{EmbeddingTriple, EmbeddingUpdate, PendingEmbeddingJob, Substrate, VectorError};
use tokio::sync::watch;
use tokio::time::sleep;

use super::{EmbeddingError, EmbeddingProvider, EmbeddingProviderAcquire, EmbeddingProviderSlot};

/// How many jobs to pull and embed per drain tick. Bounded so one tick cannot
/// monopolize a blocking thread for an unbounded backlog; the next tick picks up
/// the remainder.
const DRAIN_BATCH: usize = 64;

/// Default interval between drain ticks when the queue is empty.
const IDLE_INTERVAL: Duration = Duration::from_secs(5);

/// Per-process retry budget for a poisoned pending job. Exhausted jobs are left
/// pending on disk and naturally retry after daemon restart, but this process
/// skips them so newer jobs behind the head of queue can drain.
const MAX_JOB_ATTEMPTS: u32 = 5;

/// Cap for the zero-success drain backoff.
const MAX_ZERO_SUCCESS_BACKOFF: Duration = Duration::from_secs(300);

static EXHAUSTED_RETRY_BUDGET_JOBS: AtomicUsize = AtomicUsize::new(0);

/// Number of jobs this process has skipped after exhausting the in-memory retry
/// budget. Exposed to `doctor` as an advisory.
pub fn exhausted_retry_budget_job_count() -> usize {
    EXHAUSTED_RETRY_BUDGET_JOBS.load(Ordering::Relaxed)
}

/// Spawn the embedding drain loop. Returns immediately; the loop runs until
/// `shutdown` flips to `true` or the sender drops.
pub fn spawn_embedding_worker(
    substrate: Arc<Substrate>,
    provider_slot: EmbeddingProviderSlot,
    shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(substrate, provider_slot, shutdown, IDLE_INTERVAL))
}

#[doc(hidden)]
pub fn spawn_embedding_worker_with_interval(
    substrate: Arc<Substrate>,
    provider_slot: EmbeddingProviderSlot,
    shutdown: watch::Receiver<bool>,
    idle_interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(substrate, provider_slot, shutdown, idle_interval))
}

async fn run(
    substrate: Arc<Substrate>,
    provider_slot: EmbeddingProviderSlot,
    mut shutdown: watch::Receiver<bool>,
    idle_interval: Duration,
) {
    let mut retry_budget = JobRetryBudget::default();
    let mut zero_success_backoff = idle_interval;
    let mut next_delay = idle_interval;

    loop {
        if sleep_or_shutdown(&mut shutdown, next_delay).await {
            return;
        }
        next_delay = idle_interval;

        // Drain until the queue is empty or a batch comes back short, so a
        // large backlog after import clears without waiting one idle interval
        // per successful batch.
        loop {
            if *shutdown.borrow() {
                return;
            }
            match substrate.pending_embedding_job_count(memory_substrate::EmbeddingLaneEligibility::AllTiers) {
                Ok(0) => {
                    zero_success_backoff = idle_interval;
                    break;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        %error,
                        sleep_ms = zero_success_backoff.as_millis(),
                        "embedding pending-job count failed; backing off"
                    );
                    next_delay = zero_success_backoff;
                    zero_success_backoff = grow_backoff(zero_success_backoff);
                    break;
                }
            }
            if let Err(error) = provider_slot.ensure_loaded().await {
                tracing::warn!(
                    %error,
                    retry_seconds = super::lifecycle::MODEL_LOAD_RETRY_BACKOFF.as_secs(),
                    "embedding worker model load failed; recall stays FTS-only until retry succeeds"
                );
                next_delay = super::lifecycle::MODEL_LOAD_RETRY_BACKOFF;
                break;
            }
            let guard = match provider_slot.acquire() {
                EmbeddingProviderAcquire::Active(guard) => guard,
                EmbeddingProviderAcquire::Dormant | EmbeddingProviderAcquire::Loading => {
                    next_delay = Duration::from_millis(100);
                    break;
                }
                EmbeddingProviderAcquire::Failed { last_error } => {
                    tracing::warn!(
                        error = ?last_error,
                        retry_seconds = super::lifecycle::MODEL_LOAD_RETRY_BACKOFF.as_secs(),
                        "embedding worker provider unavailable after load attempt"
                    );
                    next_delay = super::lifecycle::MODEL_LOAD_RETRY_BACKOFF;
                    break;
                }
            };
            match drain_batch_with_budget(&substrate, guard.provider(), DRAIN_BATCH, &mut retry_budget).await {
                Ok(outcome) if outcome.fetched == 0 => {
                    zero_success_backoff = idle_interval;
                    break;
                }
                Ok(outcome) if outcome.succeeded > 0 => {
                    zero_success_backoff = idle_interval;
                    if outcome.fetched < outcome.requested {
                        break;
                    }
                    continue;
                }
                Ok(outcome) => {
                    tracing::warn!(
                        fetched = outcome.fetched,
                        exhausted_jobs = retry_budget.exhausted_count(),
                        sleep_ms = zero_success_backoff.as_millis(),
                        "embedding drain made no successful progress; backing off"
                    );
                    next_delay = zero_success_backoff;
                    zero_success_backoff = grow_backoff(zero_success_backoff);
                    break;
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        sleep_ms = zero_success_backoff.as_millis(),
                        "embedding drain tick failed; backing off"
                    );
                    next_delay = zero_success_backoff;
                    zero_success_backoff = grow_backoff(zero_success_backoff);
                    break;
                }
            }
        }
    }
}

async fn sleep_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        biased;
        _ = shutdown.changed() => true,
        _ = sleep(duration) => false,
    }
}

fn grow_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_ZERO_SUCCESS_BACKOFF)
}

#[derive(Debug, Default)]
struct JobRetryBudget {
    attempts_by_chunk: HashMap<String, u32>,
}

impl JobRetryBudget {
    fn exhausted_count(&self) -> usize {
        self.attempts_by_chunk.values().filter(|&&attempts| attempts >= MAX_JOB_ATTEMPTS).count()
    }

    fn is_exhausted(&self, chunk_id: &str) -> bool {
        self.attempts_by_chunk.get(chunk_id).is_some_and(|attempts| *attempts >= MAX_JOB_ATTEMPTS)
    }

    fn record_success(&mut self, chunk_id: &str) {
        self.attempts_by_chunk.remove(chunk_id);
    }

    fn record_failure(&mut self, chunk_id: &str, error: impl std::fmt::Display) {
        let attempts = self.attempts_by_chunk.entry(chunk_id.to_string()).or_insert(0);
        *attempts = attempts.saturating_add(1);
        if *attempts >= MAX_JOB_ATTEMPTS {
            EXHAUSTED_RETRY_BUDGET_JOBS.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                %chunk_id,
                attempts = *attempts,
                %error,
                "embedding job exhausted retry budget; skipping until daemon restart"
            );
        } else {
            tracing::debug!(
                %chunk_id,
                attempts = *attempts,
                max_attempts = MAX_JOB_ATTEMPTS,
                %error,
                "embedding job failed; leaving pending for retry"
            );
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DrainOutcome {
    requested: usize,
    fetched: usize,
    succeeded: usize,
}

/// Embed and write up to `limit` pending jobs for the active triple, returning
/// how many jobs succeeded.
///
/// The unit of drain work, exposed for the e2e test and any future
/// operator-triggered drain. Each job is embedded off-runtime via
/// `spawn_blocking` and written with the chunk's enqueue-time `content_hash` as
/// `expected_chunk_hash`, so a chunk edited mid-flight is rejected as stale
/// rather than written for content that changed.
pub async fn drain_batch(
    substrate: &Substrate,
    provider: &Arc<dyn EmbeddingProvider>,
    limit: usize,
) -> Result<usize, String> {
    let mut retry_budget = JobRetryBudget::default();
    drain_batch_with_budget(substrate, provider, limit, &mut retry_budget).await.map(|outcome| outcome.succeeded)
}

async fn drain_batch_with_budget(
    substrate: &Substrate,
    provider: &Arc<dyn EmbeddingProvider>,
    limit: usize,
    retry_budget: &mut JobRetryBudget,
) -> Result<DrainOutcome, String> {
    let requested = limit.saturating_add(retry_budget.exhausted_count());
    let jobs = substrate
        .pending_embedding_jobs(requested, memory_substrate::EmbeddingLaneEligibility::AllTiers)
        .await
        .map_err(|err| err.to_string())?;
    if jobs.is_empty() {
        return Ok(DrainOutcome { requested, fetched: 0, succeeded: 0 });
    }
    let fetched = jobs.len();
    let triple = provider.triple().clone();
    let mut succeeded = 0usize;

    // Partition out jobs whose chunk has already exhausted its retry budget; they
    // are skipped without an embed pass, exactly as the per-job loop did.
    let live_jobs: Vec<PendingEmbeddingJob> = jobs
        .into_iter()
        .filter(|job| {
            if retry_budget.is_exhausted(&job.chunk_id) {
                tracing::debug!(chunk_id = %job.chunk_id, "embedding job skipped after retry budget exhaustion");
                false
            } else {
                true
            }
        })
        .collect();
    if live_jobs.is_empty() {
        return Ok(DrainOutcome { requested, fetched, succeeded });
    }

    // Embed the whole live batch in one off-runtime forward pass. candle compute
    // is blocking and would otherwise stall the tokio worker; `spawn_blocking`
    // requires `'static`, satisfied by the cloned Arc + owned texts. Batching
    // amortizes the transformer matmuls over the slice — several times faster per
    // item than one `embed_document` call per chunk — and the per-item vectors
    // are byte-identical to the per-text path, so stale-chunk keying is preserved.
    let embed_provider = Arc::clone(provider);
    let texts: Vec<String> = live_jobs.iter().map(|job| job.text.clone()).collect();
    let vectors = match tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        embed_provider.embed_documents(&refs)
    })
    .await
    {
        Ok(Ok(vectors)) => vectors,
        // A batch-level inference error (`Ok(Err)`) or a panic in the batched
        // forward pass (`Err(join_error)`) both fall back to embedding each job
        // individually, so only chunks that fail on their own burn retry budget —
        // a single poisoned input never fails the whole batch toward exhaustion.
        Ok(Err(_)) | Err(_) => {
            succeeded =
                succeeded.saturating_add(embed_jobs_individually(provider, live_jobs, substrate, retry_budget).await?);
            return Ok(DrainOutcome { requested, fetched, succeeded });
        }
    };
    if vectors.len() != live_jobs.len() {
        let error = EmbeddingError::Inference(format!(
            "model returned {} vectors for {} inputs",
            vectors.len(),
            live_jobs.len()
        ));
        for job in &live_jobs {
            retry_budget.record_failure(&job.chunk_id, &error);
        }
        return Ok(DrainOutcome { requested, fetched, succeeded });
    }

    let pairs: Vec<(PendingEmbeddingJob, Vec<f32>)> = live_jobs.into_iter().zip(vectors).collect();
    succeeded =
        succeeded.saturating_add(write_and_record_embedded_jobs(substrate, &triple, pairs, retry_budget).await?);
    Ok(DrainOutcome { requested, fetched, succeeded })
}

/// Per-job fallback for when the batched forward pass fails wholesale (either a
/// model inference error or a panic in `spawn_blocking`). Embeds each job on its
/// own so a single bad input only burns its own retry budget, then writes the
/// survivors and returns how many succeeded.
async fn embed_jobs_individually(
    provider: &Arc<dyn EmbeddingProvider>,
    jobs: Vec<PendingEmbeddingJob>,
    substrate: &Substrate,
    retry_budget: &mut JobRetryBudget,
) -> Result<usize, String> {
    let triple = provider.triple().clone();
    let mut surviving = Vec::new();
    for job in jobs {
        let embed_provider = Arc::clone(provider);
        let text = job.text.clone();
        match tokio::task::spawn_blocking(move || embed_provider.embed_document(&text)).await {
            Ok(Ok(vector)) => surviving.push((job, vector)),
            Ok(Err(individual_error)) => retry_budget.record_failure(&job.chunk_id, &individual_error),
            Err(join_error) => retry_budget.record_failure(&job.chunk_id, &join_error),
        }
    }
    write_and_record_embedded_jobs(substrate, &triple, surviving, retry_budget).await
}

async fn write_and_record_embedded_jobs(
    substrate: &Substrate,
    triple: &EmbeddingTriple,
    pairs: Vec<(PendingEmbeddingJob, Vec<f32>)>,
    retry_budget: &mut JobRetryBudget,
) -> Result<usize, String> {
    if pairs.is_empty() {
        return Ok(0);
    }
    let updates: Vec<EmbeddingUpdate> = pairs
        .iter()
        .map(|(job, vector)| EmbeddingUpdate {
            chunk_id: job.chunk_id.clone(),
            expected_chunk_hash: job.content_hash.clone(),
            triple: triple.clone(),
            vector: vector.clone(),
        })
        .collect();
    let jobs: Vec<&PendingEmbeddingJob> = pairs.iter().map(|(job, _)| job).collect();
    let write_results = substrate.update_embeddings_batch(updates).await.map_err(|err| err.to_string())?;
    let mut succeeded = 0usize;
    for (job, result) in jobs.into_iter().zip(write_results) {
        let chunk_id = job.chunk_id.as_str();
        match result {
            Ok(()) => {
                succeeded = succeeded.saturating_add(1);
                retry_budget.record_success(chunk_id);
            }
            Err(VectorError::StaleChunk { .. }) => {
                // StaleChunk here is benign: the chunk changed between drain and
                // write; reconcile re-enqueues with the new content hash.
                retry_budget.record_success(chunk_id);
                tracing::debug!(%chunk_id, "embedding write skipped for stale chunk");
            }
            Err(error) => {
                retry_budget.record_failure(chunk_id, error);
            }
        }
    }
    Ok(succeeded)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use memory_substrate::{InitOptions, Roots, Substrate};

    use super::*;
    use crate::embedding::FixtureProvider;
    use crate::handlers::{handle_request_with_state, HandlerState};
    use crate::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

    async fn init_substrate() -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate =
            Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".into()) })
                .await
                .expect("init");
        (temp, substrate)
    }

    async fn write_project_memory(substrate: &Substrate, summary: &str, body: &str) -> String {
        let response = handle_request_with_state(
            substrate,
            &HandlerState::new(),
            RequestEnvelope::new(
                "worker-test-write",
                RequestPayload::WriteMemory {
                    body: body.to_string(),
                    title: Some(summary.to_string()),
                    tags: Vec::new(),
                    meta: serde_json::json!({
                        "namespace": "project",
                        "type": "claim",
                        "summary": summary,
                        "canonical_namespace_id": "proj_embedding_worker_test",
                        "namespace_alias": "embedding-worker-test",
                        "confidence": 0.95,
                        "sensitivity": "internal",
                        "source_kind": "user",
                        "explicit_user_context": true
                    }),
                },
            ),
        )
        .await;
        match response.result {
            ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write.id.expect("write id"),
            other => panic!("expected governed write success, got {other:?}"),
        }
    }

    struct PoisonProvider {
        triple: memory_substrate::EmbeddingTriple,
    }

    impl EmbeddingProvider for PoisonProvider {
        fn triple(&self) -> &memory_substrate::EmbeddingTriple {
            &self.triple
        }

        fn embed_query(&self, text: &str) -> Result<Vec<f32>, crate::embedding::EmbeddingError> {
            self.embed_document(text)
        }

        fn embed_document(&self, text: &str) -> Result<Vec<f32>, crate::embedding::EmbeddingError> {
            if text.contains("poison") {
                Err(crate::embedding::EmbeddingError::Inference("deterministic poison".to_string()))
            } else {
                Ok(vec![1.0; self.triple.dimension as usize])
            }
        }
    }

    #[tokio::test]
    async fn drain_batch_is_noop_on_empty_queue() {
        let (_temp, substrate) = init_substrate().await;
        let provider: Arc<dyn EmbeddingProvider> =
            Arc::new(FixtureProvider::new(substrate.active_embedding_triple().expect("triple")));
        assert_eq!(drain_batch(&substrate, &provider, 64).await.expect("drain"), 0);
    }

    #[tokio::test]
    async fn worker_disables_on_triple_mismatch() {
        let (_temp, substrate) = init_substrate().await;
        write_project_memory(
            &substrate,
            "mismatched embedding worker fixture",
            "The mismatched embedding worker fixture should remain pending.",
        )
        .await;
        let substrate = Arc::new(substrate);

        // Fixture triple `synthetic/stream-a-test/32` deliberately != the active
        // production triple. The lifecycle should reject it before any drain.
        let mismatched: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::synthetic_test_triple());
        let provider_slot = EmbeddingProviderSlot::empty();
        provider_slot.configure_loader(
            substrate.active_embedding_triple().expect("triple"),
            crate::embedding::EmbeddingIdleWindow::from_duration(Some(Duration::from_secs(60)), "test"),
            move || Ok(Arc::clone(&mismatched)),
        );

        let (tx, rx) = watch::channel(false);
        let worker = tokio::spawn(run(Arc::clone(&substrate), provider_slot.clone(), rx, Duration::from_millis(10)));

        let mut saw_failure = false;
        for _ in 0..50 {
            if provider_slot.snapshot().state == "failed" {
                saw_failure = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tx.send(true).expect("shutdown worker");
        worker.await.expect("worker joins");

        assert!(saw_failure, "mismatched provider should fail lifecycle load");
        assert_eq!(
            substrate
                .pending_embedding_job_count(memory_substrate::EmbeddingLaneEligibility::AllTiers)
                .expect("pending count"),
            1
        );
    }

    #[tokio::test]
    async fn exhausted_retry_budget_skips_poisoned_head_and_drains_later_jobs() {
        let (_temp, substrate) = init_substrate().await;
        write_project_memory(
            &substrate,
            "poisoned embedding worker fixture",
            "The poisoned embedding worker fixture must trigger poison deterministic embedding failure.",
        )
        .await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        write_project_memory(
            &substrate,
            "healthy embedding worker fixture",
            "The healthy embedding worker fixture should drain behind the failing head.",
        )
        .await;

        let triple = substrate.active_embedding_triple().expect("triple");
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(PoisonProvider { triple: triple.clone() });
        let mut retry_budget = JobRetryBudget::default();
        let exhausted_before = exhausted_retry_budget_job_count();

        for _ in 0..MAX_JOB_ATTEMPTS {
            let outcome =
                drain_batch_with_budget(&substrate, &provider, 1, &mut retry_budget).await.expect("drain poison");
            assert_eq!(outcome.succeeded, 0);
        }
        assert_eq!(retry_budget.exhausted_count(), 1);
        assert!(
            exhausted_retry_budget_job_count() > exhausted_before,
            "budget exhaustion should be surfaced to doctor"
        );

        let outcome =
            drain_batch_with_budget(&substrate, &provider, 1, &mut retry_budget).await.expect("drain behind poison");
        assert_eq!(outcome.succeeded, 1, "later job should drain after poisoned head is skipped");
        assert_eq!(substrate.vector_count(triple).await.expect("vector count"), 1);
    }
}
