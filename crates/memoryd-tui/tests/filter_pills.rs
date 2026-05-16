use memorum_theme::{Charset, ColorCapability, Theme};
use memoryd_tui::app::{App, AppParts, DaemonSnapshot};
use memoryd_tui::config::UiConfig;
use memoryd_tui::inbox::{FilterCounts, InboxFilter};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn filter_counts_cover_all_pills() {
    let app = App::with_snapshot(DaemonSnapshot::sample());
    let counts = FilterCounts::from_items(app.inbox_items());

    assert_eq!(counts.get(InboxFilter::All), 7);
    assert_eq!(counts.get(InboxFilter::Conflicts), 1);
    assert_eq!(counts.get(InboxFilter::Review), 2);
    assert_eq!(counts.get(InboxFilter::Recall), 1);
    assert_eq!(counts.get(InboxFilter::Dreams), 1);
    assert_eq!(counts.get(InboxFilter::Due), 1);
}

#[test]
fn header_renders_filter_pills_with_counts() {
    let app = App::with_theme(DaemonSnapshot::sample(), Theme::default_warm_dark());
    let frame = render_app(&app);

    for label in expected_filter_labels("·") {
        assert!(frame.contains(&label), "missing rendered filter pill {label:?}\n{frame}");
    }
}

#[test]
fn header_renders_ascii_filter_pills_when_charset_is_minimal() {
    let app = App::from_parts(AppParts {
        config: UiConfig::default(),
        theme: Theme::default_warm_dark(),
        charset: Charset::Minimal,
        color_capability: ColorCapability::TrueColor,
        hot_reload: None,
        snapshot: DaemonSnapshot::sample(),
    });
    let frame = render_app(&app);

    for label in expected_filter_labels("|") {
        assert!(frame.contains(&label), "missing rendered ASCII filter pill {label:?}\n{frame}");
    }
}

fn expected_filter_labels(separator: &str) -> Vec<String> {
    [("all", 7), ("review", 2), ("conflicts", 1), ("recall", 1), ("dreams", 1), ("due", 1)]
        .into_iter()
        .map(|(label, count)| format!("{label}{separator}{count}"))
        .collect()
}

fn render_app(app: &App) -> String {
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("render");
    terminal.backend().to_string()
}
