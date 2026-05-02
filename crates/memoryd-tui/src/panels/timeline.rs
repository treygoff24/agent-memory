use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineState {
    cursor: usize,
    filter_open: bool,
}

pub fn handle_key(key: &KeyEvent, state: &mut TimelineState, row_count: usize) -> Option<PanelCommand> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = next_cursor(state.cursor, row_count);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cursor = state.cursor.saturating_sub(1);
            None
        }
        KeyCode::Char('G') => {
            state.cursor = row_count.saturating_sub(1);
            None
        }
        KeyCode::Char('g') => {
            state.cursor = 0;
            None
        }
        KeyCode::Char('/') => {
            state.filter_open = true;
            None
        }
        KeyCode::Enter => Some(PanelCommand::OpenModal(Modal::MemoryDetail)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut lines = vec!["Timeline   last 500 events   filter:[all]".to_owned()];
    for row in &app.snapshot().timeline {
        lines.push(format!("{}  {:<10}  {}", row.timestamp, row.kind, row.detail));
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Timeline").borders(Borders::ALL)),
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
