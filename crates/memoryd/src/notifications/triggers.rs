use crate::protocol::NotificationEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    MergeQuarantined { path: String },
    DreamCompleted { scope: String, promoted: usize, queued: usize, dropped: usize },
    DailySynthesis { scope: String },
    ReviewQueueOverThreshold { count: usize, threshold: usize },
    RealityCheckOverdue { last_completed_at: Option<chrono::DateTime<chrono::Utc>>, weeks_skipped: u32 },
}

pub fn notification_for(kind: EventKind) -> NotificationEvent {
    match kind {
        EventKind::MergeQuarantined { path } => NotificationEvent::BlockingMergeConflict { path },
        EventKind::DreamCompleted { scope, promoted, queued, dropped } => {
            NotificationEvent::DreamRunCompleted { scope, promoted, queued, dropped }
        }
        EventKind::DailySynthesis { scope } => NotificationEvent::DailySynthesisSummaryReady { scope },
        EventKind::ReviewQueueOverThreshold { count, threshold } => {
            NotificationEvent::ReviewQueueOverThreshold { count, threshold }
        }
        EventKind::RealityCheckOverdue { last_completed_at, weeks_skipped } => {
            NotificationEvent::RealityCheckOverdue { last_completed_at, weeks_skipped }
        }
    }
}
