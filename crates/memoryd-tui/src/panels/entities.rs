use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntitiesState {
    cursor: usize,
    search_open: bool,
    memory_list_focused: bool,
    chain_visible: bool,
}

pub fn handle_key(key: &KeyEvent, state: &mut EntitiesState, row_count: usize) -> Option<PanelCommand> {
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
            state.search_open = true;
            None
        }
        KeyCode::Tab => {
            state.memory_list_focused = !state.memory_list_focused;
            None
        }
        KeyCode::Char('s') => {
            state.chain_visible = !state.chain_visible;
            None
        }
        KeyCode::Enter | KeyCode::Char('t') => Some(PanelCommand::OpenModal(Modal::MemoryDetail)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let data = &app.snapshot().entities;
    let mut lines = vec![
        format!("Entities   /entity-search: [{}]", data.query),
        String::new(),
        format!("Entity: {} ({})", data.entity, data.project_id),
        format!("Memories: {} active", data.memory_count),
        format!("Recall count (30d): {}", data.recall_count_30d),
        String::new(),
        "Top memories:".to_owned(),
    ];
    lines.extend(data.top_memories.iter().cloned());
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Entities").borders(Borders::ALL)),
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
