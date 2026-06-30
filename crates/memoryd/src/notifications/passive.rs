use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::protocol::NotificationSnapshot;

const PASSIVE_QUEUE_CAPACITY: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveNotification {
    pub message: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub dedup_key: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PassiveQueue {
    inner: Arc<Mutex<VecDeque<PassiveNotification>>>,
}

impl PassiveQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&self, message: impl Into<String>) -> bool {
        self.append_with_key(message, None)
    }

    pub fn append_at(&self, message: impl Into<String>, created_at: DateTime<Utc>) -> bool {
        self.append_at_with_key(message, created_at, None)
    }

    pub fn append_with_key(&self, message: impl Into<String>, dedup_key: Option<String>) -> bool {
        self.append_at_with_key(message, Utc::now(), dedup_key)
    }

    pub fn append_at_with_key(
        &self,
        message: impl Into<String>,
        created_at: DateTime<Utc>,
        dedup_key: Option<String>,
    ) -> bool {
        let mut entries = self.inner.lock().expect("passive notification queue lock poisoned");
        if let Some(key) = dedup_key.as_deref() {
            if entries.iter().any(|entry| entry.dedup_key.as_deref() == Some(key)) {
                return false;
            }
        }
        if entries.len() == PASSIVE_QUEUE_CAPACITY {
            entries.pop_front();
        }
        entries.push_back(PassiveNotification { message: message.into(), created_at, dedup_key });
        true
    }

    pub fn clear_by_key(&self, key: &str) {
        self.inner
            .lock()
            .expect("passive notification queue lock poisoned")
            .retain(|entry| entry.dedup_key.as_deref() != Some(key));
    }

    pub fn dedup_keys(&self) -> BTreeSet<String> {
        self.inner
            .lock()
            .expect("passive notification queue lock poisoned")
            .iter()
            .filter_map(|entry| entry.dedup_key.clone())
            .collect()
    }

    pub fn entries(&self) -> Vec<PassiveNotification> {
        self.inner.lock().expect("passive notification queue lock poisoned").iter().cloned().collect()
    }

    pub fn recent_snapshots(&self, limit: Option<usize>) -> Vec<NotificationSnapshot> {
        let mut entries = self.entries();
        let limit = limit.unwrap_or(PASSIVE_QUEUE_CAPACITY).min(PASSIVE_QUEUE_CAPACITY);
        if entries.len() > limit {
            entries = entries.split_off(entries.len() - limit);
        }
        entries.into_iter().map(NotificationSnapshot::from).collect()
    }

    pub fn messages(&self) -> Vec<String> {
        self.entries().into_iter().map(|entry| entry.message).collect()
    }
}

impl From<PassiveNotification> for NotificationSnapshot {
    fn from(notification: PassiveNotification) -> Self {
        Self {
            id: passive_notification_id(&notification),
            kind: "passive".to_owned(),
            message: notification.message,
            created_at: notification.created_at,
        }
    }
}

fn passive_notification_id(notification: &PassiveNotification) -> String {
    let mut digest = Sha256::new();
    digest.update(notification.created_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
    digest.update(b"\0");
    digest.update(notification.message.as_bytes());
    format!("notif_{}", &hex::encode(digest.finalize())[..16])
}
