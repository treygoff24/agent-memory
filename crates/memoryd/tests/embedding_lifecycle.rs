use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Serializes tests that mutate or observe the process-global
/// `MODEL_LOAD_FAILURE` static (via `mark_failed`, `configure_loader`, or
/// doctor's `embedding_model_load_failed` finding). Without this, those tests
/// race each other under `--test-threads=2`. Async-aware so it can be held
/// across await points.
static GLOBAL_FAILURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn global_failure_guard() -> tokio::sync::MutexGuard<'static, ()> {
    GLOBAL_FAILURE_LOCK.lock().await
}

use memory_substrate::{EmbeddingTriple, InitOptions, Roots, Substrate};
use memoryd::embedding::{
    worker, EmbeddingError, EmbeddingIdleWindow, EmbeddingProvider, EmbeddingProviderAcquire, EmbeddingProviderSlot,
    FixtureProvider,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{DoctorFinding, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::{build_delta_response_with_vector_recall, DeltaRequest, VectorRecallConfig, VectorRecallContext};
use tokio::sync::watch;

#[tokio::test]
async fn startup_is_dormant_and_does_not_load_without_demand() {
    let fixture = TestRepo::new("dev_lcstart").await;
    let slot = configured_slot(fixture.triple(), idle(Some(Duration::from_secs(60))), Arc::new(AtomicUsize::new(0)));

    let snapshot = slot.snapshot();
    assert_eq!(snapshot.state, "dormant");
    assert_eq!(snapshot.load_count, 0);
}

#[tokio::test]
async fn queued_jobs_load_provider_and_drain() {
    let fixture = TestRepo::new("dev_lcdrain").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let slot = configured_slow_slot(fixture.triple(), idle(Some(Duration::from_secs(60))), loads.clone(), 80);
    write_project_memory(&fixture.substrate, "queued lifecycle job", "queued lifecycle job body").await;

    let (_tx, rx) = watch::channel(false);
    let handle = worker::spawn_embedding_worker_with_interval(
        fixture.substrate.clone(),
        slot.clone(),
        rx,
        Duration::from_millis(10),
    );

    eventually("worker enters loading", || slot.snapshot().state == "loading").await;
    eventually_async("job drains", || async {
        fixture.substrate.vector_count(fixture.triple()).await.unwrap_or(0) > 0
    })
    .await;
    assert_eq!(slot.snapshot().state, "active");
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    handle.abort();
}

#[tokio::test]
async fn idle_window_unloads_active_provider() {
    let fixture = TestRepo::new("dev_lcidle").await;
    let slot = configured_slot(fixture.triple(), idle(Some(Duration::from_millis(30))), Arc::new(AtomicUsize::new(0)));

    slot.ensure_loaded().await.expect("load");
    tokio::time::sleep(Duration::from_millis(45)).await;
    slot.check_idle_now();

    let snapshot = slot.snapshot();
    assert_eq!(snapshot.state, "dormant");
    assert_eq!(snapshot.unload_count, 1);
}

#[tokio::test]
async fn idle_unload_defers_while_guard_is_held() {
    let fixture = TestRepo::new("dev_lcguard").await;
    let slot = configured_slot(fixture.triple(), idle(Some(Duration::from_millis(30))), Arc::new(AtomicUsize::new(0)));
    slot.ensure_loaded().await.expect("load");
    let guard = match slot.acquire() {
        EmbeddingProviderAcquire::Active(guard) => guard,
        _ => panic!("expected active provider"),
    };

    tokio::time::sleep(Duration::from_millis(45)).await;
    slot.check_idle_now();
    assert_eq!(slot.snapshot().state, "unloading-pending-inflight");

    drop(guard);
    let snapshot = slot.snapshot();
    assert_eq!(snapshot.state, "dormant");
    assert_eq!(snapshot.unload_count, 1);
}

#[tokio::test]
async fn dormant_query_degrades_once_and_triggers_background_load() {
    let fixture = TestRepo::new("dev_lcquery").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let slot = configured_slow_slot(fixture.triple(), idle(Some(Duration::from_secs(60))), loads.clone(), 40);
    write_project_memory(&fixture.substrate, "fallback exact keyword", "fallback exact keyword body").await;

    let response = delta_with_slot(&fixture, slot.clone(), "fallback exact keyword").await;

    assert_eq!(response.vector_recall_degraded.as_deref(), Some("embedding_dormant"));
    eventually("background load starts", || {
        let state = slot.snapshot().state;
        state == "loading" || state == "active"
    })
    .await;
    eventually("background load finishes", || slot.snapshot().load_count == 1).await;
    assert_eq!(loads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_query_keeps_no_embedding_provider_marker() {
    let fixture = TestRepo::new("dev_lcfail").await;
    let slot = EmbeddingProviderSlot::empty();
    let triple = fixture.triple();
    slot.configure_loader(triple, idle(Some(Duration::from_secs(60))), || {
        Err(EmbeddingError::Load("fixture load failure".to_string()))
    });
    assert!(slot.ensure_loaded().await.is_err());

    let response = delta_with_slot(&fixture, slot, "anything").await;

    assert_eq!(response.vector_recall_degraded.as_deref(), Some("no_embedding_provider"));
}

/// F5: a governance write with a configured-but-dormant slot must surface
/// `similarity_degraded:embedding_dormant` in the decision trace (the dormant
/// lifecycle state is healthy but the similarity backend is not live yet).
#[tokio::test]
async fn f05_governance_write_with_dormant_slot_emits_embedding_dormant_marker() {
    let fixture = TestRepo::new("dev_f05dormant").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let state = HandlerState::new();
    // Configure a loader on the state's own slot but keep it dormant (don't
    // call ensure_loaded). Use a slow loader so the slot stays dormant/loading
    // during the write rather than completing before the governance path reads it.
    let slot = state.embedding_provider_slot();
    let triple = fixture.triple();
    let loads_for_loader = loads.clone();
    let triple_for_loader = triple.clone();
    slot.configure_loader(triple, idle(Some(Duration::from_secs(60))), move || {
        loads_for_loader.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_for_loader.clone()));
        Ok(provider)
    });

    let response = handle_request_with_state(
        &fixture.substrate,
        &state,
        RequestEnvelope::new(
            "f05-governance-write",
            RequestPayload::WriteMemory {
                body: "A grounded project claim for the dormant marker test.".to_string(),
                title: Some("dormant marker test claim".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "dormant marker test claim",
                    "canonical_namespace_id": "proj_f05_dormant",
                    "namespace_alias": "f05-dormant",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    // The slot is dormant (no provider loaded), so the governance write path
    // must see the embedding_dormant degradation marker, NOT no_embedding_provider
    // (which is for failed/disabled, not dormant).
    let write = match response.result {
        ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write,
        other => panic!("expected governed write success, got {other:?}"),
    };
    assert_eq!(
        write.similarity_degraded.as_deref(),
        Some("similarity_degraded:embedding_dormant"),
        "dormant slot must surface embedding_dormant marker, got {:?}",
        write.similarity_degraded,
    );
}

#[tokio::test]
async fn zero_idle_window_never_unloads() {
    let fixture = TestRepo::new("dev_lcnever").await;
    let slot = configured_slot(fixture.triple(), idle(None), Arc::new(AtomicUsize::new(0)));

    slot.ensure_loaded().await.expect("load");
    tokio::time::sleep(Duration::from_millis(40)).await;
    slot.check_idle_now();

    assert_eq!(slot.snapshot().state, "active");
    assert_eq!(slot.snapshot().unload_count, 0);
}

#[tokio::test]
async fn concurrent_loads_are_coalesced() {
    let fixture = TestRepo::new("dev_lccoal").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let slot = configured_slow_slot(fixture.triple(), idle(Some(Duration::from_secs(60))), loads.clone(), 60);

    let (a, b) = tokio::join!(slot.ensure_loaded(), slot.ensure_loaded());

    assert!(a.is_ok());
    assert!(b.is_ok());
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(slot.snapshot().state, "active");
}

#[tokio::test]
async fn reload_rechecks_triple_and_disables_on_mismatch() {
    let fixture = TestRepo::new("dev_lcreload").await;
    let active = fixture.triple();
    let calls = Arc::new(AtomicUsize::new(0));
    let slot = EmbeddingProviderSlot::empty();
    let calls_for_loader = calls.clone();
    let active_for_loader = active.clone();
    slot.configure_loader(active, idle(Some(Duration::from_millis(20))), move || {
        let call = calls_for_loader.fetch_add(1, Ordering::SeqCst);
        let triple = if call == 0 {
            active_for_loader.clone()
        } else {
            EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "other".to_string(), dimension: 32 }
        };
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple));
        Ok(provider)
    });

    slot.ensure_loaded().await.expect("first load");
    tokio::time::sleep(Duration::from_millis(30)).await;
    slot.check_idle_now();
    assert_eq!(slot.snapshot().state, "dormant");

    let error = slot.ensure_loaded().await.expect_err("reload mismatch");

    assert!(error.to_string().contains("does not match active triple"));
    assert_eq!(slot.snapshot().state, "failed");
    assert_eq!(slot.snapshot().load_count, 1);
}

#[tokio::test]
async fn active_demand_resets_idle_timer() {
    let fixture = TestRepo::new("dev_lcreset").await;
    let slot = configured_slot(fixture.triple(), idle(Some(Duration::from_millis(60))), Arc::new(AtomicUsize::new(0)));
    slot.ensure_loaded().await.expect("load");

    tokio::time::sleep(Duration::from_millis(35)).await;
    drop(slot.acquire());
    tokio::time::sleep(Duration::from_millis(35)).await;
    slot.check_idle_now();
    assert_eq!(slot.snapshot().state, "active");

    tokio::time::sleep(Duration::from_millis(35)).await;
    slot.check_idle_now();
    assert_eq!(slot.snapshot().state, "dormant");
}

/// F2: cancelling the initiating `ensure_loaded` future mid-load must not strand
/// the phase at `Loading`. The decoupled spawn task always transitions state
/// regardless of caller cancellation, so a subsequent acquire is not stuck.
#[tokio::test]
async fn f02_cancelled_ensure_loaded_still_reaches_active() {
    let fixture = TestRepo::new("dev_f02cancel").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let slot = configured_slow_slot(fixture.triple(), idle(Some(Duration::from_secs(60))), loads.clone(), 200);

    // Start the load, then abort the future mid-load (before the 200ms sleep
    // finishes). The decoupled task should still complete the transition.
    let slot_for_spawn = slot.clone();
    let load_handle = tokio::spawn(async move { slot_for_spawn.ensure_loaded().await });
    tokio::time::sleep(Duration::from_millis(50)).await; // let the load enter Loading
    load_handle.abort();

    // The spawned task should still reach Active despite the caller abort.
    eventually("state reaches active after caller abort", || slot.snapshot().state == "active").await;
    assert_eq!(loads.load(Ordering::SeqCst), 1, "exactly one load was coalesced");

    // A subsequent acquire must not be stuck at Loading.
    let guard = match slot.acquire() {
        EmbeddingProviderAcquire::Active(guard) => guard,
        _ => panic!("expected active provider after cancelled load"),
    };
    drop(guard);
}

/// F2: a failed load in the decoupled task must still reach Failed (not hang
/// waiters). A subsequent `ensure_loaded` within the backoff window returns Err
/// without re-invoking the loader.
#[tokio::test]
async fn f02_cancelled_ensure_loaded_failure_reaches_failed() {
    let fixture = TestRepo::new("dev_f02fail").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let slot = EmbeddingProviderSlot::empty();
    let loads_for_loader = loads.clone();
    slot.configure_loader(fixture.triple(), idle(Some(Duration::from_secs(60))), move || {
        loads_for_loader.fetch_add(1, Ordering::SeqCst);
        Err(EmbeddingError::Load("fixture load failure".to_string()))
    });

    let slot_for_spawn = slot.clone();
    let load_handle = tokio::spawn(async move { slot_for_spawn.ensure_loaded().await });
    eventually("state reaches failed after caller abort", || slot.snapshot().state == "failed").await;
    load_handle.abort();
    assert_eq!(loads.load(Ordering::SeqCst), 1);

    // Within backoff: ensure_loaded returns Err without invoking the loader again.
    let result = slot.ensure_loaded().await;
    assert!(result.is_err(), "ensure_loaded within backoff must return Err");
    assert_eq!(loads.load(Ordering::SeqCst), 1, "loader must not be invoked again within backoff");
}

/// F3: a timed-out query keeps `in_flight > 0` until the blocking embed
/// finishes. The guard is moved into `spawn_blocking`, so the timeout drops the
/// JoinHandle but the task runs on — `in_flight` stays elevated, unload is
/// deferred, and after the channel releases, `in_flight` drops to 0 and unload
/// can proceed.
#[tokio::test]
async fn f03_timeout_abandoned_guard_keeps_in_flight_until_embed_completes() {
    let fixture = TestRepo::new("dev_f03timeout").await;
    let triple = fixture.triple();

    // A provider whose `embed_query` blocks on a std channel until the test
    // releases it. This simulates a wedged/slow blocking embed.
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let blocking_provider = BlockingQueryProvider::new(triple.clone(), release_rx);
    let slot = EmbeddingProviderSlot::empty();
    slot.set(Arc::new(blocking_provider));
    slot.set_idle_window_for_tests(idle(Some(Duration::from_millis(30))));

    // Write a memory so FTS has something to return (the vector recall will
    // time out but FTS still works).
    write_project_memory(&fixture.substrate, "timeout guard keyword", "timeout guard keyword body").await;

    // Use a 1ms embed timeout so the query times out almost immediately.
    let timeout_config = VectorRecallConfig { embed_timeout_ms: Some(1), ..VectorRecallConfig::default() };
    let response = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("timeout guard keyword"),
        VectorRecallContext::from_lifecycle(slot.clone(), timeout_config),
    )
    .await
    .expect("delta");
    assert_eq!(
        response.vector_recall_degraded.as_deref(),
        Some("embedding_timeout"),
        "the timeout marker must be returned",
    );

    // While the embed is still blocked, in_flight must be >= 1.
    let snapshot = slot.snapshot();
    assert!(
        snapshot.in_flight >= 1,
        "in_flight must be >= 1 while the blocking embed is still running, got {}",
        snapshot.in_flight,
    );

    // Let the idle window elapse and check_idle_now: unload should defer
    // because in_flight > 0.
    tokio::time::sleep(Duration::from_millis(40)).await;
    slot.check_idle_now();
    assert_eq!(slot.snapshot().state, "unloading-pending-inflight", "unload must defer while in_flight > 0",);

    // Release the channel: the blocking embed finishes, the guard drops,
    // in_flight drops to 0, and unload can proceed.
    release_tx.send(()).expect("release channel");
    eventually("in_flight drops to 0 after channel release", || slot.snapshot().in_flight == 0).await;
    slot.check_idle_now();
    assert_eq!(slot.snapshot().state, "dormant", "unload proceeds after in_flight drops to 0");
}

/// F6: a failed load is rejected before the backoff elapses (without re-invoking
/// the loader), and retried after the (short, test-configured) backoff elapses.
#[tokio::test]
async fn f06_failed_load_backoff_rejects_then_retries() {
    let fixture = TestRepo::new("dev_f06backoff").await;
    let loads = Arc::new(AtomicUsize::new(0));
    let slot = EmbeddingProviderSlot::empty();
    let loads_for_loader = loads.clone();
    let triple = fixture.triple();
    slot.configure_loader(triple.clone(), idle(Some(Duration::from_secs(60))), move || {
        loads_for_loader.fetch_add(1, Ordering::SeqCst);
        Err(EmbeddingError::Load("fixture load failure".to_string()))
    });
    slot.set_load_retry_backoff_for_tests(Duration::from_millis(50));

    assert!(slot.ensure_loaded().await.is_err(), "first load must fail");
    assert_eq!(loads.load(Ordering::SeqCst), 1, "loader invoked once");
    assert_eq!(slot.snapshot().state, "failed");

    assert!(slot.ensure_loaded().await.is_err(), "ensure_loaded within backoff must return Err");
    assert_eq!(loads.load(Ordering::SeqCst), 1, "loader must not be invoked again within backoff");

    tokio::time::sleep(Duration::from_millis(60)).await;

    assert!(slot.ensure_loaded().await.is_err(), "retry after backoff must still fail (fixture)");
    assert_eq!(loads.load(Ordering::SeqCst), 2, "loader invoked again after backoff elapsed");
}

/// F1: doctor must NOT emit `embedding_worker_idle` for a configured-but-dormant
/// slot with a nonzero backlog and empty vector table. Dormant is healthy per
/// design amendment F4 — the `embedding_backlog` advisory alone covers it.
#[tokio::test]
async fn f01_doctor_no_idle_finding_for_dormant_slot_with_backlog() {
    let _global = global_failure_guard().await;
    let fixture = TestRepo::new("dev_f01dormant").await;
    let state = HandlerState::new();
    // Configure a loader on the state's slot but keep it dormant.
    let slot = state.embedding_provider_slot();
    let triple = fixture.triple();
    let triple_for_loader = triple.clone();
    slot.configure_loader(triple, idle(Some(Duration::from_secs(60))), move || {
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_for_loader.clone()));
        Ok(provider)
    });

    // Write a memory to create pending embedding jobs (backlog > 0). The vector
    // table is empty because we never drained.
    write_project_memory(&fixture.substrate, "dormant doctor keyword", "dormant doctor keyword body").await;
    let backlog = fixture
        .substrate
        .pending_embedding_job_count(memory_substrate::EmbeddingLaneEligibility::AllTiers)
        .expect("pending count");
    assert!(backlog > 0, "there must be pending embedding jobs");
    let vec_count = fixture.substrate.vector_count(fixture.triple()).await.unwrap_or(0);
    assert_eq!(vec_count, 0, "vector table must be empty");

    let doctor = request_doctor_with_state(&fixture.substrate, &state).await;

    // The dormant slot with a loader is healthy: NO embedding_worker_idle.
    assert!(
        !doctor.findings.iter().any(|f| f.code == "embedding_worker_idle"),
        "dormant slot with a loader must NOT emit embedding_worker_idle, got: {:?}",
        doctor.findings.iter().map(|f| &f.code).collect::<Vec<_>>(),
    );
    // But embedding_backlog advisory should still be present.
    assert!(
        doctor.findings.iter().any(|f| f.code == "embedding_backlog"),
        "embedding_backlog advisory should be present for nonzero backlog",
    );
}

