use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd_tui::app::{App, DaemonCall, DaemonSnapshot, Modal, ReviewAction};
use memoryd_tui::inbox::InboxFilter;

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
    app.set_filter(InboxFilter::Review);
    let review_id = app.selected_item().expect("review item").id().to_string();

    app.handle_event(press('a'), start);
    app.on_tick(start + Duration::from_millis(3_001));

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::Review { action: ReviewAction::Approve, memory_id: review_id }]
    );
}

#[test]
fn review_action_advances_cursor_and_chains_commits() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let start = Instant::now();
    app.set_filter(InboxFilter::Review);

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

#[test]
fn review_action_ignores_non_review_items_and_preserves_pending_undo() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let start = Instant::now();

    app.handle_event(press('a'), start);
    assert!(app.pending_action().is_none(), "conflicts are not review candidates");
    assert!(app.queued_daemon_calls().is_empty(), "non-review actions should not reach the daemon queue");
    assert_eq!(app.snapshot().footer_hint, "selected item is not a review candidate");

    app.set_selected(2);
    let review_id = app.selected_item().expect("review item").id().to_string();
    app.handle_event(press('a'), start + Duration::from_millis(100));
    assert_eq!(app.pending_action().expect("review action staged").memory_id(), review_id);

    app.set_selected(0);
    app.handle_event(press('r'), start + Duration::from_millis(200));
    assert_eq!(
        app.pending_action().expect("pending action remains undoable").memory_id(),
        review_id,
        "pressing an action key on a non-review row must not auto-commit the pending review action",
    );
    assert!(app.queued_daemon_calls().is_empty());
}

#[test]
fn review_action_does_not_duplicate_same_row_when_cursor_cannot_advance() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let start = Instant::now();
    app.set_filter(InboxFilter::Review);
    app.set_selected(1);
    let last_review_id = app.selected_item().expect("last review item").id().to_string();

    app.handle_event(press('a'), start);
    app.handle_event(press('r'), start + Duration::from_millis(100));

    let pending = app.pending_action().expect("original action should remain staged");
    assert_eq!(pending.memory_id(), last_review_id);
    assert_eq!(pending.action(), &ReviewAction::Approve);
    assert!(
        app.queued_daemon_calls().is_empty(),
        "same-row repeat during undo must not enqueue a duplicate/conflicting daemon call",
    );
}
