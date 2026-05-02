use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::notifications::config::{EmailNotificationConfig, ExternalChannelConfig, ExternalNotificationConfig};
use crate::notifications::passive::PassiveQueue;
use crate::protocol::NotificationEvent;

#[derive(Clone, Debug, Serialize)]
pub struct SlackPayload {
    text: String,
    blocks: Vec<SlackBlock>,
}

#[derive(Clone, Debug, Serialize)]
struct SlackBlock {
    #[serde(rename = "type")]
    kind: &'static str,
    text: SlackText,
}

#[derive(Clone, Debug, Serialize)]
struct SlackText {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EmailMessage {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub password: String,
    pub to: String,
    pub from: String,
    pub subject: String,
    pub body: String,
}

impl fmt::Debug for EmailMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmailMessage")
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_user", &self.smtp_user)
            .field("password", &"[redacted]")
            .field("to", &self.to)
            .field("from", &self.from)
            .field("subject", &self.subject)
            .field("body", &self.body)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalDeliveryError {
    reason: String,
}

impl ExternalDeliveryError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into() }
    }

    pub fn sanitized_reason(&self) -> &'static str {
        "external delivery failed"
    }
}

impl std::fmt::Display for ExternalDeliveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for ExternalDeliveryError {}

pub trait SlackWebhook: Send + Sync {
    fn post(
        &self,
        webhook_url: &str,
        payload: SlackPayload,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>>;
}

pub trait EmailDelivery: Send + Sync {
    fn send(
        &self,
        message: EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>>;
}

pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

#[derive(Clone)]
pub struct ExternalNotifier {
    slack: Arc<dyn SlackWebhook>,
    email: Arc<dyn EmailDelivery>,
    sleeper: Arc<dyn Sleeper>,
}

impl ExternalNotifier {
    pub fn new() -> Self {
        Self {
            slack: Arc::new(ReqwestSlackWebhook::new()),
            email: Arc::new(LettreEmailDelivery),
            sleeper: Arc::new(TokioSleeper),
        }
    }

    pub fn disabled() -> Self {
        Self {
            slack: Arc::new(DisabledSlackWebhook),
            email: Arc::new(DisabledEmailDelivery),
            sleeper: Arc::new(TokioSleeper),
        }
    }

    pub fn slack_for_tests(slack: Arc<dyn SlackWebhook>, sleeper: Arc<dyn Sleeper>) -> Self {
        Self { slack, email: Arc::new(DisabledEmailDelivery), sleeper }
    }

    pub fn email_for_tests(email: Arc<dyn EmailDelivery>, sleeper: Arc<dyn Sleeper>) -> Self {
        Self { slack: Arc::new(DisabledSlackWebhook), email, sleeper }
    }

    pub async fn dispatch(
        &self,
        event: &NotificationEvent,
        config: &ExternalNotificationConfig,
        passive: &PassiveQueue,
    ) {
        let Some(channel) = &config.channel else {
            return;
        };
        if config.retry_max == 0 {
            return;
        }

        let retry = RetryPolicy { max_attempts: config.retry_max, backoff_seconds: &config.retry_backoff_seconds };
        let result = match channel {
            ExternalChannelConfig::Slack { webhook_url } => {
                self.dispatch_slack(SlackDispatch { webhook_url, event, retry }).await
            }
            ExternalChannelConfig::Email(config) => self.dispatch_email(EmailDispatch { config, event, retry }).await,
        };

        if let Err(error) = result {
            tracing::warn!("external notification failed after retries: {}", error.sanitized_reason());
            passive.append(format!("External notification failed: {}", error.sanitized_reason()));
        }
    }