/// F1: doctor MUST emit `embedding_worker_idle` when the slot is in a failed
/// state with a nonzero backlog and empty vector table.
#[tokio::test]
async fn f01_doctor_idle_finding_for_failed_slot_with_backlog() {
    let _global = global_failure_guard().await;
    let fixture = TestRepo::new("dev_f01failed").await;
    let state = HandlerState::new();
    let slot = state.embedding_provider_slot();
    let triple = fixture.triple();
    slot.configure_loader(triple, idle(Some(Duration::from_secs(60))), || {
        Err(EmbeddingError::Load("fixture load failure".to_string()))
    });
    assert!(slot.ensure_loaded().await.is_err());
    assert_eq!(slot.snapshot().state, "failed");

    write_project_memory(&fixture.substrate, "failed doctor keyword", "failed doctor keyword body").await;

    let doctor = request_doctor_with_state(&fixture.substrate, &state).await;

    assert!(
        doctor.findings.iter().any(|f| f.code == "embedding_worker_idle"),
        "failed slot with backlog must emit embedding_worker_idle, got: {:?}",
        doctor.findings.iter().map(|f| &f.code).collect::<Vec<_>>(),
    );
}

/// F7: doctor must NOT claim "retrying" when the worker is hard-disabled via
/// MEMORUM_DISABLE_EMBEDDING_WORKER. The message must distinguish intentional
/// disable from transient load failure.
#[tokio::test]
async fn f07_doctor_disabled_worker_message_does_not_claim_retrying() {
    let _global = global_failure_guard().await;
    let fixture = TestRepo::new("dev_f07disabled").await;
    let state = HandlerState::new();
    let slot = state.embedding_provider_slot();
    // Mark the slot as failed with the intentional-disable message, exactly as
    // server.rs does when MEMORUM_DISABLE_EMBEDDING_WORKER is set.
    slot.mark_failed("embedding worker disabled via MEMORUM_DISABLE_EMBEDDING_WORKER");

    let doctor = request_doctor_with_state(&fixture.substrate, &state).await;

    let finding = doctor
        .findings
        .iter()
        .find(|f: &&DoctorFinding| f.code == "embedding_model_load_failed")
        .expect("embedding_model_load_failed finding must be present for a disabled worker");
    assert!(
        !finding.message.contains("retrying"),
        "disabled-worker message must NOT claim retrying, got: {}",
        finding.message,
    );
    assert!(
        finding.message.contains("intentionally disabled"),
        "disabled-worker message must say 'intentionally disabled', got: {}",
        finding.message,
    );
}

