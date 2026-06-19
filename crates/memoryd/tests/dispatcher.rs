use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use memory_substrate::MemoryId;
use memoryd::notifications::config::{
    EmailNotificationConfig, ExternalChannelConfig, ExternalNotificationConfig, NotificationConfig,
    NotificationTrigger, OsNotificationConfig,
};
use memoryd::notifications::dispatcher::NotificationDispatcher;
use memoryd::notifications::external::{EmailDelivery, EmailMessage, ExternalDeliveryError, ExternalNotifier, Sleeper};
use memoryd::notifications::os::{OsNotification, OsNotificationSink, OsNotifier};
use memoryd::notifications::passive::PassiveQueue;
use memoryd::protocol::NotificationEvent;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration as TokioDuration};

#[tokio::test]
async fn test_passive_queue_receives_all_events() {
    let passive = PassiveQueue::new();
    let dispatcher = test_dispatcher(passive.clone(), NotificationConfig::default());

    for event in all_notification_events() {
        dispatcher.dispatch_event(event).await;
    }

    assert_eq!(passive.messages().len(), 7);
}

#[tokio::test]
async fn test_passive_queue_drops_oldest_when_full() {
    let queue = PassiveQueue::new();

    for index in 0..100 {
        queue.append(format!("event {index}"));
    }
    queue.append("event 100");

    let messages = queue.messages();
    assert_eq!(messages.len(), 100);
    assert_eq!(messages.first().map(String::as_str), Some("event 1"));
    assert_eq!(messages.last().map(String::as_str), Some("event 100"));
}

#[tokio::test]
async fn test_os_notification_not_fired_when_disabled() {
    let sink = Arc::new(RecordingOsSink::default());
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive,
        NotificationConfig { os: OsNotificationConfig { enabled: false, ..Default::default() }, ..Default::default() },
        OsNotifier::with_sink(sink.clone()),
        ExternalNotifier::disabled(),
    );

    dispatcher.dispatch_event(secret_event()).await;

    assert_eq!(sink.calls(), 0);
}

#[tokio::test]
async fn test_os_notification_fires_when_enabled_and_trigger_matches() {
    let sink = Arc::new(RecordingOsSink::default());
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive,
        NotificationConfig {
            os: OsNotificationConfig { enabled: true, triggers: vec![NotificationTrigger::LeakedSecretDetected] },
            ..Default::default()
        },
        OsNotifier::with_sink(sink.clone()),
        ExternalNotifier::disabled(),
    );

    dispatcher.dispatch_event(secret_event()).await;

    assert_eq!(sink.calls(), 1);
}

#[tokio::test]
async fn test_slack_webhook_retried_on_failure() {
    let server = SlackMockServer::start(HttpStatus::InternalServerError).await;
    let sleeper = Arc::new(RecordingSleeper::default());
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive,
        external_slack_config(server.url(), 3),
        OsNotifier::disabled(),
        ExternalNotifier::slack_for_tests(
            Arc::new(memoryd::notifications::external::ReqwestSlackWebhook::new()),
            sleeper,
        ),
    );

    dispatcher.dispatch_event(reality_check_due()).await;

    assert_eq!(server.requests(), 3);
}

#[tokio::test]
async fn test_slack_webhook_falls_back_to_passive_on_final_failure() {
    let server = SlackMockServer::start(HttpStatus::InternalServerError).await;
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive.clone(),
        external_slack_config(server.url(), 3),
        OsNotifier::disabled(),
        ExternalNotifier::slack_for_tests(
            Arc::new(memoryd::notifications::external::ReqwestSlackWebhook::new()),
            Arc::new(RecordingSleeper::default()),
        ),
    );

    dispatcher.dispatch_event(reality_check_due()).await;

    assert!(passive.messages().iter().any(|message| message.starts_with("External notification failed:")));
}

#[tokio::test]
async fn test_external_failure_fallback_redacts_webhook_url() {
    let canary_url = "https://hooks.slack.com/services/T000/B000/SECRET_CANARY_TOKEN";
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive.clone(),
        external_slack_config(canary_url.to_owned(), 1),
        OsNotifier::disabled(),
        ExternalNotifier::slack_for_tests(
            Arc::new(FailingSlackWebhook { error: format!("POST {canary_url} failed") }),
            Arc::new(RecordingSleeper::default()),
        ),
    );

    dispatcher.dispatch_event(reality_check_due()).await;

    let messages = passive.messages().join("\n");
    assert!(messages.contains("External notification failed: external delivery failed"));
    assert!(!messages.contains("SECRET_CANARY_TOKEN"));
    assert!(!messages.contains("hooks.slack.com"));
}

