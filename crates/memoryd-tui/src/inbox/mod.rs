pub mod filter;
pub mod item;
pub mod ranking;

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Padding};
use ratatui::Frame;

pub use filter::{FilterCounts, InboxFilter};
pub use item::{InboxItem, InboxKind};

use crate::theme_glue::ThemeStyles;

pub struct InboxRenderContext<'a> {
    pub items: &'a [InboxItem],
    pub selected: usize,
    pub styles: &'a ThemeStyles,
}

pub fn render(frame: &mut Frame<'_>, area: Rect, context: InboxRenderContext<'_>) {
    let rows = if context.items.is_empty() {
        vec![ListItem::new(Line::from("No inbox items yet."))]
    } else {
        context
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let is_selected = index == context.selected;
                let gutter = if is_selected {
                    Span::styled(context.styles.glyphs.selection_gutter.clone(), context.styles.selection_gutter)
                } else {
                    Span::raw(" ")
                };
                let kind_glyph = kind_glyph(item.kind(), context.styles);
                let title_style = if is_selected {
                    context.styles.base.add_modifier(Modifier::BOLD)
                } else {
                    context.styles.base
                };
                ListItem::new(vec![
                    Line::from(vec![
                        gutter.clone(),
                        Span::raw(" "),
                        Span::styled(kind_glyph, context.styles.accent),
                        Span::raw(" "),
                        Span::styled(item.title().to_string(), title_style),
                    ]),
                    Line::from(vec![
                        gutter,
                        Span::raw(" "),
                        Span::styled(
                            format!(
                                "{} {} {} {} {}",
                                item.namespace(),
                                context.styles.glyphs.pill_separator,
                                item.age_label(),
                                context.styles.glyphs.pill_separator,
                                item.id()
                            ),
                            context.styles.muted,
                        ),
                    ]),
                ])
            })
            .collect()
    };
    frame.render_widget(
        List::new(rows).block(
            Block::new()
                .borders(Borders::RIGHT)
                .border_set(context.styles.border)
                .border_style(context.styles.block)
                .padding(Padding::new(1, 1, 0, 0)),
        ),
        area,
    );
}

fn kind_glyph(kind: InboxKind, styles: &ThemeStyles) -> String {
    match kind {
        InboxKind::Review => styles.glyphs.review.clone(),
        InboxKind::Conflict => styles.glyphs.conflict.clone(),
        InboxKind::Recall => styles.glyphs.recall.clone(),
        InboxKind::Due => styles.glyphs.due.clone(),
        InboxKind::Dream => styles.glyphs.dream.clone(),
        InboxKind::Memory => styles.glyphs.memory.clone(),
    }
}
