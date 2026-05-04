use ratatui::{layout::Rect, Frame};

use crate::app::{App, PanelId};

pub mod conflicts;
pub mod entities;
pub mod namespace;
pub mod overview;
pub mod policy;
pub mod reality_check;
pub mod recall;
pub mod review_queue;
pub mod timeline;

pub fn render_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    match app.active_panel() {
        PanelId::Overview => overview::render(frame, area, app),
        PanelId::ReviewQueue => review_queue::render(frame, area, app),
        PanelId::Conflicts => conflicts::render(frame, area, app),
        PanelId::Entities => entities::render(frame, area, app),
        PanelId::Timeline => timeline::render(frame, area, app),
        PanelId::Namespace => namespace::render(frame, area, app),
        PanelId::Policy => policy::render(frame, area, app),
        PanelId::RealityCheck => reality_check::render(frame, area, app),
        PanelId::Recall => recall::render(frame, area, app),
    }
}
