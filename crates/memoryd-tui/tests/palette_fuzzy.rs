use memoryd_tui::palette::commands::catalog;
use memoryd_tui::palette::fuzzy::ranked_labels;

#[test]
fn fuzzy_matches_review_filter() {
    let labels = catalog().into_iter().map(|command| command.label).collect::<Vec<_>>();
    let ranked = ranked_labels("rev", labels.iter().copied());

    assert_eq!(ranked.first(), Some(&"filter:review"));
}

#[test]
fn acronym_match_ranks_reality_check() {
    let labels = catalog().into_iter().map(|command| command.label).collect::<Vec<_>>();
    let ranked = ranked_labels("rc", labels.iter().copied());

    assert_eq!(ranked.first(), Some(&"reality-check:start"));
}
