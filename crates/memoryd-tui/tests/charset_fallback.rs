use memorum_theme::{Charset, ColorCapability, Loader};
use memoryd_tui::app::{App, AppParts, DaemonSnapshot};
use memoryd_tui::config::UiConfig;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn minimal_charset_renders_ascii_shell() {
    let app = App::from_parts(AppParts {
        config: UiConfig::default(),
        theme: Loader::resolve(Some("default-warm-dark"), None).expect("preset"),
        charset: Charset::Minimal,
        color_capability: ColorCapability::TrueColor,
        hot_reload: None,
        snapshot: DaemonSnapshot::sample(),
    });
    let backend = TestBackend::new(140, 38);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    let frame = terminal.backend().to_string();

    assert!(frame.is_ascii(), "minimal charset should be ASCII-only:\n{frame}");
    assert!(frame.contains("+Inbox"), "plain border should render for minimal charset:\n{frame}");
    assert!(frame.contains("theme:default-warm-dark"));
}
