use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let data = &app.snapshot().overview;
    let text = format!(
        "Daemon       {}  (PID {})        Uptime: {}\n\
Socket       {} ok\n\
Index        ~{} active memories      last reindex: {}\n\
Sync         ahead {} / behind {}          remote: {}\n\n\
Pending review      {}   ({} candidate, {} quarantined, {} dream low-confidence)\n\
Conflicts           {}\n\
Active sessions     {}\n\
Dreaming            scheduled  next: {}\n\
                    last run: {}  promoted:{} queued:{} dropped:{}\n\n\
Recall (session totals)  startup:{}  delta:{}  peer-updates:{}",
        data.daemon_state,
        data.pid,
        data.uptime,
        data.socket_path,
        data.active_memories,
        data.last_reindex,
        data.sync_ahead,
        data.sync_behind,
        data.remote,
        data.pending_review,
        data.candidate,
        data.quarantined,
        data.dream_low_confidence,
        data.conflicts,
        data.active_sessions,
        data.dream_next,
        data.dream_last,
        data.dream_promoted,
        data.dream_queued,
        data.dream_dropped,
        data.recall_startup,
        data.recall_delta,
        data.peer_updates,
    );
    frame.render_widget(Paragraph::new(text).block(Block::default().title("Overview").borders(Borders::ALL)), area);
}
