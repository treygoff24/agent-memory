use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallStatusCounters {
    pub startup_invoked_total: u64,
    #[serde(default)]
    pub startup_failed_total: BTreeMap<String, u64>,
    pub delta_invoked_total: u64,
    #[serde(default)]
    pub delta_failed_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub budget_exhausted_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub dream_question_omitted_total: BTreeMap<String, u64>,
    /// Bounded in-process latency distributions, keyed by stable surface name.
    #[serde(default)]
    pub latency: BTreeMap<String, LatencyPercentiles>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub count: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
}

#[derive(Debug, Default)]
struct RecallCounterState {
    counters: RecallStatusCounters,
    latency_samples: BTreeMap<String, VecDeque<u64>>,
}

#[derive(Debug, Clone, Default)]
pub struct SharedRecallCounters {
    inner: Arc<Mutex<RecallCounterState>>,
}

impl SharedRecallCounters {
    /// Lock the inner counters, recovering the guard on poison rather than panicking.
    fn locked(&self) -> std::sync::MutexGuard<'_, RecallCounterState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> RecallStatusCounters {
        let mut state = self.locked();
        let distributions =
            state.latency_samples.iter().map(|(surface, samples)| (surface.clone(), percentiles(samples))).collect();
        state.counters.latency = distributions;
        state.counters.clone()
    }

    pub fn record_startup_success(&self) {
        self.locked().counters.startup_invoked_total += 1;
    }

    pub fn record_startup_failure(&self, code: &str) {
        *self.locked().counters.startup_failed_total.entry(code.to_owned()).or_default() += 1;
    }

    pub fn record_delta_success(&self) {
        self.locked().counters.delta_invoked_total += 1;
    }

    pub fn record_delta_failure(&self, code: &str) {
        *self.locked().counters.delta_failed_total.entry(code.to_owned()).or_default() += 1;
    }

    pub fn record_budget_exhausted(&self, section: &str) {
        *self.locked().counters.budget_exhausted_total.entry(section.to_owned()).or_default() += 1;
    }

    pub fn record_dream_question_omissions(&self, omissions: &BTreeMap<String, u64>) {
        let mut state = self.locked();
        for (reason, count) in omissions {
            *state.counters.dream_question_omitted_total.entry(reason.clone()).or_default() += count;
        }
    }

    pub fn record_latency(&self, surface: &str, elapsed: std::time::Duration) {
        const MAX_SAMPLES: usize = 1_024;
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let mut state = self.locked();
        let samples = state.latency_samples.entry(surface.to_owned()).or_default();
        if samples.len() == MAX_SAMPLES {
            samples.pop_front();
        }
        samples.push_back(micros);
    }
}

fn percentiles(samples: &VecDeque<u64>) -> LatencyPercentiles {
    let mut sorted = samples.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    let at = |percent: usize| {
        let index = (sorted.len().saturating_mul(percent).saturating_add(99) / 100).saturating_sub(1);
        sorted.get(index).copied().unwrap_or_default()
    };
    LatencyPercentiles { count: sorted.len() as u64, p50_us: at(50), p95_us: at(95), p99_us: at(99) }
}

#[cfg(test)]
mod tests {
    use super::SharedRecallCounters;

    #[test]
    fn counters_remain_consistent_under_concurrent_recording() {
        let counters = SharedRecallCounters::default();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let counters = counters.clone();
                scope.spawn(move || {
                    for _ in 0..64 {
                        counters.record_startup_success();
                        counters.record_delta_success();
                        counters.record_budget_exhausted("recent-memory");
                    }
                });
            }
        });

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.startup_invoked_total, 512);
        assert_eq!(snapshot.delta_invoked_total, 512);
        assert_eq!(snapshot.budget_exhausted_total.get("recent-memory"), Some(&512));
    }

    #[test]
    fn latency_snapshot_exposes_percentiles() {
        let counters = SharedRecallCounters::default();
        for millis in 1..=100 {
            counters.record_latency("search_local", std::time::Duration::from_millis(millis));
        }
        let latency = &counters.snapshot().latency["search_local"];
        assert_eq!(latency.count, 100);
        assert_eq!(latency.p50_us, 50_000);
        assert_eq!(latency.p95_us, 95_000);
        assert_eq!(latency.p99_us, 99_000);
    }
}
