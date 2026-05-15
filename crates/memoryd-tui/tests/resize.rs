use memoryd_tui::app::{App, DaemonSnapshot};
use memoryd_tui::inbox::InboxFilter;
use ratatui::{backend::TestBackend, Terminal};

fn render(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal.draw(|frame| app.render(frame)).expect("frame should render");
    terminal.backend().to_string()
}

#[test]
fn test_below_minimum_shows_warning_banner() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());

    let frame = render(&mut app, 79, 23);

    assert!(frame.contains("Terminal too small"));
    assert!(frame.contains("current: 79x23"));
    assert!(frame.contains("minimum: 80x24"));
    assert!(!frame.contains("Pending review      7"));
}

#[test]
fn test_resize_above_minimum_resumes() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_filter(InboxFilter::Review);

    let small = render(&mut app, 79, 23);
    assert!(small.contains("Terminal too small"));

    let normal = render(&mut app, 80, 24);
    assert!(normal.contains("Memorum"));
    assert!(normal.contains("Prefer CITEXT"));
    assert!(!normal.contains("Terminal too small"));
}
