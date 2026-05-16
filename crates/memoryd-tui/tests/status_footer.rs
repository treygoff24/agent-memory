use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd_tui::app::{App, DaemonSnapshot};
use ratatui::{backend::TestBackend, Terminal};

fn render(app: App) -> String {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

fn press(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
}

#[test]
fn inbox_footer_only_shows_review_actions_for_review_candidates() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_selected(0);
    let conflict_frame = render(app);
    assert!(conflict_frame.contains("enter inspect"));
    assert!(!conflict_frame.contains("approve"));
    assert!(!conflict_frame.contains("reject"));
    assert!(!conflict_frame.contains("forget"));

    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_selected(3);
    let review_frame = render(app);
    assert!(review_frame.contains("approve"));
    assert!(review_frame.contains("reject"));
    assert!(review_frame.contains("forget"));
}

#[test]
fn focus_footer_slates_match_wired_keymaps() {
    let inbox_frame = render(App::with_snapshot(DaemonSnapshot::sample()));
    assert!(inbox_frame.contains("INBOX"));
    assert!(inbox_frame.contains("enter inspect"));

    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.enter_reality_check_focus("session-1", 5, 12);
    let reality_frame = render(app);
    assert!(reality_frame.contains("REALITY CHECK"));
    assert!(reality_frame.contains("k correct"));
    assert!(reality_frame.contains("esc back"));
    assert!(!reality_frame.contains("y confirm"));
    assert!(!reality_frame.contains("f forget"));
    assert!(!reality_frame.contains("s skip"));

    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.enter_reality_check_focus("session-1", 5, 12);
    app.handle_event(press('k'), std::time::Instant::now());
    let editor_frame = render(app);
    assert!(editor_frame.contains("EDITOR"));
    assert!(editor_frame.contains("ctrl-s submit"));
    assert!(editor_frame.contains("enter newline"));
}

#[test]
fn footer_hint_replaces_focus_slate_temporarily() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.handle_event(press('/'), std::time::Instant::now());
    let frame = render(app);

    assert!(frame.contains("search is handled by Task 11B palette/search"));
    assert!(!frame.contains("enter inspect"));
}
