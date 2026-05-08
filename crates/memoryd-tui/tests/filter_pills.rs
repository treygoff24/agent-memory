use memoryd_tui::app::{App, DaemonSnapshot};
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
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = App::with_snapshot(DaemonSnapshot::sample());

    terminal.draw(|frame| app.render(frame)).expect("render");
    let frame = terminal.backend().to_string();

    assert!(frame.contains("all·7"));
    assert!(frame.contains("review·2"));
    assert!(frame.contains("conflicts·1"));
}
