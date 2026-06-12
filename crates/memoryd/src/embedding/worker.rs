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

use memory_substrate::{EmbeddingUpdate, PendingEmbeddingJob, Substrate, VectorError};
use tokio::sync::watch;
use tokio::time::sleep;

use super::EmbeddingProvider;

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
    provider: Arc<dyn EmbeddingProvider>,
    shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(substrate, provider, shutdown, IDLE_INTERVAL))
}

async fn run(
    substrate: Arc<Substrate>,
    provider: Arc<dyn EmbeddingProvider>,
    mut shutdown: watch::Receiver<bool>,
    idle_interval: Duration,
) {
    // Verify the provider matches the active triple before draining anything.
    match substrate.active_embedding_triple() {
        Ok(active) if &active == provider.triple() => {}
        Ok(active) => {
            tracing::error!(
                provider_triple = ?provider.triple(),
                active_triple = ?active,
                "embedding worker disabled: provider triple does not match active triple"
            );
            return;
        }
        Err(error) => {
            tracing::error!(%error, "embedding worker disabled: cannot read active triple");
            return;
        }
    }

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
            match drain_batch_with_budget(&substrate, &provider, DRAIN_BATCH, &mut retry_budget).await {
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
    let jobs = substrate.pending_embedding_jobs(requested).await.map_err(|err| err.to_string())?;
    if jobs.is_empty() {
        return Ok(DrainOutcome { requested, fetched: 0, succeeded: 0 });
    }
    let fetched = jobs.len();
    let triple = provider.triple().clone();
    let mut succeeded = 0usize;

    for job in jobs {
        let PendingEmbeddingJob { chunk_id, text, content_hash } = job;
        if retry_budget.is_exhausted(&chunk_id) {
            tracing::debug!(%chunk_id, "embedding job skipped after retry budget exhaustion");
            continue;
        }
        // Embed off the async runtime — candle compute is blocking and would
        // otherwise stall the tokio worker. `spawn_blocking` requires `'static`,
        // which the cloned Arc + owned text satisfy.
        let embed_provider = Arc::clone(provider);
        let vector = match tokio::task::spawn_blocking(move || embed_provider.embed_document(&text)).await {
            Ok(Ok(vector)) => vector,
            Ok(Err(error)) => {
                retry_budget.record_failure(&chunk_id, error);
                continue;
            }
            Err(join_error) => {
                retry_budget.record_failure(&chunk_id, join_error);
                continue;
            }
        };
        let update = EmbeddingUpdate {
            chunk_id: chunk_id.clone(),
            expected_chunk_hash: content_hash,
            triple: triple.clone(),
            vector,
        };
        match substrate.update_embedding(update).await {
            Ok(()) => {
                succeeded = succeeded.saturating_add(1);
                retry_budget.record_success(&chunk_id);
            }
            Err(VectorError::StaleChunk { .. }) => {
                // StaleChunk here is benign: the chunk changed between drain and
                // write; reconcile re-enqueues with the new content hash.
                retry_budget.record_success(&chunk_id);
                tracing::debug!(%chunk_id, "embedding write skipped for stale chunk");
            }
            Err(error) => {
                retry_budget.record_failure(&chunk_id, error);
            }
        }
    }
    Ok(DrainOutcome { requested, fetched, succeeded })
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
        let substrate = Arc::new(substrate);
        // Fixture triple `synthetic/stream-a-test/32` deliberately != the active
        // production triple. run() must return promptly rather than draining.
        let mismatched: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::synthetic_test_triple());
        let (_tx, rx) = watch::channel(false);
        run(substrate, mismatched, rx, Duration::from_millis(10)).await;
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