#[tokio::test]
async fn test_slack_payload_contains_no_memory_content() {
    let server = SlackMockServer::start(HttpStatus::Ok).await;
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive,
        external_slack_config(server.url(), 1),
        OsNotifier::disabled(),
        ExternalNotifier::slack_for_tests(
            Arc::new(memoryd::notifications::external::ReqwestSlackWebhook::new()),
            Arc::new(RecordingSleeper::default()),
        ),
    );

    dispatcher.dispatch_event(reality_check_due()).await;

    let bodies = server.bodies();
    assert_eq!(bodies.len(), 1);
    let payload = bodies.join("\n");
    assert!(!payload.contains("Prefer CITEXT for email columns"));
    assert!(!payload.contains("Alice Example"));
    assert!(!payload.contains("body with sensitive memory content"));
}

#[tokio::test]
async fn test_lagged_dispatcher_logs_warning_and_continues() {
    let (sender, receiver) = tokio::sync::broadcast::channel(1);

    sender.send(secret_event()).expect("first event sends");
    sender.send(blocking_conflict_event()).expect("second event sends");

    let passive = PassiveQueue::new();
    let dispatcher = test_dispatcher(passive.clone(), NotificationConfig::default());
    let handle = tokio::spawn(dispatcher.run(receiver));

    sender.send(reality_check_due()).expect("post-lag event sends");

    wait_for_message_count(&passive, 1).await;
    assert!(passive.messages().iter().any(|message| message.contains("Reality Check")));

    drop(sender);
    timeout(TokioDuration::from_secs(1), handle)
        .await
        .expect("dispatcher exits when channel closes")
        .expect("task joins");
}

#[tokio::test]
async fn test_smtp_password_read_from_env_var() {
    std::env::set_var("TEST_SMTP_PW", "actual-secret-from-env");

    let email = Arc::new(RecordingEmailDelivery::default());
    let external = ExternalNotifier::email_for_tests(email.clone(), Arc::new(RecordingSleeper::default()));
    let dispatcher = NotificationDispatcher::new(
        PassiveQueue::new(),
        email_config("TEST_SMTP_PW"),
        OsNotifier::disabled(),
        external,
    );

    dispatcher.dispatch_event(reality_check_due()).await;

    let messages = email.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].password, "actual-secret-from-env");
    assert_ne!(messages[0].password, "TEST_SMTP_PW");

    std::env::remove_var("TEST_SMTP_PW");
}

#[test]
fn test_email_message_debug_redacts_password() {
    let debug = format!(
        "{:?}",
        EmailMessage {
            smtp_host: "smtp.example.test".to_owned(),
            smtp_port: 587,
            smtp_user: "memorum".to_owned(),
            password: "smtp-secret-canary".to_owned(),
            to: "to@example.test".to_owned(),
            from: "from@example.test".to_owned(),
            subject: "subject".to_owned(),
            body: "body".to_owned(),
        }
    );

    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("smtp-secret-canary"));
}

#[tokio::test]
async fn test_smtp_password_missing_env_var_logs_error_and_disables() {
    std::env::remove_var("MISSING_TEST_SMTP_PW");

    let email = Arc::new(RecordingEmailDelivery::default());
    let external = ExternalNotifier::email_for_tests(email.clone(), Arc::new(RecordingSleeper::default()));
    let dispatcher = NotificationDispatcher::new(
        PassiveQueue::new(),
        email_config("MISSING_TEST_SMTP_PW"),
        OsNotifier::disabled(),
        external,
    );

    dispatcher.dispatch_event(reality_check_due()).await;

    assert_eq!(email.messages().len(), 0);
}

fn test_dispatcher(passive: PassiveQueue, config: NotificationConfig) -> NotificationDispatcher {
    NotificationDispatcher::new(passive, config, OsNotifier::disabled(), ExternalNotifier::disabled())
}

fn all_notification_events() -> Vec<NotificationEvent> {
    vec![
        secret_event(),
        blocking_conflict_event(),
        NotificationEvent::ReviewQueueOverThreshold { count: 51, threshold: 50 },
        NotificationEvent::DreamRunCompleted {
            scope: "project:agent-memory".to_owned(),
            promoted: 2,
            queued: 1,
            dropped: 0,
        },
        reality_check_due(),
        NotificationEvent::RealityCheckOverdue {
            last_completed_at: Some(Utc.with_ymd_and_hms(2026, 4, 6, 9, 0, 0).unwrap()),
            weeks_skipped: 3,
        },
        NotificationEvent::DailySynthesisSummaryReady { scope: "daily".to_owned() },
    ]
}

fn secret_event() -> NotificationEvent {
    NotificationEvent::LeakedSecretDetected { memory_id: MemoryId::new("mem_20260501_0123456789abcdef_000001") }
}

