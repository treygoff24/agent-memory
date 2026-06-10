use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::state::RealityCheckState;
use crate::theme_glue::ThemeStyles;

/// Width of the inline drift-component bar, in cells.
const BAR_WIDTH: usize = 16;
/// Component value at/above which a line is treated as high-drift (warn cue).
const HIGH_DRIFT: f64 = 0.66;
/// Component value at/above which a line is treated as elevated-drift (info cue).
const ELEVATED_DRIFT: f64 = 0.33;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let state = app.reality_check_state();
    let progress = app.focus_transition_percent();
    let gauge = progress_gauge(
        state.items_reviewed,
        state.items_total,
        app.theme().glyphs.progress_filled.as_str(),
        app.theme().glyphs.progress_empty.as_str(),
    );

    let mut body = vec![
        Line::from(vec![
            Span::styled(state.progress_label(), styles.accent),
            Span::styled(format!("  transition:{progress}%"), styles.muted),
        ]),
        Line::from(gauge),
        Line::from(state.current_title.as_deref().unwrap_or("No Reality Check item selected.").to_owned()),
    ];

    body.extend(score_breakdown_lines(state, styles));

    body.push(Line::from(""));
    body.push(Line::from(vec![
        Span::styled("y", styles.accent),
        Span::styled(" confirm · ", styles.muted),
        Span::styled("k", styles.accent),
        Span::styled(" correct · ", styles.muted),
        Span::styled("f", styles.accent),
        Span::styled(" forget · ", styles.muted),
        Span::styled("n", styles.accent),
        Span::styled(" not-relevant · ", styles.muted),
        Span::styled("s", styles.accent),
        Span::styled(" skip · ", styles.muted),
        Span::styled("Esc", styles.accent),
        Span::styled(" inbox", styles.muted),
    ]));

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

/// Render the per-memory drift-score breakdown: a header with the total score,
/// then one severity-colored bar per component.
fn score_breakdown_lines(state: &RealityCheckState, styles: &ThemeStyles) -> Vec<Line<'static>> {
    let Some(breakdown) = state.current_breakdown else {
        return vec![Line::from(Span::styled("Score breakdown unavailable for this item.", styles.muted))];
    };

    let mut lines = Vec::with_capacity(7);
    let total = state.current_score.map_or_else(|| "—".to_owned(), |score| format!("{score:.2}"));
    let mut header =
        vec![Span::styled("Score breakdown", styles.accent), Span::styled(format!("  total {total}"), styles.muted)];
    if state.current_encrypted {
        header.push(Span::styled("  · encrypted", styles.info));
    }
    lines.push(Line::from(header));

    for (label, value) in breakdown.components() {
        lines.push(component_line(label, value, styles));
    }
    lines
}

/// One drift component rendered as `label  ███░░░  0.91`, where the bar fill and
/// value are colored by severity (high drift → warn, elevated → info, else ok).
fn component_line(label: &str, value: f64, styles: &ThemeStyles) -> Line<'static> {
    let style = severity_style(value, styles);
    let filled = bar_fill(value);
    Line::from(vec![
        Span::styled(format!("{label:>18}  "), styles.muted),
        Span::styled("█".repeat(filled), style),
        Span::styled("░".repeat(BAR_WIDTH.saturating_sub(filled)), styles.dim),
        Span::styled(format!("  {value:.2}"), style),
    ])
}

/// Map a normalized [0,1] drift component to a theme severity style.
pub fn severity_style(value: f64, styles: &ThemeStyles) -> Style {
    if value >= HIGH_DRIFT {
        styles.warn
    } else if value >= ELEVATED_DRIFT {
        styles.info
    } else {
        styles.ok
    }
}

fn bar_fill(value: f64) -> usize {
    let clamped = value.clamp(0.0, 1.0);
    (clamped * BAR_WIDTH as f64).round() as usize
}

fn progress_gauge(reviewed: usize, total: usize, filled: &str, empty: &str) -> String {
    if total == 0 {
        return empty.repeat(10);
    }
    let filled_count = reviewed.saturating_mul(10) / total;
    format!("{}{}", filled.repeat(filled_count), empty.repeat(10usize.saturating_sub(filled_count)))
}
