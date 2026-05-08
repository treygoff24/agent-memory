use memoryd_tui::app::{App, DaemonSnapshot};
use ratatui::{backend::TestBackend, Terminal};

fn render(app: App) -> String {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

#[test]
fn focus_mode_renders_reality_check_takeover() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.enter_reality_check_focus("session-1", 5, 12);
    let frame = render(app);

    assert!(frame.contains("Reality Check focus"));
    assert!(frame.contains("5 of 12"));
    assert!(!frame.contains("0 of 12"));
}
