use memoryd::protocol::MemoryId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RealityCheckState {
    pub active_session_id: Option<String>,
    pub reviewed: usize,
    pub deferred: usize,
    pub items_total: usize,
    pub items_reviewed: usize,
    pub current_title: Option<String>,
    pub transition_start_tick: Option<u64>,
}

impl RealityCheckState {
    pub fn progress_label(&self) -> String {
        format!("{} of {}", self.items_reviewed, self.items_total)
    }

    pub fn remaining(&self) -> usize {
        self.items_total.saturating_sub(self.items_reviewed + self.deferred)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusKind {
    None,
    RealityCheck { session: String },
    CorrectEditor { item_id: MemoryId },
}
