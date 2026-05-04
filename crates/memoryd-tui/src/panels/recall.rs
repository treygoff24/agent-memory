use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Modal, PanelCommand};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecallState {
    cursor: usize,
}

impl RecallState {
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
}

pub fn handle_key(key: &KeyEvent, state: &mut RecallState, row_count: usize) -> Option<PanelCommand> {
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
        KeyCode::Enter | KeyCode::Char('t') => Some(PanelCommand::OpenModal(Modal::MemoryDetail)),
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let data = &app.snapshot().recall;
    let mut lines = vec![
        format!("Recall hits   showing {} of requested {}", data.hits.len(), data.limit),
        String::new(),
        "Hourly density".to_owned(),
    ];

    if data.hits.is_empty() {
        lines.push("No recall hits yet - try startup recall or a delta block.".to_owned());
    } else {
        for (bucket, count) in histogram_lines(&data.hits) {
            let bar = "█".repeat(count.min(24));
            lines.push(format!("{bucket}  {bar} {count}"));
        }
        lines.push(String::new());
        lines.push("Recent hits".to_owned());
        lines.push("score:n/a  harness:n/a  session:n/a until the daemon protocol carries those fields".to_owned());

        for (index, hit) in data.hits.iter().enumerate() {
            let marker =
                if index == app.recall_state().cursor().min(data.hits.len().saturating_sub(1)) { ">" } else { " " };
            lines.push(format!(
                "{marker} {}  {}  device:{} seq:{}  score:n/a harness:n/a session:n/a",
                hit.recalled_at, hit.memory_id, hit.device, hit.seq
            ));
            if let Some(summary) = hit.summary.as_ref().filter(|summary| !summary.is_empty()) {
                lines.push(format!("    {summary}"));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Recall").borders(Borders::ALL)),
        area,
    );
}

fn histogram_lines(hits: &[crate::app::RecallHitRow]) -> Vec<(String, usize)> {
    let mut counts = BTreeMap::<String, usize>::new();
    for hit in hits {
        let bucket = hit.recalled_at.chars().take(13).collect::<String>();
        *counts.entry(format!("{bucket}:00")).or_default() += 1;
    }
    counts.into_iter().rev().take(6).collect()
}

fn next_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (cursor + 1).min(len - 1)
    }
}
