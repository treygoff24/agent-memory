use chrono::{DateTime, Utc};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};

use crate::protocol::RealityCheckItem;

const REALITY_CHECK_FOOTER: &str = "Run `memoryd reality-check run` or open TUI panel 8 to complete the review.";

pub fn format_reality_check_output(items: &[RealityCheckItem]) -> String {
    if items.is_empty() {
        return format!("No Reality Check items pending.\n\n{REALITY_CHECK_FOOTER}\n");
    }

    let classifier = DeterministicPrivacyClassifier::new();
    let mut output = format!("## Reality Check — {} {}\n\n", items.len(), review_count_label(items.len()));

    for (index, item) in items.iter().enumerate() {
        output.push_str(&format!(
            "{}. {} ({}) — {}\n",
            index + 1,
            display_item(item, &classifier),
            display_context(item),
            display_activity(item)
        ));
    }

    output.push('\n');
    output.push_str(REALITY_CHECK_FOOTER);
    output.push('\n');
    output
}

fn review_count_label(count: usize) -> &'static str {
    if count == 1 {
        "memory to review"
    } else {
        "memories to review"
    }
}

fn display_item(item: &RealityCheckItem, classifier: &DeterministicPrivacyClassifier) -> String {
    if item.encrypted {
        return encrypted_item(item);
    }

    match safe_plaintext_fragment(classifier, &item.title) {
        SafeFragmentDecision::Allow => display_safe_title(item).unwrap_or_else(|| encrypted_item(item)),
        SafeFragmentDecision::OmitEncryptedBodyHidden | SafeFragmentDecision::OmitReviewPending => encrypted_item(item),
    }
}

fn display_safe_title(item: &RealityCheckItem) -> Option<String> {
    let title = normalize_title(&item.title);
    (!title.is_empty()).then(|| format!("\"{}\"", title.replace('"', "\\\"")))
}

fn encrypted_item(item: &RealityCheckItem) -> String {
    format!("[encrypted item, score: {:.2}]", item.score)
}

fn display_context(item: &RealityCheckItem) -> String {
    format!("{}, score: {:.2}", item.namespace, item.score)
}

fn display_activity(item: &RealityCheckItem) -> String {
    format!("last observed {}; {}", display_date(item.last_observed_at), display_recall_count(item.recall_count_30d))
}

fn display_recall_count(recall_count_30d: u32) -> String {
    match recall_count_30d {
        1 => "1 recall in 30d".to_owned(),
        count => format!("{count} recalls in 30d"),
    }
}

fn display_date(timestamp: DateTime<Utc>) -> String {
    timestamp.format("%Y-%m-%d").to_string()
}

fn normalize_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}
