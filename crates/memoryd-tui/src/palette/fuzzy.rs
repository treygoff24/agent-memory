use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

pub fn score(query: &str, label: &str) -> Option<u32> {
    if query.trim().is_empty() {
        return Some(0);
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
    let base = pattern.match_list([label], &mut matcher).into_iter().next().map(|(_, score)| score)?;
    Some(base.saturating_add(acronym_boost(query, label)))
}

fn acronym_boost(query: &str, label: &str) -> u32 {
    let acronym = label
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|part| part.chars().next())
        .collect::<String>()
        .to_ascii_lowercase();
    if acronym.starts_with(&query.to_ascii_lowercase()) {
        10_000
    } else {
        0
    }
}

pub fn ranked_labels<'a>(query: &str, labels: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut matches =
        labels.into_iter().filter_map(|label| score(query, label).map(|score| (label, score))).collect::<Vec<_>>();
    matches.sort_by(|(left_label, left_score), (right_label, right_score)| {
        right_score.cmp(left_score).then_with(|| left_label.cmp(right_label))
    });
    matches.into_iter().map(|(label, _)| label).collect()
}
