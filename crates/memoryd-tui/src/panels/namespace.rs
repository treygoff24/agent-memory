use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamespaceState {
    cursor: usize,
    search_open: bool,
    detail_focused: bool,
    selected_expanded: bool,
}

pub fn handle_key(key: &KeyEvent, state: &mut NamespaceState, row_count: usize) -> Option<PanelCommand> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = next_cursor(state.cursor, row_count);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cursor = state.cursor.saturating_sub(1);
            None
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.selected_expanded = false;
            None
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.selected_expanded = true;
            None
        }
        KeyCode::Char('/') => {
            state.search_open = true;
            None
        }
        KeyCode::Tab => {
            state.detail_focused = !state.detail_focused;
            None
        }
        KeyCode::Enter | KeyCode::Char('t') => Some(PanelCommand::OpenModal(Modal::MemoryDetail)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    let data = &app.snapshot().namespace;
    frame.render_widget(
        Paragraph::new(data.tree_lines.join("\n"))
            .block(Block::default().title("Namespace Explorer").borders(Borders::ALL)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(data.detail_lines.join("\n"))
            .block(Block::default().title("Selected memory").borders(Borders::ALL)),
        chunks[1],
    );
}

fn next_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (cursor + 1).min(len - 1)
    }
}
