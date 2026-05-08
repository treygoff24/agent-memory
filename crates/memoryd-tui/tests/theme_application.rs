use memorum_theme::Theme;
use memoryd_tui::app::{App, DaemonSnapshot};
use ratatui::{backend::TestBackend, Terminal};

fn render(app: App) -> String {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

#[test]
fn theme_changes_rendered_buffer_styles() {
    let default_frame = render(App::with_theme(DaemonSnapshot::sample(), Theme::default_warm_dark()));
    let test_frame = render(App::with_theme(DaemonSnapshot::sample(), Theme::for_test()));

    assert_ne!(default_frame, test_frame);
    assert!(test_frame.contains("Memorum"));
}
