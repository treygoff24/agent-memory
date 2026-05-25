use chrono::{TimeZone, Utc};
use memory_substrate::MemoryId;
use memoryd::handlers::{self, HandlerState};
use memoryd::mcp::forward_payload_to_daemon;
use memoryd::notifications::config::NotificationConfig;
use memoryd::notifications::external::ExternalNotifier;
use memoryd::notifications::os::OsNotifier;
use memoryd::notifications::NotificationDispatcher;
use memoryd::protocol::{
    NotificationEvent, RealityCheckRequest, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
    NOTIFICATION_CHANNEL_CAPACITY,
};

#[tokio::test]
async fn test_notification_event_all_seven_variants_constructible() {
    let due_at = Utc.with_ymd_and_hms(2026, 5, 4, 9, 0, 0).unwrap();
    let last_completed_at = Utc.with_ymd_and_hms(2026, 4, 6, 9, 0, 0).unwrap();
    let events = [
        NotificationEvent::LeakedSecretDetected { memory_id: MemoryId::new("mem_20260501_0123456789abcdef_000001") },
        NotificationEvent::BlockingMergeConflict { path: "memories/project/conflict.md".to_owned() },
        NotificationEvent::ReviewQueueOverThreshold { count: 51, threshold: 50 },
        NotificationEvent::DreamRunCompleted {
            scope: "project:agent-memory".to_owned(),
            promoted: 2,
            queued: 1,
            dropped: 0,
        },
        NotificationEvent::RealityCheckDue { due_at },
        NotificationEvent::RealityCheckOverdue { last_completed_at: Some(last_completed_at), weeks_skipped: 3 },
        NotificationEvent::DailySynthesisSummaryReady { scope: "daily".to_owned() },
    ];
    let (sender, mut receiver) = tokio::sync::broadcast::channel(NOTIFICATION_CHANNEL_CAPACITY);

    for event in &events {
        sender.send(event.clone()).expect("notification event sends");
    }

    for expected in events {
        assert_eq!(receiver.recv().await.expect("notification event receives"), expected);
    }
}

#[tokio::test]
async fn test_notification_event_mcp_rejected() {
    let response = forward_payload_to_daemon(
        std::path::Path::new("/tmp/memoryd-not-used.sock"),
        "req-reality-check",
        RequestPayload::RealityCheck(RealityCheckRequest::List { namespace: None, limit: None }),
    )
    .await
    .expect("MCP forwarder rejects admin payload without contacting daemon");

    let ResponseResult::Error(error) = response.result else {
        panic!("expected MCP rejection, got {:?}", response.result);
    };
    assert_eq!(error.code, "method_not_allowed_on_mcp");
    assert!(!error.retryable);
}

#[tokio::test]
async fn notifications_recent_returns_passive_queue() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let substrate = memory_substrate::Substrate::init(
        memory_substrate::Roots::new(&repo, &runtime),
        memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_notifs01".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();
    let dispatcher = NotificationDispatcher::new(
        state.passive_notifications(),
        NotificationConfig::default(),
        OsNotifier::disabled(),
        ExternalNotifier::disabled(),
    );
    dispatcher.dispatch_event(NotificationEvent::ReviewQueueOverThreshold { count: 7, threshold: 5 }).await;

    let response = handlers::handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("notifications-recent-test", RequestPayload::NotificationsRecent { limit: Some(50) }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::NotificationsRecent(recent)) = response.result else {
        panic!("expected notifications recent success, got {:?}", response.result);
    };
    assert_eq!(recent.notifications.len(), 1);
    assert_eq!(recent.notifications[0].message, "Review queue has 7 items over threshold 5.");
    assert!(!recent.notifications[0].id.is_empty());
}
