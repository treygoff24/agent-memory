use memoryd_tui::app::{App, DaemonSnapshot};
use ratatui::{backend::TestBackend, Terminal};

fn render(app: App) -> String {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

#[test]
fn inspector_routes_conflict_item() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_selected(0);
    let frame = render(app);

    assert!(frame.contains("Blocking merge conflict"));
    assert!(frame.contains("Pool size: 20 vs Pool size: 30"));
}

#[test]
fn inspector_routes_review_item() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_selected(3);
    let frame = render(app);

    assert!(frame.contains("Review candidate"));
    assert!(frame.contains("Body"));
    assert!(frame.contains("Use CITEXT for the users.email column"));
    assert!(frame.contains("requires_user_confirmation"));
}

#[test]
fn inspector_marks_truncated_review_body() {
    let mut snapshot = DaemonSnapshot::sample();
    snapshot.review_queue[0].body_truncated = true;
    let mut app = App::with_snapshot(snapshot);
    app.set_selected(3);
    let frame = render(app);

    assert!(frame.contains("Body"));
    assert!(frame.contains("truncated"));
}
