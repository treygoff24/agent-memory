use crate::protocol::NotificationEvent;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationConfig {
    pub os: OsNotificationConfig,
    pub external: ExternalNotificationConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsNotificationConfig {
    pub enabled: bool,
    pub triggers: Vec<NotificationTrigger>,
}

impl Default for OsNotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            triggers: vec![
                NotificationTrigger::LeakedSecretDetected,
                NotificationTrigger::BlockingMergeConflict,
                NotificationTrigger::ReviewQueueOver { threshold: 50 },
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalNotificationConfig {
    pub channel: Option<ExternalChannelConfig>,
    pub triggers: Vec<NotificationTrigger>,
    pub retry_max: usize,
    pub retry_backoff_seconds: Vec<u64>,
}

impl Default for ExternalNotificationConfig {
    fn default() -> Self {
        Self {
            channel: None,
            triggers: vec![
                NotificationTrigger::RealityCheckDue,
                NotificationTrigger::DailySynthesisSummary,
                NotificationTrigger::RealityCheckOverdue,
            ],
            retry_max: 3,
            retry_backoff_seconds: vec![30, 120, 600],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalChannelConfig {
    Slack { webhook_url: String },
    Email(EmailNotificationConfig),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmailNotificationConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password_env: String,
    pub to: String,
    pub from: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationTrigger {
    LeakedSecretDetected,
    BlockingMergeConflict,
    ReviewQueueOver { threshold: usize },
    DreamRunCompleted,
    RealityCheckDue,
    RealityCheckOverdue,
    DailySynthesisSummary,
}

impl NotificationTrigger {
    pub fn matches(&self, event: &NotificationEvent) -> bool {
        match (self, event) {
            (Self::LeakedSecretDetected, NotificationEvent::LeakedSecretDetected { .. }) => true,
            (Self::BlockingMergeConflict, NotificationEvent::BlockingMergeConflict { .. }) => true,
            (Self::ReviewQueueOver { threshold }, NotificationEvent::ReviewQueueOverThreshold { count, .. }) => {
                count > threshold
            }
            (Self::DreamRunCompleted, NotificationEvent::DreamRunCompleted { promoted, queued, .. }) => {
                *promoted > 0 || *queued > 0
            }
            (Self::RealityCheckDue, NotificationEvent::RealityCheckDue { .. }) => true,
            (Self::RealityCheckOverdue, NotificationEvent::RealityCheckOverdue { .. }) => true,
            (Self::DailySynthesisSummary, NotificationEvent::DailySynthesisSummaryReady { .. }) => true,
            _ => false,
        }
    }
}

pub(crate) fn contains_trigger(triggers: &[NotificationTrigger], event: &NotificationEvent) -> bool {
    triggers.iter().any(|trigger| trigger.matches(event))
}
