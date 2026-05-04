use memoryd_tui::app::{App, DaemonSnapshot, PanelId};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn recall_panel_renders_histogram_and_hit_rows() {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_active_panel(PanelId::Recall);

    terminal.draw(|frame| app.render(frame)).expect("frame should render");
    let frame = terminal.backend().to_string();

    assert!(frame.contains("Recall"));
    assert!(frame.contains("Hourly density"));
    assert!(frame.contains("mem_20260501_0123456789abcdef_000009"));
    assert!(frame.contains("Deploy target is production ECS"));
    assert!(frame.contains("score:n/a"));
    assert!(frame.contains("harness:n/a"));
    assert!(frame.contains("session:n/a"));
}

#[test]
fn recall_panel_empty_state_is_operator_readable() {
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut app = App::with_snapshot(DaemonSnapshot::empty());
    app.set_active_panel(PanelId::Recall);

    terminal.draw(|frame| app.render(frame)).expect("frame should render");
    let frame = terminal.backend().to_string();

    assert!(frame.contains("No recall hits yet"));
    assert!(frame.contains("startup recall"));
}