fn blocking_conflict_event() -> NotificationEvent {
    NotificationEvent::BlockingMergeConflict { path: "memories/project/conflict.md".to_owned() }
}

fn reality_check_due() -> NotificationEvent {
    NotificationEvent::RealityCheckDue { due_at: Utc.with_ymd_and_hms(2026, 5, 4, 9, 0, 0).unwrap() }
}

fn external_slack_config(webhook_url: String, retry_max: usize) -> NotificationConfig {
    NotificationConfig {
        external: ExternalNotificationConfig {
            channel: Some(ExternalChannelConfig::Slack { webhook_url }),
            triggers: vec![NotificationTrigger::RealityCheckDue],
            retry_max,
            retry_backoff_seconds: vec![30, 120, 600],
        },
        ..Default::default()
    }
}

fn email_config(smtp_password_env: &str) -> NotificationConfig {
    NotificationConfig {
        external: ExternalNotificationConfig {
            channel: Some(ExternalChannelConfig::Email(EmailNotificationConfig {
                smtp_host: "smtp.example.test".to_owned(),
                smtp_port: 587,
                smtp_user: "memorum".to_owned(),
                smtp_password_env: smtp_password_env.to_owned(),
                to: "trey@example.test".to_owned(),
                from: "memorum@example.test".to_owned(),
            })),
            triggers: vec![NotificationTrigger::RealityCheckDue],
            retry_max: 1,
            retry_backoff_seconds: vec![30, 120, 600],
        },
        ..Default::default()
    }
}

async fn wait_for_message_count(queue: &PassiveQueue, expected: usize) {
    timeout(TokioDuration::from_secs(1), async {
        loop {
            if queue.messages().len() >= expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("passive queue receives expected messages");
}

#[derive(Default)]
struct RecordingOsSink {
    notifications: Mutex<Vec<OsNotification>>,
}

struct FailingSlackWebhook {
    error: String,
}

impl memoryd::notifications::external::SlackWebhook for FailingSlackWebhook {
    fn post(
        &self,
        _webhook_url: &str,
        _payload: memoryd::notifications::external::SlackPayload,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>> {
        let error = self.error.clone();
        Box::pin(async move { Err(ExternalDeliveryError::new(error)) })
    }
}

impl RecordingOsSink {
    fn calls(&self) -> usize {
        self.notifications.lock().expect("os notifications lock").len()
    }
}

impl OsNotificationSink for RecordingOsSink {
    fn send(&self, notification: &OsNotification) -> Result<(), String> {
        self.notifications.lock().expect("os notifications lock").push(notification.clone());
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.durations.lock().expect("sleep durations lock").push(duration);
        Box::pin(async {})
    }
}

#[derive(Default)]
struct RecordingEmailDelivery {
    messages: Mutex<Vec<EmailMessage>>,
}

impl RecordingEmailDelivery {
    fn messages(&self) -> Vec<EmailMessage> {
        self.messages.lock().expect("email messages lock").clone()
    }
}

impl EmailDelivery for RecordingEmailDelivery {
    fn send(
        &self,
        message: EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>> {
        self.messages.lock().expect("email messages lock").push(message);
        Box::pin(async { Ok(()) })
    }
}

enum HttpStatus {
    Ok,
    InternalServerError,
}

struct SlackMockServer {
    url: String,
    requests: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl SlackMockServer {
    async fn start(status: HttpStatus) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("slack mock binds");
        let address = listener.local_addr().expect("slack mock address");
        let requests = Arc::new(AtomicUsize::new(0));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let server_requests = requests.clone();
        let server_bodies = bodies.clone();

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                server_requests.fetch_add(1, Ordering::SeqCst);
                let mut buffer = vec![0; 4096];
                let bytes_read = stream.read(&mut buffer).await.expect("slack mock request reads");
                server_bodies
                    .lock()
                    .expect("slack mock bodies lock")
                    .push(String::from_utf8_lossy(&buffer[..bytes_read]).into_owned());
                // `Connection: close` forces the reqwest client to open a fresh TCP
                // connection per retry instead of reusing a keep-alive connection.
                // The mock counts accept()s and handles one request per connection,
                // so without this the accept count undercounts attempts under load.
                let response = match status {
                    HttpStatus::Ok => "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    HttpStatus::InternalServerError => {
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    }
                };
                stream.write_all(response.as_bytes()).await.expect("slack mock response writes");
            }
        });

        Self { url: format!("http://{address}/slack"), requests, bodies }
    }

    fn url(&self) -> String {
        self.url.clone()
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().expect("slack mock bodies lock").clone()
    }
}
