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

use std::sync::Arc;
use std::time::Duration;

use memory_substrate::{EmbeddingUpdate, PendingEmbeddingJob, Substrate};
use tokio::sync::watch;
use tokio::time::{interval_at, Instant, MissedTickBehavior};

use super::EmbeddingProvider;

/// How many jobs to pull and embed per drain tick. Bounded so one tick cannot
/// monopolize a blocking thread for an unbounded backlog; the next tick picks up
/// the remainder.
const DRAIN_BATCH: usize = 64;

/// Default interval between drain ticks when the queue is empty.
const IDLE_INTERVAL: Duration = Duration::from_secs(5);

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

    let mut ticker = interval_at(Instant::now() + idle_interval, idle_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            _ = ticker.tick() => {
                // Drain until the queue is empty or a batch comes back short,
                // so a large backlog after import clears without waiting one
                // idle interval per batch.
                loop {
                    if *shutdown.borrow() {
                        return;
                    }
                    match drain_batch(&substrate, &provider, DRAIN_BATCH).await {
                        Ok(0) => break,
                        Ok(n) if n < DRAIN_BATCH => break,
                        Ok(_) => continue,
                        Err(error) => {
                            tracing::warn!(%error, "embedding drain tick failed; retrying next interval");
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Embed and write up to `limit` pending jobs for the active triple, returning
/// how many jobs were processed.
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
    let jobs = substrate.pending_embedding_jobs(limit).await.map_err(|err| err.to_string())?;
    if jobs.is_empty() {
        return Ok(0);
    }
    let processed = jobs.len();
    let triple = provider.triple().clone();

    for job in jobs {
        let PendingEmbeddingJob { chunk_id, text, content_hash } = job;
        // Embed off the async runtime — candle compute is blocking and would
        // otherwise stall the tokio worker. `spawn_blocking` requires `'static`,
        // which the cloned Arc + owned text satisfy.
        let embed_provider = Arc::clone(provider);
        let vector = match tokio::task::spawn_blocking(move || embed_provider.embed_document(&text)).await {
            Ok(Ok(vector)) => vector,
            Ok(Err(error)) => {
                tracing::warn!(%error, %chunk_id, "embedding inference failed for chunk; leaving job pending");
                continue;
            }
            Err(join_error) => {
                tracing::warn!(%join_error, %chunk_id, "embedding blocking task panicked; leaving job pending");
                continue;
            }
        };
        let update = EmbeddingUpdate {
            chunk_id: chunk_id.clone(),
            expected_chunk_hash: content_hash,
            triple: triple.clone(),
            vector,
        };
        if let Err(error) = substrate.update_embedding(update).await {
            // StaleChunk here is benign: the chunk changed between drain and
            // write; reconcile re-enqueues with the new content hash.
            tracing::debug!(%error, %chunk_id, "embedding write skipped (stale or transient)");
        }
    }
    Ok(processed)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use memory_substrate::{InitOptions, Roots, Substrate};

    use super::*;
    use crate::embedding::FixtureProvider;

    async fn init_substrate() -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate =
            Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".into()) })
                .await
                .expect("init");
        (temp, substrate)
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
}
