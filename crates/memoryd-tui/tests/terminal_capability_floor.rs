use memorum_theme::{Charset, ColorCapability, Loader};
use memoryd_tui::app::{App, AppParts, DaemonSnapshot};
use memoryd_tui::config::UiConfig;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn indexed16_floor_uses_no_rgb_cells_and_preserves_layout_text() {
    let app = App::from_parts(AppParts {
        config: UiConfig::default(),
        theme: Loader::resolve(Some("default-warm-dark"), None).expect("preset"),
        charset: Charset::Full,
        color_capability: ColorCapability::Indexed16,
        hot_reload: None,
        snapshot: DaemonSnapshot::sample(),
    });
    let backend = TestBackend::new(140, 38);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    let buffer_debug = format!("{:?}", terminal.backend().buffer());
    let frame = terminal.backend().to_string();

    assert!(!buffer_debug.contains("Rgb("), "16-color floor should not leave truecolor cells");
    assert!(frame.contains("Memorum"));
    assert!(frame.contains("Database connection pool size"));
}
