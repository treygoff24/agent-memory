use super::item::{InboxItem, InboxKind};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InboxFilter {
    #[default]
    All,
    Review,
    Conflicts,
    Recall,
    Dreams,
    Due,
}

impl InboxFilter {
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Review => "review",
            Self::Conflicts => "conflicts",
            Self::Recall => "recall",
            Self::Dreams => "dreams",
            Self::Due => "due",
        }
    }

    pub const fn all() -> [Self; 6] {
        [Self::All, Self::Review, Self::Conflicts, Self::Recall, Self::Dreams, Self::Due]
    }

    pub fn matches(self, item: &InboxItem) -> bool {
        match self {
            Self::All => true,
            Self::Review => item.kind() == InboxKind::Review,
            Self::Conflicts => item.kind() == InboxKind::Conflict,
            Self::Recall => item.kind() == InboxKind::Recall,
            Self::Dreams => item.kind() == InboxKind::Dream,
            Self::Due => item.kind() == InboxKind::Due,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FilterCounts {
    pub all: usize,
    pub review: usize,
    pub conflicts: usize,
    pub recall: usize,
    pub dreams: usize,
    pub due: usize,
}

impl FilterCounts {
    pub fn from_items(items: &[InboxItem]) -> Self {
        Self {
            all: items.len(),
            review: items.iter().filter(|item| item.kind() == InboxKind::Review).count(),
            conflicts: items.iter().filter(|item| item.kind() == InboxKind::Conflict).count(),
            recall: items.iter().filter(|item| item.kind() == InboxKind::Recall).count(),
            dreams: items.iter().filter(|item| item.kind() == InboxKind::Dream).count(),
            due: items.iter().filter(|item| item.kind() == InboxKind::Due).count(),
        }
    }

    pub const fn get(&self, filter: InboxFilter) -> usize {
        match filter {
            InboxFilter::All => self.all,
            InboxFilter::Review => self.review,
            InboxFilter::Conflicts => self.conflicts,
            InboxFilter::Recall => self.recall,
            InboxFilter::Dreams => self.dreams,
            InboxFilter::Due => self.due,
        }
    }
}
