pub mod commands;
pub mod fuzzy;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::palette::commands::{catalog, Command};
use crate::theme_glue::ThemeStyles;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaletteState {
    input: String,
    selected: usize,
    message: Option<String>,
}

impl PaletteState {
    pub fn open(&mut self) {
        self.input.clear();
        self.selected = 0;
        self.message = None;
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn selected_label(&self) -> Option<&'static str> {
        self.candidates().get(self.selected).map(|command| command.label)
    }

    pub fn selected_command(&self) -> Option<Command> {
        self.candidates().get(self.selected).cloned()
    }

    pub fn candidates(&self) -> Vec<Command> {
        let mut commands = catalog();
        if !self.input.trim().is_empty() {
            commands = commands
                .into_iter()
                .filter_map(|command| fuzzy::score(&self.input, command.label).map(|score| (command, score)))
                .collect::<Vec<_>>()
                .into_iter()
                .sorted_by_score();
        }
        commands
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> PaletteKeyResult {
        match key.code {
            KeyCode::Esc => PaletteKeyResult::Close,
            KeyCode::Enter => PaletteKeyResult::Submit,
            KeyCode::Backspace => {
                self.input.pop();
                self.selected = 0;
                self.message = None;
                PaletteKeyResult::Handled
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                PaletteKeyResult::Handled
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.candidates().len().saturating_sub(1));
                PaletteKeyResult::Handled
            }
            KeyCode::Char(ch) => {
                self.input.push(ch);
                self.selected = 0;
                self.message = None;
                PaletteKeyResult::Handled
            }
            _ => PaletteKeyResult::Handled,
        }
    }

    pub fn show_no_match(&mut self) {
        self.message = Some("No matching command".to_string());
    }

    pub fn show_error(&mut self, message: String) {
        self.message = Some(message);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKeyResult {
    Handled,
    Submit,
    Close,
}

trait SortByScore {
    fn sorted_by_score(self) -> Vec<Command>;
}

impl SortByScore for std::vec::IntoIter<(Command, u32)> {
    fn sorted_by_score(self) -> Vec<Command> {
        let mut matches = self.collect::<Vec<_>>();
        matches.sort_by(|(left, left_score), (right, right_score)| {
            right_score.cmp(left_score).then_with(|| left.label.cmp(right.label))
        });
        matches.into_iter().map(|(command, _)| command).collect()
    }
}

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &PaletteState, styles: &ThemeStyles) {
    let mut lines = vec![Line::from(vec![Span::styled(":", styles.accent), Span::raw(state.input())])];
    if let Some(message) = state.message() {
        lines.push(Line::from(Span::styled(message.to_string(), styles.warn)));
    }
    for (index, command) in state.candidates().into_iter().take(8).enumerate() {
        let style = if index == state.selected { styles.selected } else { styles.base };
        lines.push(Line::from(Span::styled(command.label.to_string(), style)));
    }
    frame.render_widget(
        Paragraph::new(lines).style(styles.base).block(
            Block::new()
                .title("Command Palette")
                .borders(Borders::ALL)
                .border_set(styles.border)
                .border_style(styles.block),
        ),
        area,
    );
}
