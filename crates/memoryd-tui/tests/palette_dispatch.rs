use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd_tui::app::{App, DaemonSnapshot, Modal};
use memoryd_tui::inbox::InboxFilter;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_query(app: &mut App, query: &str) {
    let now = std::time::Instant::now();
    app.handle_event(key(KeyCode::Char(':')), now);
    for ch in query.chars() {
        app.handle_event(key(KeyCode::Char(ch)), now);
    }
}

#[test]
fn palette_dispatches_filter_command() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    type_query(&mut app, "filter review");
    app.handle_event(key(KeyCode::Enter), std::time::Instant::now());

    assert_eq!(app.filter(), InboxFilter::Review);
    assert_eq!(app.modal(), None);
}

#[test]
fn palette_switches_theme_for_session() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    type_query(&mut app, "kanagawa");
    app.handle_event(key(KeyCode::Enter), std::time::Instant::now());

    assert_eq!(app.theme_name(), "kanagawa");
    assert_eq!(app.modal(), None);
}

#[test]
fn unknown_palette_command_stays_open_with_no_match() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    type_query(&mut app, "zzzz-nope");
    app.handle_event(key(KeyCode::Enter), std::time::Instant::now());

    assert_eq!(app.modal(), Some(&Modal::CommandPrompt));
    assert_eq!(app.palette().message(), Some("No matching command"));
}
