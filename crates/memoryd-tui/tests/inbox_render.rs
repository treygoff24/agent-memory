use memoryd_tui::app::{App, DaemonSnapshot};
use memoryd_tui::inbox::InboxFilter;
use ratatui::{backend::TestBackend, Terminal};

fn render(app: App) -> String {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

#[test]
fn inbox_renders_ranked_unified_items() {
    let frame = render(App::with_snapshot(DaemonSnapshot::sample()));

    assert!(frame.contains("Memorum"), "brand sigil + name should anchor the header");
    assert!(frame.contains("Database connection pool size"));
    assert!(frame.contains("Prefer CITEXT for email columns"));
    assert!(frame.contains("Deploy target is production ECS"));
}

#[test]
fn recall_filter_renders_recall_items_only() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_filter(InboxFilter::Recall);
    let frame = render(app);

    assert!(frame.contains("Deploy target is production ECS"));
    assert!(!frame.contains("Prefer CITEXT for email columns"));
}
