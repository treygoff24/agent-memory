use chrono::{TimeZone, Utc};
use memory_substrate::MemoryId;
use memoryd::mcp::forward_payload_to_daemon;
use memoryd::protocol::{
    NotificationEvent, RealityCheckRequest, RequestPayload, ResponseResult, NOTIFICATION_CHANNEL_CAPACITY,
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
