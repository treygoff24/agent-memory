use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, SocketState};
use crate::theme_glue::ThemeStyles;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let socket = match app.socket_state() {
        SocketState::Connected => Span::styled("socket:ok", styles.ok),
        SocketState::Unreachable { .. } => Span::styled("socket:UNREACHABLE", styles.bad),
    };
    let line = Line::from(vec![
        Span::styled(format!("memoryd {}  ", app.snapshot().version), styles.muted),
        Span::styled(format!("Daemon:{}  ", app.snapshot().daemon_state), styles.muted),
        socket,
        Span::raw(format!("  filter:{}  {}", app.filter().label(), app.snapshot().footer_hint)),
    ]);
    frame.render_widget(Paragraph::new(line).style(styles.base), area);
}
