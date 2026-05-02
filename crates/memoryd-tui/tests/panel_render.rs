use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd_tui::app::{App, DaemonSnapshot, PanelId};
use ratatui::{backend::TestBackend, Terminal};

fn render_panel(panel: PanelId) -> String {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_active_panel(panel);

    terminal.draw(|frame| app.render(frame)).expect("frame should render");
    terminal.backend().to_string()
}

fn render_app(app: App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");

    terminal.draw(|frame| app.render(frame)).expect("frame should render");
    terminal.backend().to_string()
}

#[test]
fn test_overview_panel_renders_daemon_status() {
    let frame = render_panel(PanelId::Overview);

    assert!(frame.contains("Memorum"));
    assert!(frame.contains("Daemon"));
    assert!(frame.contains("running"));
    assert!(frame.contains("Pending review"));
    assert!(frame.contains("Recall (session totals)"));
    assert!(frame.contains("socket:ok"));
}

#[test]
fn test_review_queue_renders_candidate_items() {
    let frame = render_panel(PanelId::ReviewQueue);

    assert!(frame.contains("Review Queue"));
    assert!(frame.contains("[candidate]"));
    assert!(frame.contains("Prefer CITEXT"));
    assert!(frame.contains("requires_user_confirmation"));
}

#[test]
fn test_review_queue_renders_dream_low_confidence() {
    let frame = render_panel(PanelId::ReviewQueue);

    assert!(frame.contains("[dream_low_confidence]"));
    assert!(frame.contains("Dream candidate"));
    assert!(frame.contains("dream_low_confidence"));
}

#[test]
fn test_conflicts_panel_renders_side_by_side() {
    let frame = render_panel(PanelId::Conflicts);

    assert!(frame.contains("Conflicts"));
    assert!(frame.contains("LOCAL"));
    assert!(frame.contains("REMOTE"));
    assert!(frame.contains("COMMON ANCESTOR"));
    assert!(frame.contains("Pool size: 20"));
    assert!(frame.contains("Pool size: 30"));
}

#[test]
fn test_entities_panel_search_renders_results() {
    let frame = render_panel(PanelId::Entities);

    assert!(frame.contains("Entities"));
    assert!(frame.contains("/entity-search"));
    assert!(frame.contains("Entity: atlasos"));
    assert!(frame.contains("Top memories"));
}

#[test]
fn test_timeline_panel_renders_events_by_kind() {
    let frame = render_panel(PanelId::Timeline);

    assert!(frame.contains("Timeline"));
    assert!(frame.contains("write"));
    assert!(frame.contains("dream_pass"));
    assert!(frame.contains("recall"));
    assert!(frame.contains("privacy"));
}

#[test]
fn test_namespace_tree_renders_hierarchy() {
    let frame = render_panel(PanelId::Namespace);

    assert!(frame.contains("Namespace Explorer"));
    assert!(frame.contains("me/"));
    assert!(frame.contains("projects/"));
    assert!(frame.contains("atlasos/"));
    assert!(frame.contains("agent/"));
}

#[test]
fn test_policy_panel_renders_active_policies() {
    let frame = render_panel(PanelId::Policy);

    assert!(frame.contains("Policy Inspector"));
    assert!(frame.contains("me-strict@v1"));
    assert!(frame.contains("project-standard@v2"));
    assert!(frame.contains("agent-strict@v3"));
    assert!(frame.contains("dreaming-strict@v1"));
}

#[test]
fn test_reality_check_panel_renders_score_breakdown() {
    let frame = render_panel(PanelId::RealityCheck);

    assert!(frame.contains("Reality Check"));
    assert!(frame.contains("Top drift-risk memories"));
    assert!(frame.contains("score:0.82"));
    assert!(frame.contains("Score breakdown"));
    assert!(frame.contains("staleness:0.35"));
}

#[test]
fn test_memory_detail_modal_renders_trust_artifact_fields() {
    let snapshot = DaemonSnapshot::sample();
    let artifact = snapshot.trust_artifact.clone().expect("sample includes a daemon trust artifact");
    let mut app = App::with_snapshot(snapshot);
    app.set_active_panel(PanelId::Entities);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)), Instant::now());
    app.set_trust_artifact(artifact);

    let frame = render_app(app, 120, 60);

    assert!(frame.contains("Memory Detail"));
    assert!(frame.contains("mem_20260501_0123456789abcdef_000009"));
    assert!(frame.contains("source: substrate:projects/atlasos/deploy-target.md"));
    assert!(frame.contains("trust: high trust; policy-promoted"));
    assert!(frame.contains("Confidence"));
    assert!(frame.contains("Recall"));
    assert!(frame.contains("evidence:"));
    assert!(frame.contains("Supersession"));
}

#[test]
fn test_memory_detail_without_resolved_id_does_not_render_stale_artifact() {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_active_panel(PanelId::Namespace);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)), Instant::now());

    let frame = render_app(app, 120, 60);

    assert!(frame.contains("Memory Detail"));
    assert!(frame.contains("No trust artifact loaded."));
    assert!(!frame.contains("mem_20260501_0123456789abcdef_000009"));
    assert!(!frame.contains("source: substrate:projects/atlasos/deploy-target.md"));
}
