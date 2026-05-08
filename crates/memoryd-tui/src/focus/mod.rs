use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::App;
use crate::state::FocusKind;
use crate::theme_glue::ThemeStyles;

pub mod correct_editor;
pub mod reality_check;

pub struct FocusRenderContext<'a> {
    pub kind: &'a FocusKind,
    pub app: &'a App,
    pub styles: &'a ThemeStyles,
}

pub fn render(frame: &mut Frame<'_>, area: Rect, context: FocusRenderContext<'_>) {
    match context.kind {
        FocusKind::None => {}
        FocusKind::RealityCheck { .. } => reality_check::render(frame, area, context.app, context.styles),
        FocusKind::CorrectEditor { .. } => {
            correct_editor::render(frame, area, context.app.correct_editor_state(), context.styles);
        }
    }
}
