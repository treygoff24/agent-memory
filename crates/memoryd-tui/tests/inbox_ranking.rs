use memoryd_tui::inbox::ranking::merge_and_filter;
use memoryd_tui::inbox::{InboxFilter, InboxItem};

#[test]
fn conflict_ranks_before_due_before_review() {
    let items = merge_and_filter(
        vec![vec![
            InboxItem::ReviewCandidate {
                id: "r".into(),
                title: "m-review".into(),
                namespace: "n".into(),
                reason: None,
                age_label: "now".into(),
                body: String::new(),
                body_truncated: false,
            },
            InboxItem::Conflict {
                id: "c".into(),
                title: "z-conflict".into(),
                namespace: "n".into(),
                reason: None,
                age_label: "now".into(),
            },
            InboxItem::RealityCheckDue {
                id: "d".into(),
                title: "a-due".into(),
                namespace: "n".into(),
                score: "0.8".into(),
                age_label: "now".into(),
            },
        ]],
        InboxFilter::All,
        10,
    );

    let ordered_ids = items.iter().map(InboxItem::id).collect::<Vec<_>>();
    assert_eq!(ordered_ids, vec!["c", "d", "r"]);
}
