use memoryd::workers::{WorkerName, WorkerState, WorkerSupervisor};
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn worker_lifecycle_supervisor_reports_named_workers_and_stops_cleanly() {
    let mut supervisor = WorkerSupervisor::start();

    timeout(Duration::from_secs(1), supervisor.wait_until_all_running())
        .await
        .expect("workers should report running before timeout");

    let health = supervisor.health();
    let worker_names: Vec<WorkerName> = health.workers.iter().map(|worker| worker.name).collect();
    assert_eq!(
        worker_names,
        vec![
            WorkerName::WatcherIndexer,
            WorkerName::EmbeddingQueue,
            WorkerName::SyncManager,
            WorkerName::McpPeerActivity,
        ]
    );
    assert!(health.workers.iter().all(|worker| worker.state == WorkerState::Running));
    assert!(health.is_healthy());

    supervisor.shutdown().await.expect("workers should stop cleanly");

    let health = supervisor.health();
    assert!(health.workers.iter().all(|worker| worker.state == WorkerState::Stopped));
    assert!(!health.is_healthy());
}
