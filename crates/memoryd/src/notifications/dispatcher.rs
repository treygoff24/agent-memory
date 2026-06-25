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
        self.passive.append_with_key(passive_message(&event), dedup_key(&event));
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
        NotificationEvent::BlockingMergeConflict { path } => {
            format!("Sync is blocked by a merge conflict in {path}.")
        }
        NotificationEvent::OperatorActionRequired { message } => message.clone(),
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

pub(crate) fn blocking_merge_conflict_dedup_key(path: &str) -> String {
    format!("blocking_merge_conflict:{path}")
}

fn dedup_key(event: &NotificationEvent) -> Option<String> {
    match event {
        NotificationEvent::BlockingMergeConflict { path } => Some(blocking_merge_conflict_dedup_key(path)),
        // Recovery-required (and any operator-action) notice is re-emitted on every
        // startup; key it on its content so a restart does not multiply it (I-F3.1).
        NotificationEvent::OperatorActionRequired { message } => Some(format!("operator_action_required:{message}")),
        _ => None,
    }
}

fn os_notification(event: &NotificationEvent) -> OsNotification {
    OsNotification { title: "Memorum".to_owned(), body: passive_message(event) }
}
