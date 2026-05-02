use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const PASSIVE_QUEUE_CAPACITY: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveNotification {
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct PassiveQueue {
    inner: Arc<Mutex<VecDeque<PassiveNotification>>>,
}

impl PassiveQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&self, message: impl Into<String>) {
        self.append_at(message, Utc::now());
    }

    pub fn append_at(&self, message: impl Into<String>, created_at: DateTime<Utc>) {
        let mut entries = self.inner.lock().expect("passive notification queue lock poisoned");
        if entries.len() == PASSIVE_QUEUE_CAPACITY {
            entries.pop_front();
        }
        entries.push_back(PassiveNotification { message: message.into(), created_at });
    }

    pub fn entries(&self) -> Vec<PassiveNotification> {
        self.inner.lock().expect("passive notification queue lock poisoned").iter().cloned().collect()
    }

    pub fn messages(&self) -> Vec<String> {
        self.entries().into_iter().map(|entry| entry.message).collect()
    }
}
