use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use memoryd::protocol::{
    RealityCheckAction, RealityCheckCompletion, RealityCheckResponse, RequestEnvelope, RequestPayload,
    ResponseEnvelope, ResponsePayload,
};
use memoryd_tui::app::{App, DaemonCall, DaemonSnapshot, RealityCheckAction as AppRealityCheckAction};
use memoryd_tui::client::DaemonClient;
use memoryd_tui::inbox::InboxFilter;
use memoryd_tui::state::FocusKind;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn focused_app() -> App {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.enter_reality_check_focus("session-1", 0, 7);
    app
}

/// Reality Check focus pinned to the sample's due item, so the queued response
/// carries a deterministic memory id.
const DUE_MEMORY_ID: &str = "mem_20260501_0123456789abcdef_000004";

fn focused_on_due_item() -> App {
    let mut app = App::with_snapshot(DaemonSnapshot::sample());
    app.set_filter(InboxFilter::Due);
    app.set_selected(0);
    app.enter_reality_check_focus("session-1", 0, 7);
    app
}

#[test]
fn pressing_n_queues_not_relevant_for_focused_item() {
    let mut app = focused_on_due_item();
    app.handle_event(key(KeyCode::Char('n')), std::time::Instant::now());

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::RealityCheck {
            action: AppRealityCheckAction::NotRelevant,
            session_id: "session-1".into(),
            memory_id: DUE_MEMORY_ID.into(),
        }],
        "pressing n should queue a NotRelevant response for the focused memory",
    );
    // The focus stays in Reality Check so the operator keeps reviewing.
    assert!(matches!(app.focus(), FocusKind::RealityCheck { .. }));
}

#[test]
fn pressing_y_queues_confirm_for_focused_item() {
    let mut app = focused_on_due_item();
    app.handle_event(key(KeyCode::Char('y')), std::time::Instant::now());

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::RealityCheck {
            action: AppRealityCheckAction::Confirm,
            session_id: "session-1".into(),
            memory_id: DUE_MEMORY_ID.into(),
        }],
    );
}

#[test]
fn pressing_f_queues_forget_for_focused_item() {
    let mut app = focused_on_due_item();
    app.handle_event(key(KeyCode::Char('f')), std::time::Instant::now());

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::RealityCheck {
            action: AppRealityCheckAction::Forget,
            session_id: "session-1".into(),
            memory_id: DUE_MEMORY_ID.into(),
        }],
    );
}

#[test]
fn pressing_s_queues_skip_for_focused_item() {
    let mut app = focused_on_due_item();
    app.handle_event(key(KeyCode::Char('s')), std::time::Instant::now());

    assert_eq!(
        app.queued_daemon_calls(),
        &[DaemonCall::RealityCheck {
            action: AppRealityCheckAction::SkipWeek,
            session_id: "session-1".into(),
            memory_id: DUE_MEMORY_ID.into(),
        }],
    );
}

#[test]
fn pressing_k_during_reality_check_opens_correct_editor() {
    let mut app = focused_app();
    app.handle_event(key(KeyCode::Char('k')), std::time::Instant::now());

    assert!(matches!(app.focus(), FocusKind::CorrectEditor { .. }));
    assert_eq!(app.correct_editor_state().body(), "");
}

#[test]
fn escape_returns_from_correct_editor_without_dispatch() {
    let mut app = focused_app();
    let now = std::time::Instant::now();
    app.handle_event(key(KeyCode::Char('k')), now);
    app.handle_event(key(KeyCode::Char('x')), now);
    app.handle_event(key(KeyCode::Esc), now);

    assert!(matches!(app.focus(), FocusKind::RealityCheck { .. }));
    assert!(app.queued_daemon_calls().is_empty());
}

#[test]
fn empty_submit_shows_required_hint_without_dispatch() {
    let mut app = focused_app();
    let now = std::time::Instant::now();
    app.handle_event(key(KeyCode::Char('k')), now);
    app.handle_event(key_mod(KeyCode::Char('s'), KeyModifiers::CONTROL), now);

    assert_eq!(app.correct_editor_state().hint(), Some("body required"));
    assert!(app.queued_daemon_calls().is_empty());
}

#[tokio::test]
async fn ctrl_s_dispatches_correct_envelope_and_ack_advances_focus() {
    let socket_path = std::env::temp_dir().join(format!("memoryd-tui-correct-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("bind socket");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.expect("read");
        let request = RequestEnvelope::from_json_line(&line).expect("decode").request;
        let memory_id = match &request {
            RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::Respond { memory_id, .. }) => {
                memory_id.clone()
            }
            other => panic!("unexpected request: {other:?}"),
        };
        let response = ResponseEnvelope::success(
            "correct",
            ResponsePayload::RealityCheck(RealityCheckResponse::RespondAccepted {
                session_id: "session-1".into(),
                memory_id,
                next_item: None,
                completion: RealityCheckCompletion::Progress { remaining: 0, deferred: 0 },
            }),
        );
        stream.get_mut().write_all(response.to_json_line().expect("json").as_bytes()).await.expect("write");
        request
    });

    let mut app = focused_app();
    let now = std::time::Instant::now();
    app.handle_event(key(KeyCode::Char('k')), now);
    for ch in "Corrected body".chars() {
        app.handle_event(key(KeyCode::Char(ch)), now);
    }
    app.handle_event(key_mod(KeyCode::Char('s'), KeyModifiers::CONTROL), now);
    let client = DaemonClient::new(&socket_path);
    app.dispatch_queued_daemon_calls(&client).await;

    let request = server.await.expect("server");
    let RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::Respond { session_id, action, .. }) =
        request
    else {
        panic!("expected reality-check respond request");
    };
    assert_eq!(session_id, "session-1");
    assert_eq!(action, RealityCheckAction::Correct { new_body: "Corrected body".into() });
    assert!(matches!(app.focus(), FocusKind::RealityCheck { .. }));
    assert_eq!(app.reality_check_state().items_reviewed, 1);
    let _ = std::fs::remove_file(socket_path);
}
