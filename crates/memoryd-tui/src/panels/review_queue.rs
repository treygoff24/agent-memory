use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand, ReviewAction};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewQueueState {
    cursor: usize,
    filter_open: bool,
    namespace_filter_enabled: bool,
}

impl ReviewQueueState {
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
}

pub fn handle_key(key: &KeyEvent, state: &mut ReviewQueueState, row_count: usize) -> Option<PanelCommand> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = next_cursor(state.cursor, row_count);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cursor = previous_cursor(state.cursor);
            None
        }
        KeyCode::Enter => Some(PanelCommand::OpenModal(Modal::MemoryDetail)),
        KeyCode::Char('/') => {
            state.filter_open = true;
            None
        }
        KeyCode::Tab => {
            state.namespace_filter_enabled = !state.namespace_filter_enabled;
            None
        }
        KeyCode::Char('a') => Some(PanelCommand::StageReview(ReviewAction::Approve)),
        KeyCode::Char('r') => Some(PanelCommand::StageReview(ReviewAction::Reject)),
        KeyCode::Char('f') => Some(PanelCommand::OpenModal(Modal::ConfirmForget)),
        KeyCode::Char('e') => Some(PanelCommand::StageReview(ReviewAction::Edit)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = &app.snapshot().review_queue;
    let mut lines = vec![format!("Review Queue  {} items  filter:[all]", rows.len())];
    for row in rows {
        lines.push(format!("[{}]  {}  \"{}\"", row.status, row.id, row.title));
        lines.push(format!("Namespace: {}  Confidence: {}  Added: {}", row.namespace, row.confidence, row.added));
        lines.push(format!("Policy: {}  Next: {}", row.policy, row.next));
        if let Some(reason) = &row.reason {
            lines.push(format!("Reason: {reason}"));
        }
        lines.push(String::new());
    }
    lines.push("a: approve   r: reject   f: forget   q: quarantine   e: edit".to_owned());
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Review Queue").borders(Borders::ALL)),
        area,
    );
}

fn next_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (cursor + 1).min(len - 1)
    }
}

fn previous_cursor(cursor: usize) -> usize {
    cursor.saturating_sub(1)
}