/// F1: a stale load completion from a previous lifecycle configuration must
/// not clobber a newer Active provider. Loader A blocks on a channel;
/// reconfigure with loader B; complete B (Active); release A with a FAILING
/// result; assert state remains Active and B's provider is the one served.
#[tokio::test]
async fn f1_stale_load_completion_does_not_clobber_newer_active() {
    let fixture = TestRepo::new("dev_f1stale").await;
    let triple = fixture.triple();

    // Loader A: blocks on a channel until the test releases it, then returns
    // a FAILING result.
    let (release_a_tx, release_a_rx) = mpsc::channel::<Result<(), EmbeddingError>>();
    let release_a_rx = std::sync::Mutex::new(release_a_rx);
    let slot = EmbeddingProviderSlot::empty();
    let release_for_loader_a = release_a_rx;
    let triple_for_loader_a = triple.clone();
    slot.configure_loader(triple.clone(), idle(Some(Duration::from_secs(60))), move || {
        let rx = release_for_loader_a.lock().expect("release lock");
        match rx.recv() {
            Ok(Ok(())) => {
                let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_for_loader_a.clone()));
                Ok(provider)
            }
            _ => Err(EmbeddingError::Load("stale loader A failure".to_string())),
        }
    });

    let slot_for_load_a = slot.clone();
    let load_a_handle = tokio::spawn(async move { slot_for_load_a.ensure_loaded().await });
    eventually("load A enters loading", || slot.snapshot().state == "loading").await;

    // Reconfigure with loader B (a fast, succeeding loader). This bumps the
    // generation and resets the phase to Dormant.
    let loads_b = Arc::new(AtomicUsize::new(0));
    let loads_b_for_loader = loads_b.clone();
    let triple_b = triple.clone();
    slot.configure_loader(triple.clone(), idle(Some(Duration::from_secs(60))), move || {
        loads_b_for_loader.fetch_add(1, Ordering::SeqCst);
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_b.clone()));
        Ok(provider)
    });

    slot.ensure_loaded().await.expect("load B succeeds");
    assert_eq!(slot.snapshot().state, "active");
    assert_eq!(loads_b.load(Ordering::SeqCst), 1);

    let guard_b = match slot.acquire() {
        EmbeddingProviderAcquire::Active(guard) => guard,
        _ => panic!("expected active provider from loader B"),
    };
    let b_triple = guard_b.triple().clone();
    drop(guard_b);

    // Release loader A with a FAILING result. The stale completion must NOT
    // clobber B's Active state.
    release_a_tx.send(Err(EmbeddingError::Load("stale loader A failure".to_string()))).expect("release A");
    eventually("stale load A completion is processed", || load_a_handle.is_finished()).await;

    assert_eq!(slot.snapshot().state, "active", "stale load A failure must not clobber B's Active state",);

    let guard_after = match slot.acquire() {
        EmbeddingProviderAcquire::Active(guard) => guard,
        _ => panic!("expected active provider after stale A completion"),
    };
    assert_eq!(guard_after.triple(), &b_triple, "B's provider must still be served after stale A completion",);
    drop(guard_after);

    // B's load count must still be 1 (A's stale success/failure did not
    // increment it).
    assert_eq!(slot.snapshot().load_count, 1, "stale completion must not increment load_count");
}

