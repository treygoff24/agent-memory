use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::theme_glue::ThemeStyles;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CorrectEditorState {
    body: String,
    cursor: (usize, usize),
    hint: Option<String>,
}

impl CorrectEditorState {
    pub fn reset(&mut self) {
        self.body.clear();
        self.cursor = (0, 0);
        self.hint = None;
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub fn push_char(&mut self, ch: char) {
        self.body.push(ch);
        self.cursor.1 = self.cursor.1.saturating_add(1);
        self.hint = None;
    }

    pub fn insert_newline(&mut self) {
        self.body.push('\n');
        self.cursor.0 = self.cursor.0.saturating_add(1);
        self.cursor.1 = 0;
        self.hint = None;
    }

    pub fn backspace(&mut self) {
        if let Some(ch) = self.body.pop() {
            if ch == '\n' {
                self.cursor.0 = self.cursor.0.saturating_sub(1);
                self.cursor.1 = self.body.lines().last().map(str::len).unwrap_or(0);
            } else {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
            }
        }
    }

    pub fn show_body_required(&mut self) {
        self.hint = Some("body required".to_string());
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &CorrectEditorState, styles: &ThemeStyles) {
    let mut lines = vec![
        Line::from(Span::styled("Replacement body", styles.accent)),
        Line::from("Ctrl-S submit · Esc cancel"),
        Line::from(""),
    ];
    if state.body.is_empty() {
        lines.push(Line::from(Span::styled("Start typing the corrected memory body...", styles.muted)));
    } else {
        lines.extend(state.body.lines().map(Line::from));
    }
    lines.push(Line::from(Span::styled(styles.glyphs.cursor.clone(), styles.accent)));
    if let Some(hint) = state.hint() {
        lines.push(Line::from(Span::styled(hint.to_string(), styles.warn)));
    }
    frame.render_widget(
        Paragraph::new(lines).style(styles.base).block(
            Block::new()
                .title("Correct memory")
                .borders(Borders::ALL)
                .border_set(styles.border)
                .border_style(styles.block),
        ),
        area,
    );
}
