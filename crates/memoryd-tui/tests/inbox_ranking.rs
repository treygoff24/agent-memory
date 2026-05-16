use memoryd_tui::inbox::ranking::merge_and_filter;
use memoryd_tui::inbox::{InboxFilter, InboxItem};

#[test]
fn conflict_ranks_before_due_before_review() {
    let items = merge_and_filter(
        vec![vec![
            InboxItem::ReviewCandidate {
                id: "r".into(),
                title: "review".into(),
                namespace: "n".into(),
                reason: None,
                age_label: "now".into(),
                body: String::new(),
                body_truncated: false,
            },
            InboxItem::Conflict {
                id: "c".into(),
                title: "conflict".into(),
                namespace: "n".into(),
                reason: None,
                age_label: "now".into(),
            },
            InboxItem::RealityCheckDue {
                id: "d".into(),
                title: "due".into(),
                namespace: "n".into(),
                score: "0.8".into(),
                age_label: "now".into(),
            },
        ]],
        InboxFilter::All,
        10,
    );

    assert_eq!(items[0].id(), "c");
    assert_eq!(items[1].id(), "d");
    assert_eq!(items[2].id(), "r");
}
