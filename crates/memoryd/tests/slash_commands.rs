use chrono::{DateTime, Utc};
use memoryd::protocol::{ComponentScores, MemoryId, MemoryStatus, RealityCheckItem};
use memoryd::slash_commands::format_reality_check_output;

#[test]
fn test_slash_reality_check_formats_scored_list() {
    let items = vec![
        item("mem_20260501_a1b2c3d4e5f60718_000001", "My preferred stack is TypeScript + Rust", "me/identity", 0.82),
        item("mem_20260501_a1b2c3d4e5f60718_000002", "atlasos uses Postgres 15 with CITEXT", "project:atlasos", 0.71),
        item(
            "mem_20260501_a1b2c3d4e5f60718_000003",
            "Stream G exposes Reality Check in TUI panel 8",
            "project:agent-memory",
            0.66,
        ),
    ];

    let output = format_reality_check_output(&items);

    assert!(output.contains("## Reality Check — 3 memories to review"));
    assert!(output.contains("1. \"My preferred stack is TypeScript + Rust\" (me/identity, score: 0.82)"));
    assert!(output.contains("2. \"atlasos uses Postgres 15 with CITEXT\" (project:atlasos, score: 0.71)"));
    assert!(output.contains("3. \"Stream G exposes Reality Check in TUI panel 8\" (project:agent-memory, score: 0.66)"));
    assert!(output.contains("Run `memoryd reality-check run` or open TUI panel 8 to complete the review."));
}

#[test]
fn test_slash_reality_check_encrypted_item_shown_as_encrypted() {
    let mut encrypted_item = item("mem_20260501_a1b2c3d4e5f60718_000004", "Sensitive memory title", "me/private", 0.93);
    encrypted_item.encrypted = true;

    let output = format_reality_check_output(&[encrypted_item]);

    assert!(output.contains("1. [encrypted item, score: 0.93]"));
    assert!(!output.contains("Sensitive memory title"));
}

#[test]
fn test_slash_reality_check_no_items_pending() {
    let output = format_reality_check_output(&[]);

    assert!(output.contains("No Reality Check items pending."));
    assert!(!output.contains("memories to review"));
}

#[test]
fn test_slash_reality_check_output_contains_no_raw_bodies() {
    let raw_body = "AWS key AKIA1234567890ABCDEF must never appear";
    let leaked_title = format!("{raw_body} in body");
    let item = item("mem_20260501_a1b2c3d4e5f60718_000005", &leaked_title, "me/private", 0.88);

    let output = format_reality_check_output(&[item]);

    assert!(!output.contains(raw_body));
    assert!(!output.contains("AKIA1234567890ABCDEF"));
    assert!(output.contains("[encrypted item, score: 0.88]"));
}

fn item(id: &str, title: &str, namespace: &str, score: f64) -> RealityCheckItem {
    RealityCheckItem {
        memory_id: MemoryId::try_new(id).expect("fixture id is valid"),
        title: title.to_owned(),
        namespace: namespace.to_owned(),
        status: MemoryStatus::Active,
        sensitivity: None,
        score,
        component_scores: ComponentScores {
            days_since_observed_norm: 0.7,
            recall_frequency_norm: 0.2,
            cross_source_corroboration: 0.5,
            confidence_decay: 0.6,
            sensitivity_weight: 0.1,
        },
        encrypted: false,
        last_observed_at: instant("2026-04-24T08:30:00Z"),
        recall_count_30d: 4,
        last_recalled_at: Some(instant("2026-05-01T09:00:00Z")),
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp is valid").with_timezone(&Utc)
}
