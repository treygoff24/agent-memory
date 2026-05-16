use memoryd_tui::app::{App, DaemonSnapshot, SocketState};
use memoryd_tui::client::DaemonClient;
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

#[tokio::test]
async fn poll_daemon_marks_missing_socket_unreachable_through_client() {
    let socket = std::path::PathBuf::from(format!("/tmp/memoryd-tui-missing-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let config = UiConfig { socket_path: socket.clone(), ..UiConfig::default() };
    let client = DaemonClient::new(&socket);
    let mut app = App::new(config);

    app.poll_daemon(&client).await;

    match app.socket_state() {
        SocketState::Unreachable { path, error } => {
            assert_eq!(path, &socket);
            assert!(error.contains(&socket.display().to_string()), "error should name socket path: {error}");
        }
        SocketState::Connected => panic!("missing daemon socket should mark app unreachable"),
    }
    let frame = render(&mut app);
    assert!(frame.contains("Daemon unreachable"));
    assert!(frame.contains(&format!("Socket: {}", socket.display())));
}