/// F2: a stale idle timer from a previous configuration must not unload a
/// freshly-reloaded provider early nor strand `idle_check_armed`. Arm a short
/// idle timer, reconfigure (generation bump), publish a new active provider,
/// and assert the stale timer firing neither unloads the new provider early
/// nor prevents a fresh idle cycle from unloading on schedule.
#[tokio::test]
async fn f2_stale_idle_timer_does_not_unload_new_provider_or_strand_armed_flag() {
    let fixture = TestRepo::new("dev_f2stale").await;
    let triple = fixture.triple();

    let slot = configured_slot(triple.clone(), idle(Some(Duration::from_millis(20))), Arc::new(AtomicUsize::new(0)));
    slot.ensure_loaded().await.expect("load");
    assert_eq!(slot.snapshot().state, "active");

    // Wait briefly so the idle timer is armed and sleeping, then reconfigure
    // with a longer idle window. This bumps the generation, invalidating the
    // armed timer from the pre-reconfigure era.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let loads_b = Arc::new(AtomicUsize::new(0));
    let loads_b_for_loader = loads_b.clone();
    let triple_b = triple.clone();
    slot.configure_loader(triple.clone(), idle(Some(Duration::from_millis(200))), move || {
        loads_b_for_loader.fetch_add(1, Ordering::SeqCst);
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_b.clone()));
        Ok(provider)
    });

    // Publish a new active provider via ensure_loaded. The stale timer from
    // the pre-reconfigure era should fire during this window but must not
    // unload the new provider.
    slot.ensure_loaded().await.expect("reload after reconfigure");
    assert_eq!(slot.snapshot().state, "active");
    assert_eq!(loads_b.load(Ordering::SeqCst), 1);

    // Wait long enough for the stale 20ms timer to have fired (it was armed
    // before the reconfigure). The new provider must still be Active — the
    // stale timer no-op'd.
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(slot.snapshot().state, "active", "stale idle timer must not unload the freshly-reloaded provider early",);
    assert_eq!(slot.snapshot().unload_count, 0, "stale idle timer must not increment unload_count",);

    // Avoid `check_idle_now()` so a broken timer re-arm cannot hide behind the
    // manual fallback.
    eventually("fresh idle timer unloads on schedule without manual check", || slot.snapshot().state == "dormant")
        .await;
    assert_eq!(slot.snapshot().unload_count, 1, "unload_count must be 1 after the fresh idle cycle unloads",);
}

