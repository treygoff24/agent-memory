use memoryd_tui::state::RealityCheckState;

#[test]
fn progress_label_uses_live_counts() {
    let empty = RealityCheckState { items_total: 0, items_reviewed: 0, ..Default::default() };
    let partial = RealityCheckState { items_total: 12, items_reviewed: 5, ..Default::default() };

    assert_eq!(empty.progress_label(), "0 of 0");
    assert_eq!(partial.progress_label(), "5 of 12");
    assert_eq!(partial.remaining(), 7);
}
