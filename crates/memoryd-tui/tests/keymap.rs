use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd::protocol::{
    MemoryId, RealityCheckAction as ProtocolRealityCheckAction, RealityCheckRequest, RequestEnvelope, RequestPayload,
    ResponseEnvelope, ResponsePayload, ReviewDecisionResponse,
};
use memoryd_tui::app::{
    App, DaemonCall, DaemonSnapshot, Modal, PanelId, RealityCheckAction, RealityCheckRow, ReviewAction,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn app() -> App {
    App::with_snapshot(DaemonSnapshot::sample())
}

fn press_char(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
}

fn press_code(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn press_ctrl(ch: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
}

fn record_daemon_request(response: ResponseEnvelope) -> (std::path::PathBuf, tokio::task::JoinHandle<RequestPayload>) {
    static SOCKET_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let socket_path = std::env::temp_dir().join(format!(
        "memoryd-tui-test-{}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos(),
        SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("test daemon socket should bind");
    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("test daemon should accept one request");
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.expect("test daemon should read request");
        stream
            .get_mut()
            .write_all(response.to_json_line().expect("response should serialize").as_bytes())
            .await
            .expect("test daemon should write response");
        RequestEnvelope::from_json_line(&line).expect("request should decode").request
    });

    (socket_path, handle)
}

fn review_approve_response() -> ResponseEnvelope {
    ResponseEnvelope::success(
        "review-ok",
        ResponsePayload::ReviewApprove(ReviewDecisionResponse {
            id: "mem_20260501_0123456789abcdef_000001".to_owned(),
            status: "active".to_owned(),
            summary: "approved".to_owned(),
        }),
    )
}

#[test]
fn test_all_panels_handle_panel_switch_keys() {
    let start = Instant::now();

    for starting_panel in PanelId::all() {
        for (offset, expected_panel) in PanelId::all().into_iter().enumerate() {
            let mut app = app();
            app.set_active_panel(starting_panel);

            app.handle_event(press_char(char::from(b'1' + offset as u8)), start);

            assert_eq!(app.active_panel(), expected_panel);
            assert!(app.modal().is_none());
        }
    }
}

#[test]
fn test_quit_with_pending_actions_prompts_confirmation() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);

    app.handle_event(press_char('a'), start);
    app.handle_event(press_char('q'), start + Duration::from_millis(250));

    assert_eq!(app.modal(), Some(&Modal::ConfirmQuit));
    assert!(!app.should_quit());
    assert!(app.pending_action().is_some());
}

#[test]
fn test_escape_closes_modal() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);

    app.handle_event(press_code(KeyCode::Enter), start);
    let panel_before_escape = app.active_panel();
    assert_eq!(app.modal(), Some(&Modal::MemoryDetail));

    app.handle_event(press_code(KeyCode::Esc), start + Duration::from_millis(10));

    assert!(app.modal().is_none());
    assert_eq!(app.active_panel(), panel_before_escape);
}

#[test]
fn test_undo_window_fires_before_daemon_call() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);

    app.handle_event(press_char('a'), start);
    app.handle_event(press_char('u'), start + Duration::from_millis(999));

    assert!(app.pending_action().is_none());
    assert!(app.queued_daemon_calls().is_empty());
}

#[test]
fn test_undo_window_expires_and_fires_daemon_call() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);

    app.handle_event(press_char('a'), start);
    app.on_tick(start + Duration::from_millis(1_001));

    assert!(app.pending_action().is_none());
    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::Review {
            action: ReviewAction::Approve,
            memory_id: "mem_20260501_0123456789abcdef_000001".to_owned()
        }]
    );
}

