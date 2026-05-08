use chrono::{Duration, TimeZone, Utc};
use memory_governance::review::{over_threshold, REVIEW_QUEUE_DOGFOOD_THRESHOLD};
use memory_governance::{ReviewQueue, ReviewQueueItem, ReviewStatus};
use memoryd::notifications::config::NotificationConfig;
use memoryd::notifications::dispatcher::NotificationDispatcher;
use memoryd::notifications::external::ExternalNotifier;
use memoryd::notifications::os::OsNotifier;
use memoryd::notifications::passive::PassiveQueue;
use memoryd::notifications::triggers::{notification_for, EventKind};
use memoryd::protocol::NotificationEvent;
use memoryd::reality_check::RcScheduler;
use memoryd::state::RealityCheckState;

#[tokio::test]
async fn dispatcher_passively_records_five_dogfood_notification_variants() {
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive.clone(),
        NotificationConfig::default(),
        OsNotifier::disabled(),
        ExternalNotifier::disabled(),
    );

    for event in dogfood_events() {
        dispatcher.dispatch_event(event).await;
    }

    let messages = passive.messages();
    assert_eq!(messages.len(), 5);
    assert!(messages.iter().any(|message| message.contains("merge conflict")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Dream run completed")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Daily synthesis")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Review queue")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Reality Check is overdue")), "{messages:#?}");
}

#[test]
fn review_queue_threshold_is_pure_and_dogfood_sized() {
    assert!(!over_threshold(&review_queue(REVIEW_QUEUE_DOGFOOD_THRESHOLD - 1)));
    assert!(over_threshold(&review_queue(REVIEW_QUEUE_DOGFOOD_THRESHOLD)));
}

#[tokio::test]
async fn reality_check_due_emits_overdue_before_due_when_window_is_exceeded() {
    let now = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(30)), snooze_until: None };
    let (sender, mut receiver) = tokio::sync::broadcast::channel(4);

    assert!(RcScheduler::default().check_and_fire_if_due(&state, now, &sender));

    assert_eq!(
        receiver.recv().await.expect("overdue event"),
        NotificationEvent::RealityCheckOverdue { last_completed_at: state.last_completed_at, weeks_skipped: 4 }
    );
    assert_eq!(receiver.recv().await.expect("due event"), NotificationEvent::RealityCheckDue { due_at: now });
}

#[test]
fn trigger_registry_maps_dogfood_kinds_to_protocol_events() {
    assert_eq!(
        notification_for(EventKind::MergeQuarantined { path: "agent/conflict.md".to_string() }),
        NotificationEvent::BlockingMergeConflict { path: "agent/conflict.md".to_string() }
    );
    assert_eq!(
        notification_for(EventKind::DreamCompleted { scope: "agent".to_string(), promoted: 0, queued: 2, dropped: 1 }),
        NotificationEvent::DreamRunCompleted { scope: "agent".to_string(), promoted: 0, queued: 2, dropped: 1 }
    );
    assert_eq!(
        notification_for(EventKind::DailySynthesis { scope: "agent".to_string() }),
        NotificationEvent::DailySynthesisSummaryReady { scope: "agent".to_string() }
    );
}

fn dogfood_events() -> Vec<NotificationEvent> {
    let last_completed_at = Utc.with_ymd_and_hms(2026, 4, 1, 9, 0, 0).unwrap();
    vec![
        NotificationEvent::BlockingMergeConflict { path: "agent/conflict.md".to_string() },
        NotificationEvent::DreamRunCompleted { scope: "agent".to_string(), promoted: 0, queued: 2, dropped: 1 },
        NotificationEvent::DailySynthesisSummaryReady { scope: "agent".to_string() },
        NotificationEvent::ReviewQueueOverThreshold {
            count: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
            threshold: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
        },
        NotificationEvent::RealityCheckOverdue { last_completed_at: Some(last_completed_at), weeks_skipped: 4 },
    ]
}

fn review_queue(count: usize) -> ReviewQueue {
    ReviewQueue { items: (0..count).map(review_item).collect() }
}

fn review_item(index: usize) -> ReviewQueueItem {
    ReviewQueueItem {
        id: format!("mem_{index:02}"),
        summary: "candidate".to_string(),
        status: ReviewStatus::Candidate,
        policy_applied: "dogfood".to_string(),
        reason: None,
        next_actions: Vec::new(),
    }
}
