use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd_tui::app::{App, DaemonSnapshot, Modal};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn colon_opens_palette_and_escape_closes() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.handle_event(key(KeyCode::Char(':')), std::time::Instant::now());

    assert_eq!(app.modal(), Some(&Modal::CommandPrompt));
    assert_eq!(app.palette().input(), "");

    app.handle_event(key(KeyCode::Esc), std::time::Instant::now());
    assert_eq!(app.modal(), None);
}

#[test]
fn palette_accepts_text_and_moves_selection() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    let now = std::time::Instant::now();
    app.handle_event(key(KeyCode::Char(':')), now);
    app.handle_event(key(KeyCode::Char('t')), now);
    app.handle_event(key(KeyCode::Char('h')), now);
    app.handle_event(key(KeyCode::Down), now);

    assert_eq!(app.palette().input(), "th");
    assert!(app.palette().selected_label().is_some());
}
