pub mod fields;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::inbox::InboxItem;
use crate::inspector::fields::kv;
use crate::theme_glue::ThemeStyles;

pub fn render(frame: &mut Frame<'_>, area: Rect, item: Option<&InboxItem>, styles: &ThemeStyles) {
    let body = match item {
        Some(item @ InboxItem::ReviewCandidate { .. }) => review_view(item, styles),
        Some(item @ InboxItem::Conflict { .. }) => conflict_view(item, styles),
        Some(InboxItem::RecallHit { id, title, namespace, .. }) => recall_view(id, title, namespace, styles),
        Some(item @ InboxItem::RealityCheckDue { .. }) => due_view(item, styles),
        Some(InboxItem::DreamOutput { id, title, namespace, .. }) => dream_view(id, title, namespace, styles),
        Some(InboxItem::Memory { id, title, namespace, .. }) => memory_view(id, title, namespace, styles),
        None => vec![Line::from("Select an inbox item to inspect it.")],
    };
    frame.render_widget(
        Paragraph::new(body).style(styles.base).block(
            Block::new().title("Inspector").borders(Borders::ALL).border_set(styles.border).border_style(styles.block),
        ),
        area,
    );
}

fn review_view<'a>(item: &'a InboxItem, styles: &ThemeStyles) -> Vec<Line<'a>> {
    let InboxItem::ReviewCandidate { id, title, namespace, reason, .. } = item else {
        return Vec::new();
    };
    vec![
        Line::from("Review candidate"),
        kv("id", id, styles),
        kv("title", title, styles),
        kv("scope", namespace, styles),
        kv("policy", reason.as_deref().unwrap_or("requires_user_confirmation"), styles),
        Line::from("Actions: a approve · r reject · f forget · enter detail"),
    ]
}

fn conflict_view<'a>(item: &'a InboxItem, styles: &ThemeStyles) -> Vec<Line<'a>> {
    let InboxItem::Conflict { id, title, namespace, reason, .. } = item else {
        return Vec::new();
    };
    vec![
        Line::from("Blocking merge conflict"),
        kv("id", id, styles),
        kv("title", title, styles),
        kv("scope", namespace, styles),
        kv("reason", reason.as_deref().unwrap_or("merge quarantine"), styles),
    ]
}

fn recall_view<'a>(id: &'a str, title: &'a str, namespace: &'a str, styles: &ThemeStyles) -> Vec<Line<'a>> {
    vec![Line::from("Recall hit"), kv("id", id, styles), kv("title", title, styles), kv("scope", namespace, styles)]
}

fn due_view<'a>(item: &'a InboxItem, styles: &ThemeStyles) -> Vec<Line<'a>> {
    let InboxItem::RealityCheckDue { id, title, namespace, score, .. } = item else {
        return Vec::new();
    };
    vec![
        Line::from("Reality Check due"),
        kv("id", id, styles),
        kv("title", title, styles),
        kv("scope", namespace, styles),
        kv("score", score, styles),
    ]
}

fn dream_view<'a>(id: &'a str, title: &'a str, namespace: &'a str, styles: &ThemeStyles) -> Vec<Line<'a>> {
    vec![Line::from("Dream output"), kv("id", id, styles), kv("title", title, styles), kv("scope", namespace, styles)]
}

fn memory_view<'a>(id: &'a str, title: &'a str, namespace: &'a str, styles: &ThemeStyles) -> Vec<Line<'a>> {
    vec![Line::from("Memory"), kv("id", id, styles), kv("title", title, styles), kv("scope", namespace, styles)]
}
