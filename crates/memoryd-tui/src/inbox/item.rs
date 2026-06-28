use memoryd::protocol::{ConflictSummary, EventLogEntry, RecallHitSummary, ReviewQueueItemResponse};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InboxItem {
    ReviewCandidate { id: String, title: String, namespace: String, reason: Option<String>, age_label: String },
    Conflict { id: String, title: String, namespace: String, reason: Option<String>, age_label: String },
    RecallHit { id: String, title: String, namespace: String, age_label: String },
    RealityCheckDue { id: String, title: String, namespace: String, score: String, age_label: String },
    DreamOutput { id: String, title: String, namespace: String, age_label: String },
    Memory { id: String, title: String, namespace: String, age_label: String },
}

impl InboxItem {
    pub fn id(&self) -> &str {
        match self {
            Self::ReviewCandidate { id, .. }
            | Self::Conflict { id, .. }
            | Self::RecallHit { id, .. }
            | Self::RealityCheckDue { id, .. }
            | Self::DreamOutput { id, .. }
            | Self::Memory { id, .. } => id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::ReviewCandidate { title, .. }
            | Self::Conflict { title, .. }
            | Self::RecallHit { title, .. }
            | Self::RealityCheckDue { title, .. }
            | Self::DreamOutput { title, .. }
            | Self::Memory { title, .. } => title,
        }
    }

    pub fn namespace(&self) -> &str {
        match self {
            Self::ReviewCandidate { namespace, .. }
            | Self::Conflict { namespace, .. }
            | Self::RecallHit { namespace, .. }
            | Self::RealityCheckDue { namespace, .. }
            | Self::DreamOutput { namespace, .. }
            | Self::Memory { namespace, .. } => namespace,
        }
    }

    pub fn age_label(&self) -> &str {
        match self {
            Self::ReviewCandidate { age_label, .. }
            | Self::Conflict { age_label, .. }
            | Self::RecallHit { age_label, .. }
            | Self::RealityCheckDue { age_label, .. }
            | Self::DreamOutput { age_label, .. }
            | Self::Memory { age_label, .. } => age_label,
        }
    }

    pub const fn kind(&self) -> InboxKind {
        match self {
            Self::ReviewCandidate { .. } => InboxKind::Review,
            Self::Conflict { .. } => InboxKind::Conflict,
            Self::RecallHit { .. } => InboxKind::Recall,
            Self::RealityCheckDue { .. } => InboxKind::Due,
            Self::DreamOutput { .. } => InboxKind::Dream,
            Self::Memory { .. } => InboxKind::Memory,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxKind {
    Review,
    Conflict,
    Recall,
    Due,
    Dream,
    Memory,
}

impl From<ReviewQueueItemResponse> for InboxItem {
    fn from(item: ReviewQueueItemResponse) -> Self {
        Self::ReviewCandidate {
            id: item.id,
            title: item.summary,
            namespace: "review".to_string(),
            reason: item.reason,
            age_label: item.status.as_str().to_string(),
        }
    }
}

impl From<ConflictSummary> for InboxItem {
    fn from(item: ConflictSummary) -> Self {
        Self::Conflict {
            id: item.id.to_string(),
            title: item.summary,
            namespace: "conflict".to_string(),
            reason: item.reason,
            age_label: item.updated_at.format("%Y-%m-%d").to_string(),
        }
    }
}

impl From<RecallHitSummary> for InboxItem {
    fn from(hit: RecallHitSummary) -> Self {
        Self::RecallHit {
            id: hit.memory_id.to_string(),
            title: hit.summary.unwrap_or_else(|| "recalled memory".to_string()),
            namespace: "recall".to_string(),
            age_label: hit.recalled_at.format("%H:%M").to_string(),
        }
    }
}

impl From<EventLogEntry> for InboxItem {
    fn from(entry: EventLogEntry) -> Self {
        let id = entry.memory_id.map_or_else(|| entry.event_id.to_string(), |id| id.to_string());
        Self::Memory {
            id,
            title: entry.summary,
            namespace: "timeline".to_string(),
            age_label: entry.ts.format("%H:%M").to_string(),
        }
    }
}
