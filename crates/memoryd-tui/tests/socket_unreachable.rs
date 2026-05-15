use memoryd_tui::app::{App, DaemonSnapshot};
use memoryd_tui::config::UiConfig;
use ratatui::{backend::TestBackend, Terminal};

fn render(app: &mut App) -> String {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal.draw(|frame| app.render(frame)).expect("frame should render");
    terminal.backend().to_string()
}

#[test]
fn test_tui_shows_unreachable_state_on_socket_failure() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.mark_socket_unreachable("/run/user/1000/memoryd.sock", "Connection refused");

    let frame = render(&mut app);

    assert!(frame.contains("Daemon unreachable"));
    assert!(frame.contains("Socket: /run/user/1000/memoryd.sock"));
    assert!(frame.contains("Error:  Connection refused"));
    assert!(frame.contains("Run `memoryd start`"));
    assert!(frame.contains("socket") && frame.contains("DOWN"));
    assert!(!frame.contains("Pending review      7"));
}

#[test]
fn test_tui_recovers_on_reconnection() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.mark_socket_unreachable("/run/user/1000/memoryd.sock", "Connection refused");
    assert!(render(&mut app).contains("Daemon unreachable"));

    app.mark_socket_connected(DaemonSnapshot::sample());
    let frame = render(&mut app);

    assert!(frame.contains("daemon"));
    assert!(frame.contains("running"));
    assert!(frame.contains("socket") && frame.contains("ok"));
    assert!(!frame.contains("Daemon unreachable"));
}

#[test]
fn test_loading_snapshot_does_not_render_sample_memory_content() {
    let mut app = App::new(UiConfig::default());

    let frame = render(&mut app);

    assert!(frame.contains("loading"));
    assert!(!frame.contains("Prefer CITEXT"));
    assert!(!frame.contains("Deploy target is production ECS"));
    assert!(!frame.contains("My preferred stack is TypeScript + Rust"));
    assert!(!frame.contains("project:atlasos"));
}
