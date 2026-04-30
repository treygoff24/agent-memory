use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallCounters {
    pub startup_invoked_total: u64,
    #[serde(default)]
    pub startup_failed_total: BTreeMap<String, u64>,
    pub delta_invoked_total: u64,
    #[serde(default)]
    pub delta_failed_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub budget_exhausted_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SharedRecallCounters {
    inner: Arc<Mutex<RecallCounters>>,
}

impl SharedRecallCounters {
    pub fn snapshot(&self) -> RecallCounters {
        self.inner.lock().map(|counters| counters.clone()).unwrap_or_default()
    }

    pub fn record_startup_success(&self) {
        if let Ok(mut counters) = self.inner.lock() {
            counters.startup_invoked_total += 1;
        }
    }

    pub fn record_startup_failure(&self, code: &str) {
        if let Ok(mut counters) = self.inner.lock() {
            *counters.startup_failed_total.entry(code.to_owned()).or_default() += 1;
        }
    }

    pub fn record_delta_success(&self) {
        if let Ok(mut counters) = self.inner.lock() {
            counters.delta_invoked_total += 1;
        }
    }

    pub fn record_delta_failure(&self, code: &str) {
        if let Ok(mut counters) = self.inner.lock() {
            *counters.delta_failed_total.entry(code.to_owned()).or_default() += 1;
        }
    }
}
