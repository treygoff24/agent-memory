use serde::{Deserialize, Serialize};

const NEXT_ACTION_APPROVE: &str = "review_approve";
const NEXT_ACTION_REJECT: &str = "review_reject";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueue {
    pub items: Vec<ReviewQueueItem>,
}

impl ReviewQueue {
    pub fn from_memory_envelopes<I>(envelopes: I) -> Self
    where
        I: IntoIterator<Item = ReviewMemoryEnvelope>,
    {
        let items = envelopes.into_iter().filter_map(ReviewQueueItem::from_envelope).collect();
        Self { items }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueItem {
    pub id: String,
    pub summary: String,
    pub status: ReviewStatus,
    pub policy_applied: String,
    pub reason: Option<String>,
    pub next_actions: Vec<String>,
}

impl ReviewQueueItem {
    fn from_envelope(envelope: ReviewMemoryEnvelope) -> Option<Self> {
        let status = ReviewStatus::from_review_metadata(&envelope)?;
        Some(Self {
            id: envelope.id,
            summary: envelope.summary,
            status,
            policy_applied: envelope.policy_applied,
            reason: envelope.reason.or_else(|| status.default_reason().map(str::to_string)),
            next_actions: vec![NEXT_ACTION_APPROVE.to_string(), NEXT_ACTION_REJECT.to_string()],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewStatus {
    Quarantined,
    Candidate,
    PendingReview,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quarantined => "quarantined",
            Self::Candidate => "candidate",
            Self::PendingReview => "pending_review",
        }
    }

    fn from_review_metadata(envelope: &ReviewMemoryEnvelope) -> Option<Self> {
        if envelope.status == "quarantined" {
            return Some(Self::Quarantined);
        }
        if envelope.status == "candidate" && envelope.requires_user_confirmation {
            return Some(Self::Candidate);
        }
        if matches!(envelope.review_state.as_deref(), Some("pending") | Some("pending_review") | Some("pending-review"))
        {
            return Some(Self::PendingReview);
        }
        None
    }

    fn default_reason(self) -> Option<&'static str> {
        match self {
            Self::Quarantined => Some("quarantined memory requires review"),
            Self::Candidate => Some("candidate memory requires user confirmation"),
            Self::PendingReview => Some("memory is pending human review"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewMemoryEnvelope {
    pub id: String,
    pub summary: String,
    pub status: String,
    pub requires_user_confirmation: bool,
    pub review_state: Option<String>,
    pub policy_applied: String,
    pub reason: Option<String>,
}
