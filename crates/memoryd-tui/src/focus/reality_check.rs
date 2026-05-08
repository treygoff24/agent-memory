use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::theme_glue::ThemeStyles;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let state = app.reality_check_state();
    let progress = app.focus_transition_percent();
    let gauge = progress_gauge(
        state.items_reviewed,
        state.items_total,
        app.theme().glyphs.progress_filled.as_str(),
        app.theme().glyphs.progress_empty.as_str(),
    );
    let body = vec![
        Line::from(vec![
            Span::styled(state.progress_label(), styles.accent),
            Span::styled(format!("  transition:{progress}%"), styles.muted),
        ]),
        Line::from(gauge),
        Line::from(state.current_title.as_deref().unwrap_or("No Reality Check item selected.")),
        Line::from("y confirm · k correct · f forget · s skip · Esc inbox"),
    ];
    frame.render_widget(
        Paragraph::new(body).style(styles.base).block(
            Block::new()
                .title("Reality Check focus")
                .borders(Borders::ALL)
                .border_set(styles.border)
                .border_style(styles.block),
        ),
        area,
    );
}

fn progress_gauge(reviewed: usize, total: usize, filled: &str, empty: &str) -> String {
    if total == 0 {
        return empty.repeat(10);
    }
    let filled_count = reviewed.saturating_mul(10) / total;
    format!("{}{}", filled.repeat(filled_count), empty.repeat(10usize.saturating_sub(filled_count)))
}
