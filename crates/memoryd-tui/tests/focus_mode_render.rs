use memoryd_tui::app::{App, DaemonSnapshot};
use memoryd_tui::inbox::InboxFilter;
use ratatui::{backend::TestBackend, Terminal};

fn render(app: &App) -> String {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

/// Focus the Reality Check view on the sample's due item so the score breakdown
/// resolves from `snapshot.due`.
fn app_focused_on_due_item(reviewed: usize, total: usize) -> App {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_filter(InboxFilter::Due);
    app.set_selected(0);
    app.enter_reality_check_focus("session-1", reviewed, total);
    app
}

#[test]
fn focus_mode_renders_reality_check_takeover() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.enter_reality_check_focus("session-1", 5, 12);
    let frame = render(&app);

    assert!(frame.contains("Reality Check focus"));
    assert!(frame.contains("5 of 12"));
    assert!(!frame.contains("0 of 12"));
}

#[test]
fn focus_mode_renders_all_five_drift_components() {
    let app = app_focused_on_due_item(5, 12);
    let frame = render(&app);

    assert!(frame.contains("Score breakdown"), "breakdown header missing:\n{frame}");
    for component in ["recency", "recall_frequency", "corroboration", "confidence_decay", "sensitivity"] {
        assert!(frame.contains(component), "missing drift component {component} in:\n{frame}");
    }
    // Sample total score is 0.82.
    assert!(frame.contains("total 0.82"), "breakdown total missing:\n{frame}");
}

#[test]
fn focus_mode_footer_advertises_not_relevant_keybind() {
    let app = app_focused_on_due_item(0, 7);
    let frame = render(&app);

    assert!(frame.contains("not-relevant"), "footer should advertise not-relevant action:\n{frame}");
}
