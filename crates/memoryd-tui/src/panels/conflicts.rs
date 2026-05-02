use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand, ReviewAction};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictsState {
    cursor: usize,
    field_level_expanded: bool,
}

impl ConflictsState {
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
}

pub fn handle_key(key: &KeyEvent, state: &mut ConflictsState, row_count: usize) -> Option<PanelCommand> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = next_cursor(state.cursor, row_count);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cursor = state.cursor.saturating_sub(1);
            None
        }
        KeyCode::Enter => {
            state.field_level_expanded = true;
            Some(PanelCommand::OpenModal(Modal::MemoryDetail))
        }
        KeyCode::Char('l') | KeyCode::Right => Some(PanelCommand::StageReview(ReviewAction::AcceptLocal)),
        KeyCode::Char('r') => Some(PanelCommand::StageReview(ReviewAction::AcceptRemote)),
        KeyCode::Char('m') => Some(PanelCommand::StageReview(ReviewAction::Merge)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut lines = vec![format!("Conflicts  {} item", app.snapshot().conflicts.len())];
    for row in &app.snapshot().conflicts {
        lines.push(format!("[conflict] {}  \"{}\"", row.id, row.title));
        lines.push(format!("Namespace: {}", row.namespace));
        lines.push(String::new());
        lines.push("LOCAL                         REMOTE".to_owned());
        lines.push(format!("{:<30} {}", row.local, row.remote));
        lines.push("COMMON ANCESTOR".to_owned());
        lines.push(row.ancestor.clone());
    }
    lines.push("l: accept local   r: accept remote   m: merge   q: quarantine".to_owned());
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Conflicts").borders(Borders::ALL)),
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
