use anyhow::{anyhow, Result};
use std::fmt;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerName {
    WatcherIndexer,
    EmbeddingQueue,
    SyncManager,
    McpPeerActivity,
}

impl fmt::Display for WorkerName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WatcherIndexer => "watcher_indexer",
            Self::EmbeddingQueue => "embedding_queue",
            Self::SyncManager => "sync_manager",
            Self::McpPeerActivity => "mcp_peer_activity",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerState {
    Starting,
    Running,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerHealth {
    pub name: WorkerName,
    pub state: WorkerState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkersHealth {
    pub workers: Vec<WorkerHealth>,
}

impl WorkersHealth {
    pub fn is_healthy(&self) -> bool {
        self.workers.iter().all(|worker| worker.state == WorkerState::Running)
    }
}

pub struct WorkerSupervisor {
    shutdown: watch::Sender<bool>,
    workers: Vec<WorkerHandle>,
}

impl WorkerSupervisor {
    pub fn start() -> Self {
        let (shutdown, shutdown_rx) = watch::channel(false);
        let workers = WorkerName::all().iter().map(|name| WorkerHandle::spawn(*name, shutdown_rx.clone())).collect();

        Self { shutdown, workers }
    }

    pub fn health(&self) -> WorkersHealth {
        let workers = self.workers.iter().map(WorkerHandle::health).collect();
        WorkersHealth { workers }
    }

    pub async fn wait_until_all_running(&self) {
        // Use each worker's watch channel to block until it transitions to Running.
        // This avoids busy-polling: we yield until the sender side of the channel
        // publishes the Running state (or a terminal Stopped state).
        for worker in &self.workers {
            let mut rx = worker.state.clone();
            // wait_for returns an error only when the sender is dropped (i.e., the
            // task finished without ever reaching Running — treat that as terminal).
            let _ = rx.wait_for(|state| matches!(state, WorkerState::Running | WorkerState::Stopped)).await;
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.shutdown.send(true);

        for worker in &mut self.workers {
            worker.stop().await?;
        }

        Ok(())
    }
}

impl WorkerName {
    const fn all() -> [Self; 4] {
        [Self::WatcherIndexer, Self::EmbeddingQueue, Self::SyncManager, Self::McpPeerActivity]
    }
}

struct WorkerHandle {
    name: WorkerName,
    state: watch::Receiver<WorkerState>,
    task: Option<JoinHandle<Result<()>>>,
}

impl WorkerHandle {
    fn spawn(name: WorkerName, shutdown: watch::Receiver<bool>) -> Self {
        let (state_tx, state_rx) = watch::channel(WorkerState::Starting);
        let task = tokio::spawn(run_worker(name, shutdown, state_tx));

        Self { name, state: state_rx, task: Some(task) }
    }

    fn health(&self) -> WorkerHealth {
        // The task's last-published state may lie if the task panicked or was
        // dropped without going through the clean Stopped transition. Treat any
        // non-running task as Stopped regardless of what the watch channel says.
        let task_alive = self.task.as_ref().is_some_and(|task| !task.is_finished());
        let state = if task_alive { *self.state.borrow() } else { WorkerState::Stopped };
        WorkerHealth { name: self.name, state }
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.await.map_err(|error| anyhow!("worker {} join failed: {error}", self.name))??;
        }

        Ok(())
    }
}

async fn run_worker(
    _name: WorkerName,
    mut shutdown: watch::Receiver<bool>,
    state: watch::Sender<WorkerState>,
) -> Result<()> {
    let _ = state.send(WorkerState::Running);

    loop {
        if *shutdown.borrow() {
            break;
        }

        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            () = sleep(Duration::from_secs(60)) => {}
        }
    }

    let _ = state.send(WorkerState::Stopped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worker task that panics must show up as Stopped in health(), even
    /// though the watch channel never received the Stopped transition. Without
    /// the task-finished override, health() would happily keep reporting
    /// Running until somebody called stop().
    #[tokio::test]
    async fn worker_handle_health_reports_stopped_when_task_panics() {
        let (state_tx, state_rx) = watch::channel(WorkerState::Starting);
        let task = tokio::spawn(async move {
            // Publish Running so the watch channel "lies" about the worker's
            // health when we read it after the panic.
            let _ = state_tx.send(WorkerState::Running);
            panic!("synthetic panic for test");
        });
        let handle = WorkerHandle { name: WorkerName::WatcherIndexer, state: state_rx, task: Some(task) };

        // Wait for the task to actually finish (panic). Polling is_finished is
        // the cheapest way to await a JoinHandle without taking ownership.
        for _ in 0..100 {
            if handle.task.as_ref().expect("task present").is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(handle.task.as_ref().expect("task present").is_finished(), "panicked task should finish");
        assert_eq!(*handle.state.borrow(), WorkerState::Running, "watch state still reflects pre-panic Running");

        let health = handle.health();
        assert_eq!(health.name, WorkerName::WatcherIndexer);
        assert_eq!(
            health.state,
            WorkerState::Stopped,
            "panicked worker must report Stopped, not its stale watch state"
        );
    }
}
