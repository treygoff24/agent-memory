use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd_tui::app::{App, DaemonCall, DaemonSnapshot, Modal, ReviewAction};

fn press(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
}

#[test]
fn tab_cycles_filters_and_question_opens_help() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let start = Instant::now();

    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)), start);
    assert_eq!(app.filter().label(), "review");
    app.handle_event(press('?'), start);
    assert_eq!(app.modal(), Some(&Modal::HelpOverlay));
}

#[test]
fn review_action_waits_for_undo_window() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let start = Instant::now();

    app.handle_event(press('a'), start);
    app.on_tick(start + Duration::from_millis(3_001));

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::Review {
            action: ReviewAction::Approve,
            memory_id: "mem_20260501_0123456789abcdef_000002".into()
        }]
    );
}

#[test]
fn review_action_advances_cursor_and_chains_commits() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let start = Instant::now();

    let first_selected = app.selected_item().map(|item| item.id().to_string()).expect("first item");
    app.handle_event(press('a'), start);
    let second_selected = app.selected_item().map(|item| item.id().to_string()).expect("second item");
    assert_ne!(first_selected, second_selected, "cursor should advance after staging an action");
    assert!(app.pending_action().is_some(), "first action is staged, not yet committed");
    assert!(app.queued_daemon_calls().is_empty(), "first action should still be in the undo window");

    app.handle_event(press('r'), start + Duration::from_millis(100));
    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::Review { action: ReviewAction::Approve, memory_id: first_selected.clone() }],
        "pressing a second action mid-window auto-commits the first",
    );
    assert!(app.pending_action().is_some(), "second action is now the pending one");

    app.handle_event(press('u'), start + Duration::from_millis(200));
    assert!(app.pending_action().is_none(), "u clears the pending action");
    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::Review { action: ReviewAction::Approve, memory_id: first_selected }],
        "u does not commit the cleared action",
    );
}
