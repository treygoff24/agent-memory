// This test inspects ratatui Color values out of the rendered Buffer to verify
// the indexed-16 capability floor produces no RGB cells. The disallowed-types
// lint enforces the theme_glue seam on the application side; tests that verify
// the seam's output need direct access to ratatui::style::Color.
#![allow(clippy::disallowed_types)]

use memorum_theme::{Charset, ColorCapability, Loader};
use memoryd_tui::app::{App, AppParts, DaemonSnapshot};
use memoryd_tui::config::UiConfig;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
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
    let buffer = terminal.backend().buffer();
    let frame = terminal.backend().to_string();

    assert_no_rgb_colors(buffer);
    assert!(has_ansi_color(buffer), "16-color floor should still apply themed ANSI colors");
    assert!(frame.contains("Memorum"));
    assert!(frame.contains("Database connection pool size"));
}

fn assert_no_rgb_colors(buffer: &Buffer) {
    for (index, cell) in buffer.content.iter().enumerate() {
        assert!(!matches!(cell.fg, Color::Rgb(..)), "cell {index} foreground should not be RGB: {:?}", cell.fg);
        assert!(!matches!(cell.bg, Color::Rgb(..)), "cell {index} background should not be RGB: {:?}", cell.bg);
    }
}

fn has_ansi_color(buffer: &Buffer) -> bool {
    buffer.content.iter().any(|cell| is_ansi_color(cell.fg) || is_ansi_color(cell.bg))
}

fn is_ansi_color(color: Color) -> bool {
    matches!(
        color,
        Color::Black
            | Color::Red
            | Color::Green
            | Color::Yellow
            | Color::Blue
            | Color::Magenta
            | Color::Cyan
            | Color::Gray
            | Color::DarkGray
            | Color::LightRed
            | Color::LightGreen
            | Color::LightYellow
            | Color::LightBlue
            | Color::LightMagenta
            | Color::LightCyan
            | Color::White
    )
}
