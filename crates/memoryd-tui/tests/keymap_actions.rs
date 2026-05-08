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
    app.on_tick(start + Duration::from_millis(1_001));

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::Review {
            action: ReviewAction::Approve,
            memory_id: "mem_20260501_0123456789abcdef_000002".into()
        }]
    );
}
