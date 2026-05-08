use memorum_theme::{presets, ColorCapability, Loader};
use memoryd_tui::app::{App, DaemonSnapshot};
use ratatui::{backend::TestBackend, Terminal};

fn render(app: App) -> String {
    let backend = TestBackend::new(140, 38);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}

#[test]
fn all_presets_render_inbox_kinds_with_theme_glyphs() {
    for (name, _) in presets::PRESETS {
        let theme = Loader::resolve(Some(name), None).expect("preset loads");
        let frame = render(App::with_theme(DaemonSnapshot::sample(), theme.clone()));

        for glyph in [
            &theme.glyphs.review,
            &theme.glyphs.conflict,
            &theme.glyphs.recall,
            &theme.glyphs.dream,
            &theme.glyphs.due,
            &theme.glyphs.memory,
        ] {
            assert!(frame.contains(glyph), "preset {name} missing glyph {glyph:?}:\n{frame}");
        }
        assert!(frame.contains("theme:"), "preset {name} should render theme label");
    }
}

#[test]
fn truecolor_render_smoke_works_for_default_fixture() {
    let theme = Loader::resolve(Some("default-warm-dark"), None).expect("preset");
    let app = memoryd_tui::app::App::from_parts(memoryd_tui::app::AppParts {
        config: memoryd_tui::config::UiConfig::default(),
        theme,
        charset: memorum_theme::Charset::Full,
        color_capability: ColorCapability::TrueColor,
        hot_reload: None,
        snapshot: DaemonSnapshot::sample(),
    });
    assert!(render(app).contains("Memorum"));
}
