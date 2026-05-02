use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, PanelCommand, RealityCheckAction};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RealityCheckState {
    active_run: bool,
    cursor: usize,
    filter_open: bool,
    last_action: Option<RealityCheckAction>,
}

impl RealityCheckState {
    pub const fn is_active_run(&self) -> bool {
        self.active_run
    }

    pub const fn last_action(&self) -> Option<RealityCheckAction> {
        self.last_action
    }

    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn start_run(&mut self) {
        self.active_run = true;
    }

    pub fn record_action(&mut self, action: RealityCheckAction) {
        self.last_action = Some(action);
    }
}

pub fn handle_key(key: &KeyEvent, state: &mut RealityCheckState) -> Option<PanelCommand> {
    if state.active_run {
        return handle_active_run_key(key, state);
    }

    match key.code {
        KeyCode::Char('r') => Some(PanelCommand::StartRealityCheck),
        KeyCode::Char('s') => {
            state.last_action = Some(RealityCheckAction::SkipWeek);
            None
        }
        KeyCode::Char('h') => None,
        KeyCode::Char('/') => {
            state.filter_open = true;
            None
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = state.cursor.saturating_add(1);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cursor = state.cursor.saturating_sub(1);
            None
        }
        _ => None,
    }
}

fn handle_active_run_key(key: &KeyEvent, state: &mut RealityCheckState) -> Option<PanelCommand> {
    match key.code {
        KeyCode::Char('c') => Some(PanelCommand::RealityCheck(RealityCheckAction::Confirm)),
        KeyCode::Char('k') => Some(PanelCommand::RealityCheck(RealityCheckAction::Correct)),
        KeyCode::Char('f') => Some(PanelCommand::RealityCheck(RealityCheckAction::Forget)),
        KeyCode::Char('n') => Some(PanelCommand::RealityCheck(RealityCheckAction::NotRelevant)),
        KeyCode::Char(' ') => Some(PanelCommand::RealityCheck(RealityCheckAction::SkipWeek)),
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = state.cursor.saturating_add(1);
            None
        }
        KeyCode::Up => {
            state.cursor = state.cursor.saturating_sub(1);
            None
        }
        _ => None,
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let data = &app.snapshot().reality_check;
    let mut lines = if app.reality_check_state().is_active_run() {
        vec![
            "Reality Check -- ACTIVE   0 of 12 items reviewed".to_owned(),
            String::new(),
            "c: confirm   k: correct   f: forget   n: not relevant   space: skip this week".to_owned(),
            String::new(),
        ]
    } else {
        vec![
            "Reality Check".to_owned(),
            String::new(),
            format!("Status: {}  (last completed: {})", data.status, data.last_completed),
            format!("Schedule: {}", data.schedule),
            String::new(),
            "Top drift-risk memories (12 of 1,204):".to_owned(),
        ]
    };
    for item in &data.items {
        lines.push(format!("#{}  [score:{:.2}]  \"{}\"", item.rank, item.score, item.title));
        lines.push(format!("{} | conf:{:.2} | last observed: {}", item.namespace, item.confidence, item.last_observed));
        lines.push(format!("Recall (30d): {}", item.recall_count_30d));
        lines.push("Score breakdown:".to_owned());
        lines.push(item.breakdown.clone());
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::default().title("Reality Check").borders(Borders::ALL)),
        area,
    );
}