/// Round-3 F2 pin: an idle unload via the GUARD-DROP path (not the timer's
/// `fire_idle_check`) must clear `idle_check_armed`. If it doesn't, the next
/// Active cycle never arms a timer and idle unload is permanently disarmed —
/// the daemon silently reverts to hold-forever.
#[tokio::test]
async fn guard_drop_unload_does_not_strand_idle_timer_arming() {
    let fixture = TestRepo::new("dev_guarddrop").await;
    let triple = fixture.triple();
    let slot = configured_slot(triple.clone(), idle(Some(Duration::from_millis(30))), Arc::new(AtomicUsize::new(0)));

    // Load and hold a guard past the idle window: the armed timer fires,
    // finds in_flight > 0, and defers.
    slot.ensure_loaded().await.expect("load");
    let guard = match slot.acquire() {
        EmbeddingProviderAcquire::Active(guard) => guard,
        _ => panic!("expected active acquire"),
    };
    tokio::time::sleep(Duration::from_millis(45)).await;
    assert_eq!(slot.snapshot().state, "unloading-pending-inflight", "timer must defer while a guard is held");

    // Dropping the guard unloads via the guard-drop path (idle window already
    // elapsed, in_flight hits 0).
    drop(guard);
    eventually("guard-drop unload lands", || slot.snapshot().state == "dormant").await;
    assert_eq!(slot.snapshot().unload_count, 1);

    // Reload. If the guard-drop unload stranded `idle_check_armed`, no timer
    // is armed now and the state stays active forever.
    slot.ensure_loaded().await.expect("reload");
    assert_eq!(slot.snapshot().state, "active");
    eventually("second idle cycle unloads via the timer alone", || slot.snapshot().state == "dormant").await;
    assert_eq!(slot.snapshot().unload_count, 2, "idle timer must still function after a guard-drop unload");
}

