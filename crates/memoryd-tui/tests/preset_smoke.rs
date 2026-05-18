// This test inspects ratatui Color values out of the rendered Buffer to verify
// the theme_glue seam emits the expected RGB vs ANSI variants per capability.
// The disallowed-types lint enforces the seam on the application side; tests
// that verify the seam's output need direct access to ratatui::style::Color.
#![allow(clippy::disallowed_types)]

use memorum_theme::{presets, Charset, ColorCapability, Loader};
use memoryd_tui::app::{App, AppParts, DaemonSnapshot};
use memoryd_tui::config::UiConfig;
use memoryd_tui::theme_glue::ThemeStyles;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::{backend::TestBackend, Terminal};

#[derive(Debug)]
struct RenderedApp {
    text: String,
    buffer: Buffer,
}

fn render(app: App) -> RenderedApp {
    let backend = TestBackend::new(140, 38);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    RenderedApp { text: terminal.backend().to_string(), buffer: terminal.backend().buffer().clone() }
}

#[test]
fn all_presets_render_inbox_kinds_with_theme_glyphs() {
    for (name, _) in presets::PRESETS {
        let theme = Loader::resolve(Some(name), None).expect("preset loads");
        let frame = render(App::with_theme(DaemonSnapshot::sample(), theme.clone()));

        assert!(frame.text.contains(&theme.glyphs.brand), "preset {name} missing brand glyph:\n{}", frame.text);
        assert_row_contains(&frame.text, name, &theme.glyphs.conflict, "Database connection pool size");
        assert_row_contains(&frame.text, name, &theme.glyphs.due, "SSH key rotation every 90d");
        assert_row_contains(&frame.text, name, &theme.glyphs.review, "Prefer CITEXT for email columns");
        assert_row_contains(&frame.text, name, &theme.glyphs.review, "Dream candidate needs confirmation");
        assert_row_contains(&frame.text, name, &theme.glyphs.dream, "Daily synthesis summary ready");
        assert_row_contains(&frame.text, name, &theme.glyphs.recall, "Deploy target is production ECS");
        assert_row_contains(&frame.text, name, &theme.glyphs.memory, "Agent memory uses private daemon socket");
    }
}

#[test]
fn truecolor_render_smoke_works_for_default_fixture() {
    let theme = Loader::resolve(Some("default-warm-dark"), None).expect("preset");
    let expected_truecolor_accent = ThemeStyles::from_theme(&theme, ColorCapability::TrueColor)
        .accent
        .fg
        .expect("truecolor accent style should set foreground");
    let indexed_accent = ThemeStyles::from_theme(&theme, ColorCapability::Indexed16)
        .accent
        .fg
        .expect("indexed accent style should set foreground");

    let truecolor_frame = render(App::from_parts(AppParts {
        config: memoryd_tui::config::UiConfig::default(),
        theme: theme.clone(),
        charset: memorum_theme::Charset::Full,
        color_capability: ColorCapability::TrueColor,
        hot_reload: None,
        snapshot: DaemonSnapshot::sample(),
    }));
    let indexed_frame = render(App::from_parts(AppParts {
        config: UiConfig::default(),
        theme,
        charset: Charset::Full,
        color_capability: ColorCapability::Indexed16,
        hot_reload: None,
        snapshot: DaemonSnapshot::sample(),
    }));

    assert!(truecolor_frame.text.contains("Memorum"));
    assert!(matches!(expected_truecolor_accent, Color::Rgb(..)));
    assert_ne!(expected_truecolor_accent, indexed_accent);
    assert!(
        buffer_contains_color(&truecolor_frame.buffer, expected_truecolor_accent),
        "truecolor render should apply the RGB accent color to visible cells"
    );
    assert!(
        !buffer_contains_rgb(&indexed_frame.buffer),
        "indexed render should not contain RGB cells when a lower capability is selected"
    );
}

fn assert_row_contains(frame: &str, preset_name: &str, glyph: &str, title: &str) {
    let fragment = format!("{glyph} {title}");
    assert!(
        frame.contains(&fragment),
        "preset {preset_name} should render glyph {glyph:?} beside title {title:?}:\n{frame}"
    );
}

fn buffer_contains_color(buffer: &Buffer, color: Color) -> bool {
    buffer.content.iter().any(|cell| cell.fg == color || cell.bg == color)
}

fn buffer_contains_rgb(buffer: &Buffer) -> bool {
    buffer.content.iter().any(|cell| matches!(cell.fg, Color::Rgb(..)) || matches!(cell.bg, Color::Rgb(..)))
}
