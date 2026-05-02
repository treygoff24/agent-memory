use tokio::sync::broadcast;

use crate::notifications::config::{contains_trigger, NotificationConfig};
use crate::notifications::external::ExternalNotifier;
use crate::notifications::os::{OsNotification, OsNotifier};
use crate::notifications::passive::PassiveQueue;
use crate::protocol::NotificationEvent;

#[derive(Clone)]
pub struct NotificationDispatcher {
    passive: PassiveQueue,
    config: NotificationConfig,
    os: OsNotifier,
    external: ExternalNotifier,
}

impl NotificationDispatcher {
    pub fn new(passive: PassiveQueue, config: NotificationConfig, os: OsNotifier, external: ExternalNotifier) -> Self {
        Self { passive, config, os, external }
    }

    pub fn production(passive: PassiveQueue, config: NotificationConfig) -> Self {
        let os = if config.os.enabled { OsNotifier::detect() } else { OsNotifier::disabled() };
        Self::new(passive, config, os, ExternalNotifier::new())
    }

    pub async fn dispatch_event(&self, event: NotificationEvent) {
        self.passive.append(passive_message(&event));
        if self.config.os.enabled && contains_trigger(&self.config.os.triggers, &event) {
            self.os.notify(&os_notification(&event));
        }
        if contains_trigger(&self.config.external.triggers, &event) {
            self.external.dispatch(&event, &self.config.external, &self.passive).await;
        }
    }

    pub async fn run(self, mut events: broadcast::Receiver<NotificationEvent>) {
        loop {
            match events.recv().await {
                Ok(event) => self.dispatch_event(event).await,
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!("notification dispatcher lagged {count} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

fn passive_message(event: &NotificationEvent) -> String {
    match event {
        NotificationEvent::LeakedSecretDetected { .. } => "Blocked secret write attempt detected.".to_owned(),
        NotificationEvent::BlockingMergeConflict { .. } => "Sync is blocked by a merge conflict.".to_owned(),
        NotificationEvent::ReviewQueueOverThreshold { count, threshold } => {
            format!("Review queue has {count} items over threshold {threshold}.")
        }
        NotificationEvent::DreamRunCompleted { promoted, queued, dropped, .. } => {
            format!("Dream run completed with {promoted} promoted, {queued} queued, and {dropped} dropped.")
        }
        NotificationEvent::RealityCheckDue { due_at } => {
            format!("Weekly Reality Check is ready at {}.", due_at.format("%Y-%m-%d %H:%M UTC"))
        }
        NotificationEvent::RealityCheckOverdue { weeks_skipped, .. } => {
            format!("Reality Check is overdue after {weeks_skipped} skipped weeks.")
        }
        NotificationEvent::DailySynthesisSummaryReady { .. } => "Daily synthesis summary is ready.".to_owned(),
    }
}

fn os_notification(event: &NotificationEvent) -> OsNotification {
    OsNotification { title: "Memorum".to_owned(), body: passive_message(event) }
}