/// F3: a stale global `MODEL_LOAD_FAILURE` must be cleared by
/// `configure_loader`, so doctor does not emit a stale
/// `embedding_model_load_failed` finding for a healthy reconfigured slot.
#[tokio::test]
async fn f3_configure_loader_clears_stale_global_model_load_failure() {
    let _global = global_failure_guard().await;
    let fixture = TestRepo::new("dev_f3clear").await;
    let state = HandlerState::new();
    let slot = state.embedding_provider_slot();
    let triple = fixture.triple();

    slot.mark_failed("stale failure to be cleared");
    let doctor_before = request_doctor_with_state(&fixture.substrate, &state).await;
    assert!(
        doctor_before.findings.iter().any(|f| f.code == "embedding_model_load_failed"),
        "mark_failed must produce an embedding_model_load_failed finding before configure_loader",
    );

    let triple_for_loader = triple.clone();
    slot.configure_loader(triple, idle(Some(Duration::from_secs(60))), move || {
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_for_loader.clone()));
        Ok(provider)
    });

    let doctor_after = request_doctor_with_state(&fixture.substrate, &state).await;
    assert!(
        !doctor_after.findings.iter().any(|f| f.code == "embedding_model_load_failed"),
        "configure_loader must clear stale global MODEL_LOAD_FAILURE so doctor does not emit a stale finding, got: {:?}",
        doctor_after.findings.iter().map(|f| &f.code).collect::<Vec<_>>(),
    );
}

