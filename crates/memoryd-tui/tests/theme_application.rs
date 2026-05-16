use memorum_theme::{ColorCapability, OklchColor, Theme};
use memoryd_tui::app::{App, DaemonSnapshot};
use memoryd_tui::theme_glue::ThemeStyles;
use ratatui::buffer::{Buffer, Cell};
use ratatui::{backend::TestBackend, Terminal};

#[derive(Debug)]
struct RenderedApp {
    text: String,
    buffer: Buffer,
}

fn render(app: App) -> RenderedApp {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    RenderedApp { text: terminal.backend().to_string(), buffer: terminal.backend().buffer().clone() }
}

#[test]
fn theme_changes_rendered_buffer_styles() {
    let default_theme = Theme::default_warm_dark();
    let modified_theme = color_only_test_theme();
    let expected_modified_accent = ThemeStyles::from_theme(&modified_theme, ColorCapability::TrueColor).accent;

    let default_frame = render(App::with_theme(DaemonSnapshot::sample(), default_theme));
    let modified_frame = render(App::with_theme(DaemonSnapshot::sample(), modified_theme));

    assert_eq!(default_frame.text, modified_frame.text);
    assert!(modified_frame.text.contains("Memorum"));

    let default_brand_cell = cell_at(&default_frame.buffer, 1, 0);
    let modified_brand_cell = cell_at(&modified_frame.buffer, 1, 0);

    assert_eq!(default_brand_cell.symbol(), modified_brand_cell.symbol());
    assert_ne!(default_brand_cell.fg, modified_brand_cell.fg);
    assert_eq!(modified_brand_cell.fg, expected_modified_accent.fg.expect("accent style should set foreground"));
    assert_eq!(modified_brand_cell.modifier, expected_modified_accent.add_modifier);
}

fn color_only_test_theme() -> Theme {
    let mut theme = Theme::default_warm_dark();
    theme.colors.accent = OklchColor::parse("#ff0000").expect("test color literal parses");
    theme
}

fn cell_at(buffer: &Buffer, x: u16, y: u16) -> &Cell {
    buffer.cell((x, y)).expect("stable test cell should be inside rendered buffer")
}
