use chrono::{Duration, TimeZone, Utc};
use memoryd::handlers::HandlerState;
use memoryd::protocol::NotificationEvent;
use memoryd::reality_check::RcScheduler;
use memoryd::state::RealityCheckState;

#[test]
fn test_due_after_7_days() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(8)), snooze_until: None };

    assert!(RcScheduler::default().is_due(&state, now));
}

#[test]
fn test_not_due_within_7_days() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(5)), snooze_until: None };

    assert!(!RcScheduler::default().is_due(&state, now));
}

#[test]
fn test_snoozed_not_due() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState {
        last_completed_at: Some(now - Duration::days(8)),
        snooze_until: Some(now + Duration::days(1)),
    };

    assert!(!RcScheduler::default().is_due(&state, now));
}

#[test]
fn test_overdue_after_21_days() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(22)), snooze_until: None };

    assert!(RcScheduler::default().is_overdue(&state, now));
}

#[test]
fn test_overdue_when_never_completed() {
    // A fresh install with no completed Reality Check must be considered overdue —
    // the user has never run one, which is at least as overdue as a 21-day lapse.
    // Spec §5.5: `is_overdue` mirrors `is_due` semantics where None means "infinitely
    // overdue" rather than "never due". This guards against the `is_some_and` trap
    // that returns `false` when `last_completed_at` is None.
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState { last_completed_at: None, snooze_until: None };

    assert!(RcScheduler::default().is_overdue(&state, now));
}

#[test]
fn test_invalid_cron_falls_back_to_default() {
    let scheduler = RcScheduler::new("not a cron expression");

    assert_eq!(scheduler.schedule().expression(), "0 9 * * SUN");
}

#[tokio::test]
async fn test_notification_event_fired_when_due() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(8)), snooze_until: None };
    let (sender, mut receiver) = tokio::sync::broadcast::channel(4);

    assert!(RcScheduler::default().check_and_fire_if_due(&state, now, &sender));

    let event = receiver.recv().await.expect("due event sent");
    assert_eq!(event, NotificationEvent::RealityCheckDue { due_at: now });
}

#[tokio::test]
async fn test_notification_event_not_fired_when_not_due() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(5)), snooze_until: None };
    let (sender, mut receiver) = tokio::sync::broadcast::channel(4);

    assert!(!RcScheduler::default().check_and_fire_if_due(&state, now, &sender));

    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn test_handler_state_fires_due_notification_through_shared_channel() {
    let now = instant("2026-05-01T12:00:00Z");
    let state = HandlerState::new();
    let mut receiver = state.subscribe_notifications();
    let reality_check = RealityCheckState { last_completed_at: Some(now - Duration::days(8)), snooze_until: None };

    assert!(state.fire_reality_check_due_if_due(&reality_check, now));

    let event = receiver.recv().await.expect("due event sent");
    assert_eq!(event, NotificationEvent::RealityCheckDue { due_at: now });
}

fn instant(value: &str) -> chrono::DateTime<Utc> {
    Utc.from_utc_datetime(&chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%SZ").unwrap())
}
