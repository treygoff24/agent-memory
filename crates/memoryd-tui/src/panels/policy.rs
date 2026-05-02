use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyState {
    cursor: usize,
    filter_open: bool,
    reload_requested: bool,
}

pub fn handle_key(key: &KeyEvent, state: &mut PolicyState, row_count: usize) -> Option<PanelCommand> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = next_cursor(state.cursor, row_count);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cursor = state.cursor.saturating_sub(1);
            None
        }
        KeyCode::Char('/') => {
            state.filter_open = true;
            None
        }
        KeyCode::Char('r') => {
            state.reload_requested = true;
            None
        }
        KeyCode::Enter | KeyCode::Char('e') => Some(PanelCommand::OpenModal(Modal::MemoryDetail)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let data = &app.snapshot().policy;
    let mut lines = vec!["Policy Inspector".to_owned(), String::new(), "Active policies:".to_owned()];
    lines.extend(data.active_policies.iter().cloned());
    lines.push(String::new());
    lines.push("Recent decisions:".to_owned());
    lines.extend(data.recent_decisions.iter().cloned());
    lines.push(String::new());
    lines.push("Refusal reasons:".to_owned());
    lines.extend(data.refusal_reasons.iter().cloned());
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Policy Inspector").borders(Borders::ALL)),
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