fn idle(duration: Option<Duration>) -> EmbeddingIdleWindow {
    EmbeddingIdleWindow::from_duration(duration, "test")
}

fn configured_slot(
    triple: EmbeddingTriple,
    idle_window: EmbeddingIdleWindow,
    loads: Arc<AtomicUsize>,
) -> EmbeddingProviderSlot {
    configured_slow_slot(triple, idle_window, loads, 0)
}

fn configured_slow_slot(
    triple: EmbeddingTriple,
    idle_window: EmbeddingIdleWindow,
    loads: Arc<AtomicUsize>,
    sleep_ms: u64,
) -> EmbeddingProviderSlot {
    let slot = EmbeddingProviderSlot::empty();
    let loader_triple = triple.clone();
    slot.configure_loader(triple, idle_window, move || {
        loads.fetch_add(1, Ordering::SeqCst);
        if sleep_ms > 0 {
            std::thread::sleep(Duration::from_millis(sleep_ms));
        }
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(loader_triple.clone()));
        Ok(provider)
    });
    slot
}

async fn delta_with_slot(
    fixture: &TestRepo,
    slot: EmbeddingProviderSlot,
    message: &str,
) -> memoryd::recall::DeltaResponse {
    build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request(message),
        VectorRecallContext::from_lifecycle(slot, VectorRecallConfig::default()),
    )
    .await
    .expect("delta")
}

