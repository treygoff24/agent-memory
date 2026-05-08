use super::filter::InboxFilter;
use super::item::{InboxItem, InboxKind};

pub fn merge_and_filter(sources: Vec<Vec<InboxItem>>, filter: InboxFilter, cap_per_source: usize) -> Vec<InboxItem> {
    let mut items = sources
        .into_iter()
        .flat_map(|source| source.into_iter().take(cap_per_source))
        .filter(|item| filter.matches(item))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| urgency(right).cmp(&urgency(left)).then_with(|| left.title().cmp(right.title())));
    items
}

fn urgency(item: &InboxItem) -> u8 {
    match item.kind() {
        InboxKind::Conflict => 6,
        InboxKind::Due => 5,
        InboxKind::Review => 4,
        InboxKind::Dream => 3,
        InboxKind::Recall => 2,
        InboxKind::Memory => 1,
    }
}
