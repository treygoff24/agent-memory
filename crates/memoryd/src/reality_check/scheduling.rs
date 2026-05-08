use chrono::{DateTime, Duration, Utc};
use tokio::sync::broadcast;

use crate::protocol::NotificationEvent;
use crate::state::RealityCheckState;

const DEFAULT_CRON: &str = "0 9 * * SUN";
const WEEKLY_CADENCE: Duration = Duration::days(7);
const OVERDUE_WINDOW: Duration = Duration::days(21);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RcSchedule {
    expression: String,
}

impl RcSchedule {
    pub fn parse_or_default(expression: &str) -> Self {
        if is_valid_weekly_expression(expression) {
            Self { expression: expression.to_owned() }
        } else {
            Self { expression: DEFAULT_CRON.to_owned() }
        }
    }

    pub fn expression(&self) -> &str {
        &self.expression
    }
}

impl Default for RcSchedule {
    fn default() -> Self {
        Self { expression: DEFAULT_CRON.to_owned() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RcScheduler {
    schedule: RcSchedule,
}

impl RcScheduler {
    pub fn new(expression: &str) -> Self {
        Self { schedule: RcSchedule::parse_or_default(expression) }
    }

    pub fn schedule(&self) -> &RcSchedule {
        &self.schedule
    }

    pub fn is_due(&self, state: &RealityCheckState, now: DateTime<Utc>) -> bool {
        if self.is_snoozed(state, now) {
            return false;
        }
        state
            .last_completed_at
            .is_none_or(|last_completed_at| now.signed_duration_since(last_completed_at) >= WEEKLY_CADENCE)
    }

    pub fn is_snoozed(&self, state: &RealityCheckState, now: DateTime<Utc>) -> bool {
        state.snooze_until.is_some_and(|snooze_until| snooze_until > now)
    }

    pub fn is_overdue(&self, state: &RealityCheckState, now: DateTime<Utc>) -> bool {
        state
            .last_completed_at
            .is_none_or(|last_completed_at| now.signed_duration_since(last_completed_at) > OVERDUE_WINDOW)
    }

    pub fn check_and_fire_if_due(
        &self,
        state: &RealityCheckState,
        now: DateTime<Utc>,
        notifications: &broadcast::Sender<NotificationEvent>,
    ) -> bool {
        if !self.is_due(state, now) {
            return false;
        }
        if self.is_overdue(state, now) {
            let _ = notifications.send(NotificationEvent::RealityCheckOverdue {
                last_completed_at: state.last_completed_at,
                weeks_skipped: skipped_weeks(state.last_completed_at, now),
            });
        }
        let _ = notifications.send(NotificationEvent::RealityCheckDue { due_at: now });
        true
    }
}

fn skipped_weeks(last_completed_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> u32 {
    let Some(last_completed_at) = last_completed_at else {
        return (OVERDUE_WINDOW.num_days() / WEEKLY_CADENCE.num_days()) as u32;
    };
    (now.signed_duration_since(last_completed_at).num_days() / WEEKLY_CADENCE.num_days()).max(0) as u32
}

fn is_valid_weekly_expression(expression: &str) -> bool {
    let fields = expression.split_whitespace().collect::<Vec<_>>();
    fields.len() == 5
        && fields[0].parse::<u8>().is_ok_and(|minute| minute < 60)
        && fields[1].parse::<u8>().is_ok_and(|hour| hour < 24)
        && fields[2] == "*"
        && fields[3] == "*"
        && matches!(fields[4], "SUN" | "0" | "7")
}