async fn request_doctor_with_state(substrate: &Substrate, state: &HandlerState) -> memoryd::protocol::DoctorResponse {
    let response =
        handle_request_with_state(substrate, state, RequestEnvelope::new("doctor", RequestPayload::Doctor)).await;
    match response.result {
        ResponseResult::Success(ResponsePayload::Doctor(doctor)) => doctor,
        other => panic!("expected doctor success, got {other:?}"),
    }
}

async fn eventually(label: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..100 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {label}");
}

async fn eventually_async<F, Fut>(label: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {label}");
}

struct TestRepo {
    _temp: tempfile::TempDir,
    repo: std::path::PathBuf,
    substrate: Arc<Substrate>,
}

impl TestRepo {
    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, repo, substrate: Arc::new(substrate) }
    }

    fn triple(&self) -> EmbeddingTriple {
        self.substrate.active_embedding_triple().expect("active triple")
    }

    fn delta_request(&self, message: &str) -> DeltaRequest {
        DeltaRequest {
            cwd: self.repo.to_string_lossy().into_owned(),
            session_id: "sess_embedding_lifecycle".to_owned(),
            harness: "codex".to_owned(),
            message: message.to_owned(),
            budget_tokens: Some(8_000),
            passive: false,
        }
    }
}

async fn write_project_memory(substrate: &Substrate, summary: &str, body: &str) {
    let response = handle_request_with_state(
        substrate,
        &HandlerState::new(),
        RequestEnvelope::new(
            "embedding-lifecycle-write",
            RequestPayload::WriteMemory {
                body: body.to_string(),
                title: Some(summary.to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": summary,
                    "canonical_namespace_id": "proj_embedding_lifecycle_test",
                    "namespace_alias": "embedding-lifecycle-test",
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
        ResponseResult::Success(ResponsePayload::GovernanceWrite(_)) => {}
        other => panic!("expected governed write success, got {other:?}"),
    }
}

/// A provider whose `embed_query` blocks on a std channel until released,
/// simulating a wedged/slow blocking embed that outlives a query timeout.
struct BlockingQueryProvider {
    triple: EmbeddingTriple,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}

impl BlockingQueryProvider {
    fn new(triple: EmbeddingTriple, release: mpsc::Receiver<()>) -> Self {
        Self { triple, release: std::sync::Mutex::new(release) }
    }
}

impl EmbeddingProvider for BlockingQueryProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        // Block until the test releases the channel. This simulates a real
        // blocking embed that outlives the query timeout.
        let rx = self.release.lock().expect("release lock");
        let _ = rx.recv();
        Ok(vec![0.0; self.triple.dimension as usize])
    }

    fn embed_document(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![0.0; self.triple.dimension as usize])
    }
}