#[tokio::test]
async fn test_expired_review_action_reaches_daemon_and_clears_queue() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);
    let (socket_path, recorded_request) = record_daemon_request(review_approve_response());
    let client = memoryd_tui::client::DaemonClient::new(socket_path.clone());

    app.handle_event(press_char('a'), start);
    app.on_tick(start + Duration::from_millis(1_001));
    app.dispatch_queued_daemon_calls(&client).await;

    assert_eq!(
        recorded_request.await.expect("test daemon task should finish"),
        RequestPayload::ReviewApprove { id: "mem_20260501_0123456789abcdef_000001".to_owned() }
    );
    assert!(app.queued_daemon_calls().is_empty());
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn test_daemon_dispatch_failure_is_visible_and_retryable() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);
    let response = ResponseEnvelope::error("review-failed", "review_failed", "review write failed", true);
    let (socket_path, recorded_request) = record_daemon_request(response);
    let client = memoryd_tui::client::DaemonClient::new(socket_path.clone());

    app.handle_event(press_char('a'), start);
    app.on_tick(start + Duration::from_millis(1_001));
    app.dispatch_queued_daemon_calls(&client).await;

    assert_eq!(
        recorded_request.await.expect("test daemon task should finish"),
        RequestPayload::ReviewApprove { id: "mem_20260501_0123456789abcdef_000001".to_owned() }
    );
    assert_eq!(app.queued_daemon_calls().len(), 1, "failed calls stay queued for retry");
    assert!(format!("{:?}", app.socket_state()).contains("review write failed"));
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn test_reality_check_action_dispatches_selected_memory_id() {
    let start = Instant::now();
    let mut snapshot = DaemonSnapshot::sample();
    snapshot.reality_check.session_id = Some("rc-session-1".to_owned());
    snapshot.reality_check.items = vec![
        RealityCheckRow {
            memory_id: "mem_20260501_0123456789abcdef_000001".to_owned(),
            rank: 1,
            score: 0.82,
            title: "First row title is not an id".to_owned(),
            namespace: "me".to_owned(),
            confidence: 0.88,
            last_observed: "62 days ago".to_owned(),
            recall_count_30d: 0,
            breakdown: "staleness:0.35".to_owned(),
        },
        RealityCheckRow {
            memory_id: "mem_20260501_0123456789abcdef_000002".to_owned(),
            rank: 2,
            score: 0.77,
            title: "Selected row title is not an id either".to_owned(),
            namespace: "project".to_owned(),
            confidence: 0.81,
            last_observed: "30 days ago".to_owned(),
            recall_count_30d: 3,
            breakdown: "recall:0.16".to_owned(),
        },
    ];
    let mut app = App::with_snapshot(snapshot);
    app.set_active_panel(PanelId::RealityCheck);
    let response = ResponseEnvelope::success(
        "rc-ok",
        ResponsePayload::RealityCheck(memoryd::protocol::RealityCheckResponse::RespondAccepted {
            session_id: "rc-session-1".to_owned(),
            memory_id: MemoryId::new("mem_20260501_0123456789abcdef_000002"),
            next_item: None,
            completion: memoryd::protocol::RealityCheckCompletion::Progress { remaining: 0, deferred: 0 },
        }),
    );
    let (socket_path, recorded_request) = record_daemon_request(response);
    let client = memoryd_tui::client::DaemonClient::new(socket_path.clone());

    app.handle_event(press_char('r'), start);
    app.handle_event(press_code(KeyCode::Down), start + Duration::from_millis(1));
    app.handle_event(press_char('c'), start + Duration::from_millis(2));
    app.dispatch_queued_daemon_calls(&client).await;

    assert_eq!(
        recorded_request.await.expect("test daemon task should finish"),
        RequestPayload::RealityCheck(RealityCheckRequest::Respond {
            session_id: "rc-session-1".to_owned(),
            memory_id: MemoryId::new("mem_20260501_0123456789abcdef_000002"),
            action: ProtocolRealityCheckAction::Confirm,
        })
    );
    assert!(app.queued_daemon_calls().is_empty());
    let _ = std::fs::remove_file(socket_path);
}

#[test]
fn test_memory_detail_requests_selected_daemon_trust_artifact() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);

    app.handle_event(press_code(KeyCode::Enter), start);

    assert_eq!(app.modal(), Some(&Modal::MemoryDetail));
    assert_eq!(app.pending_trust_artifact_id(), Some("mem_20260501_0123456789abcdef_000001"));
    assert!(app.snapshot().trust_artifact.is_none());
}

#[test]
fn test_memory_detail_without_resolved_id_clears_stale_trust_artifact() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::Namespace);
    assert!(app.snapshot().trust_artifact.is_some(), "sample fixture starts with a loaded artifact");

    app.handle_event(press_char('t'), start);

    assert_eq!(app.modal(), Some(&Modal::MemoryDetail));
    assert_eq!(app.pending_trust_artifact_id(), None);
    assert!(app.snapshot().trust_artifact.is_none());
}

#[test]
fn test_resize_closes_active_modal() {
    let start = Instant::now();
    let mut app = app();

    app.handle_event(press_char('?'), start);
    assert_eq!(app.modal(), Some(&Modal::HelpOverlay));

    app.handle_event(Event::Resize(100, 32), start + Duration::from_millis(10));

    assert!(app.modal().is_none());
}

#[test]
fn test_help_overlay_opens_on_question_mark() {
    let start = Instant::now();
    let mut app = app();

    app.handle_event(press_char('?'), start);

    assert_eq!(app.modal(), Some(&Modal::HelpOverlay));
}

#[test]
fn test_ctrl_c_quits_immediately() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::ReviewQueue);

    app.handle_event(press_char('a'), start);
    app.handle_event(press_ctrl('c'), start + Duration::from_millis(10));

    assert!(app.should_quit());
    assert_ne!(app.modal(), Some(&Modal::ConfirmQuit));
}

#[test]
fn test_global_keys_work_from_all_panels_before_local_keys() {
    let start = Instant::now();

    for panel in PanelId::all() {
        let mut app = app();
        app.set_active_panel(panel);

        app.handle_event(press_char('?'), start);
        assert_eq!(app.modal(), Some(&Modal::HelpOverlay), "help failed from {panel:?}");

        app.handle_event(press_code(KeyCode::Esc), start + Duration::from_millis(1));
        app.handle_event(press_char('q'), start + Duration::from_millis(2));
        assert!(app.should_quit(), "quit failed from {panel:?}");
    }
}

#[test]
fn test_reality_check_active_run_handles_action_keys() {
    let start = Instant::now();
    let mut app = app();
    app.set_active_panel(PanelId::RealityCheck);

    app.handle_event(press_char('r'), start);
    assert!(app.reality_check_state().is_active_run());

    for (index, (key, action)) in [
        (KeyCode::Char('c'), RealityCheckAction::Confirm),
        (KeyCode::Char('k'), RealityCheckAction::Correct),
        (KeyCode::Char('f'), RealityCheckAction::Forget),
        (KeyCode::Char('n'), RealityCheckAction::NotRelevant),
        (KeyCode::Char(' '), RealityCheckAction::SkipWeek),
    ]
    .into_iter()
    .enumerate()
    {
        app.handle_event(press_code(key), start + Duration::from_millis(10 + index as u64));
        assert_eq!(app.reality_check_state().last_action(), Some(action));
    }
}
