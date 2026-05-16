use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, ReviewAction, SocketState};
use crate::inbox::InboxItem;
use crate::state::FocusKind;
use crate::theme_glue::ThemeStyles;

const REVIEW_HINTS: &[(&str, &str)] = &[
    ("a", "approve"),
    ("r", "reject"),
    ("f", "forget"),
    ("enter", "inspect"),
    ("tab", "filter"),
    (":", "palette"),
    ("?", "help"),
];
const INBOX_HINTS: &[(&str, &str)] = &[("enter", "inspect"), ("tab", "filter"), (":", "palette"), ("?", "help")];
const REALITY_CHECK_HINTS: &[(&str, &str)] = &[("k", "correct"), ("esc", "back")];
const CORRECT_EDITOR_HINTS: &[(&str, &str)] = &[("ctrl-s", "submit"), ("esc", "back"), ("enter", "newline")];

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(0), Constraint::Length(16)])
        .split(area);

    frame.render_widget(Paragraph::new(vitals_line(app, styles)).style(styles.base), chunks[0]);
    frame.render_widget(Paragraph::new(hint_line(app, styles)).style(styles.base), chunks[1]);
    frame.render_widget(Paragraph::new(focus_label_line(app, styles)).style(styles.base), chunks[2]);
}

fn vitals_line<'a>(app: &'a App, styles: &'a ThemeStyles) -> Line<'a> {
    let sep = &styles.glyphs.pill_separator;
    let socket = match app.socket_state() {
        SocketState::Connected => Span::styled(format!("socket{sep}ok"), styles.ok),
        SocketState::Unreachable { .. } => Span::styled(format!("socket{sep}DOWN"), styles.bad),
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("daemon{sep}{}", app.snapshot().daemon_state), styles.muted),
        Span::raw("  "),
        socket,
    ])
}

fn hint_line<'a>(app: &'a App, styles: &'a ThemeStyles) -> Line<'a> {
    if let Some(pending) = app.pending_action() {
        let (verb, style) = match pending.action() {
            ReviewAction::Approve => ("approved", styles.ok),
            ReviewAction::Reject => ("rejected", styles.warn),
            ReviewAction::Forget => ("forgotten", styles.bad),
        };
        return Line::from(vec![
            Span::raw(" "),
            Span::styled(verb, style),
            Span::raw("   "),
            Span::styled("u", styles.accent),
            Span::raw(" "),
            Span::styled("undo", styles.muted),
        ]);
    }
    let footer_hint = app.snapshot().footer_hint.as_str();
    if !footer_hint.is_empty() {
        return Line::from(vec![Span::raw(" "), Span::styled(footer_hint.to_string(), styles.warn)]);
    }
    let hints = match app.focus() {
        FocusKind::None if matches!(app.selected_item(), Some(InboxItem::ReviewCandidate { .. })) => REVIEW_HINTS,
        FocusKind::None => INBOX_HINTS,
        FocusKind::RealityCheck { .. } => REALITY_CHECK_HINTS,
        FocusKind::CorrectEditor { .. } => CORRECT_EDITOR_HINTS,
    };
    let mut spans: Vec<Span<'a>> = Vec::with_capacity(hints.len() * 4);
    spans.push(Span::raw(" "));
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(*key, styles.accent));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(*label, styles.muted));
    }
    Line::from(spans)
}

fn focus_label_line<'a>(app: &'a App, styles: &'a ThemeStyles) -> Line<'a> {
    let label = match app.focus() {
        FocusKind::None => "INBOX",
        FocusKind::RealityCheck { .. } => "REALITY CHECK",
        FocusKind::CorrectEditor { .. } => "EDITOR",
    };
    let padded = format!("{label:>15} ");
    Line::from(Span::styled(padded, styles.dim))
}