    async fn dispatch_slack(&self, request: SlackDispatch<'_>) -> Result<(), ExternalDeliveryError> {
        let payload = slack_payload(request.event);
        let mut last_error = None;

        for attempt in 0..request.retry.max_attempts {
            match self.slack.post(request.webhook_url, payload.clone()).await {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
            self.sleep_before_next_attempt(attempt, request.retry).await;
        }

        Err(last_error.unwrap_or_else(|| ExternalDeliveryError::new("slack delivery was not attempted")))
    }

    async fn dispatch_email(&self, request: EmailDispatch<'_>) -> Result<(), ExternalDeliveryError> {
        let password = match std::env::var(&request.config.smtp_password_env) {
            Ok(password) => password,
            Err(_) => {
                tracing::error!("SMTP password env var {} not set", request.config.smtp_password_env);
                return Ok(());
            }
        };
        let message = email_message(request.config, password, request.event);
        let mut last_error = None;

        for attempt in 0..request.retry.max_attempts {
            match self.email.send(message.clone()).await {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
            self.sleep_before_next_attempt(attempt, request.retry).await;
        }

        Err(last_error.unwrap_or_else(|| ExternalDeliveryError::new("email delivery was not attempted")))
    }

    async fn sleep_before_next_attempt(&self, attempt: usize, retry: RetryPolicy<'_>) {
        if attempt + 1 >= retry.max_attempts {
            return;
        }
        let seconds =
            retry.backoff_seconds.get(attempt).copied().or_else(|| retry.backoff_seconds.last().copied()).unwrap_or(30);
        self.sleeper.sleep(Duration::from_secs(seconds)).await;
    }
}

#[derive(Clone, Copy)]
struct RetryPolicy<'a> {
    max_attempts: usize,
    backoff_seconds: &'a [u64],
}

struct SlackDispatch<'a> {
    webhook_url: &'a str,
    event: &'a NotificationEvent,
    retry: RetryPolicy<'a>,
}

struct EmailDispatch<'a> {
    config: &'a EmailNotificationConfig,
    event: &'a NotificationEvent,
    retry: RetryPolicy<'a>,
}

impl Default for ExternalNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ReqwestSlackWebhook {
    client: reqwest::Client,
}

impl ReqwestSlackWebhook {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

impl Default for ReqwestSlackWebhook {
    fn default() -> Self {
        Self::new()
    }
}

impl SlackWebhook for ReqwestSlackWebhook {
    fn post(
        &self,
        webhook_url: &str,
        payload: SlackPayload,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>> {
        let client = self.client.clone();
        let webhook_url = webhook_url.to_owned();
        Box::pin(async move {
            let response = client
                .post(webhook_url)
                .json(&payload)
                .send()
                .await
                .map_err(|error| ExternalDeliveryError::new(error.without_url().to_string()))?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(ExternalDeliveryError::new(format!("Slack webhook returned {}", response.status())))
            }
        })
    }
}

#[derive(Clone)]
struct LettreEmailDelivery;

impl EmailDelivery for LettreEmailDelivery {
    fn send(
        &self,
        message: EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || send_email_with_lettre(message))
                .await
                .map_err(|error| ExternalDeliveryError::new(error.to_string()))?
        })
    }
}

fn send_email_with_lettre(message: EmailMessage) -> Result<(), ExternalDeliveryError> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let email = Message::builder()
        .from(message.from.parse().map_err(|error| ExternalDeliveryError::new(format!("invalid from: {error}")))?)
        .to(message.to.parse().map_err(|error| ExternalDeliveryError::new(format!("invalid to: {error}")))?)
        .subject(message.subject)
        .body(message.body)
        .map_err(|error| ExternalDeliveryError::new(error.to_string()))?;
    let credentials = Credentials::new(message.smtp_user, message.password);
    let mailer = SmtpTransport::relay(&message.smtp_host)
        .map_err(|error| ExternalDeliveryError::new(error.to_string()))?
        .port(message.smtp_port)
        .credentials(credentials)
        .build();

    mailer.send(&email).map_err(|error| ExternalDeliveryError::new(error.to_string()))?;
    Ok(())
}

#[derive(Clone)]
struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[derive(Clone)]
struct DisabledSlackWebhook;

impl SlackWebhook for DisabledSlackWebhook {
    fn post(
        &self,
        _webhook_url: &str,
        _payload: SlackPayload,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone)]
struct DisabledEmailDelivery;

impl EmailDelivery for DisabledEmailDelivery {
    fn send(
        &self,
        _message: EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<(), ExternalDeliveryError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

fn slack_payload(event: &NotificationEvent) -> SlackPayload {
    let summary = external_summary(event);
    SlackPayload {
        text: format!("Memorum: {summary}"),
        blocks: vec![SlackBlock {
            kind: "section",
            text: SlackText {
                kind: "mrkdwn",
                text: format!(
                    "*Memorum Notification*\n{summary}\nRun `memoryd reality-check run` or open the dashboard."
                ),
            },
        }],
    }
}

fn email_message(config: &EmailNotificationConfig, password: String, event: &NotificationEvent) -> EmailMessage {
    let summary = external_summary(event);
    EmailMessage {
        smtp_host: config.smtp_host.clone(),
        smtp_port: config.smtp_port,
        smtp_user: config.smtp_user.clone(),
        password,
        to: config.to.clone(),
        from: config.from.clone(),
        subject: format!("Memorum: {summary}"),
        body: format!("{summary}\n\nRun `memoryd reality-check run` or open the dashboard."),
    }
}

fn external_summary(event: &NotificationEvent) -> String {
    match event {
        NotificationEvent::RealityCheckDue { due_at } => {
            format!("Weekly Reality Check is ready at {}.", due_at.format("%Y-%m-%d %H:%M UTC"))
        }
        NotificationEvent::RealityCheckOverdue { weeks_skipped, .. } => {
            format!("Reality Check is overdue after {weeks_skipped} skipped weeks.")
        }
        NotificationEvent::DailySynthesisSummaryReady { .. } => "Daily synthesis summary is ready.".to_owned(),
        NotificationEvent::ReviewQueueOverThreshold { count, threshold } => {
            format!("Review queue has {count} items over threshold {threshold}.")
        }
        NotificationEvent::DreamRunCompleted { promoted, queued, dropped, .. } => {
            format!("Dream run completed with {promoted} promoted, {queued} queued, and {dropped} dropped.")
        }
        NotificationEvent::LeakedSecretDetected { .. } => "Blocked secret write attempt detected.".to_owned(),
        NotificationEvent::BlockingMergeConflict { .. } => "Sync is blocked by a merge conflict.".to_owned(),
    }
}
