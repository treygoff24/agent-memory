use memorum_theme::{ColorCapability, Theme};
use memoryd::trust_artifact::{SafeContent, TrustArtifact};
use memoryd_tui::theme_glue::ThemeStyles;
use memoryd_tui::widgets::trust_artifact::TrustArtifactWidget;
use ratatui::text::Line;

fn styles() -> ThemeStyles {
    ThemeStyles::from_theme(&Theme::default_warm_dark(), ColorCapability::TrueColor)
}

fn full_plaintext_artifact() -> TrustArtifact {
    serde_json::from_value(serde_json::json!({
        "id": "mem_20260501_0123456789abcdef_000009",
        "namespace": "project:atlasos",
        "status": "active",
        "sensitivity": "internal",
        "source": "substrate:projects/atlasos/deploy-target.md",
        "title": {
            "kind": "plaintext",
            "value": "Deploy target is production ECS"
        },
        "body": {
            "kind": "plaintext",
            "value": "The ECS cluster in us-east-1 is the production deployment target."
        },
        "current_confidence": "0.95",
        "original_confidence": "0.90",
        "confidence_reason": "user confirmed; corroborated by codex-cli and claude-code",
        "trust_summary": "high trust; policy-promoted",
        "recall": {
            "total": 28,
            "last_30_days": 12,
            "last_recalled_at": "2026-05-01T11:02:00Z"
        },
        "provenance_chain": [
            {
                "timestamp": "2026-05-01T11:02:00Z",
                "kind": "recall_hit",
                "summary": "recalled in delta-block",
                "evidence": "session:claude-code",
                "device": "desktop"
            },
            {
                "timestamp": "2026-04-30T14:22:00Z",
                "kind": "write_committed",
                "summary": "written by codex-cli",
                "evidence": "sess_abc123",
                "device": "macbook"
            }
        ],
        "policy_decisions": [
            {
                "policy_applied": "project-standard@v2",
                "policy_source": "disk",
                "confidence_floor_pass": "pass (0.90 >= 0.80)",
                "grounding_satisfied": "2 source refs resolved",
                "contradiction_result": "none detected",
                "tombstone_enforced": "no matching tombstone",
                "sensitivity_gate_result": "pass (internal)"
            }
        ],
        "privacy_scan": {
            "labels_detected": ["none"],
            "storage_action": "plaintext"
        },
        "supersedes": [
            {
                "id": "mem_20260428_0123456789abcdef_000004",
                "timestamp": "2026-04-28T00:00:00Z",
                "title": {
                    "kind": "plaintext",
                    "value": "Deploy target ECS (initial)"
                }
            }
        ],
        "superseded_by": [],
        "sync_state": {
            "devices": ["macbook (written here)", "desktop (synced 2026-05-01 06:00)"],
            "merge_status": "clean",
            "claim_lock_status": "Stream I not active"
        }
    }))
    .expect("test trust artifact fixture matches daemon DTO")
}

fn rendered_lines(artifact: &TrustArtifact) -> Vec<Line<'static>> {
    TrustArtifactWidget::new(artifact).render_lines(&styles())
}

fn rendered_text(artifact: &TrustArtifact) -> String {
    rendered_lines(artifact).into_iter().map(|line| line.to_string()).collect::<Vec<_>>().join("\n")
}

/// Find the first rendered line whose flattened text contains `needle`, then
/// return the `Style` of the first span within that line whose content contains
/// `fragment`. Lets tests assert per-fragment severity coloring.
fn span_style_for(artifact: &TrustArtifact, needle: &str, fragment: &str) -> ratatui::style::Style {
    let lines = rendered_lines(artifact);
    let line = lines
        .iter()
        .find(|line| line.to_string().contains(needle))
        .unwrap_or_else(|| panic!("no rendered line containing {needle:?}"));
    line.spans
        .iter()
        .find(|span| span.content.contains(fragment))
        .unwrap_or_else(|| panic!("no span containing {fragment:?} in line {:?}", line.to_string()))
        .style
}

#[test]
fn test_all_sections_present_for_plaintext_memory() {
    let text = rendered_text(&full_plaintext_artifact());

    for heading in [
        "Body:",
        "Confidence",
        "Recall",
        "Provenance",
        "Policy Decisions",
        "Privacy Scan",
        "Supersession",
        "Sync State",
    ] {
        assert!(text.contains(heading), "missing heading {heading} in:\n{text}");
    }

    assert!(text.contains("source: substrate:projects/atlasos/deploy-target.md"));
    assert!(text.contains("trust: high trust; policy-promoted"));
    assert!(text.contains("evidence: session:claude-code"));
    assert!(text.contains("Supersedes: mem_20260428_0123456789abcdef_000004"));
}

#[test]
fn test_encrypted_memory_shows_content_redacted_without_leaking_private_text() {
    let mut artifact = full_plaintext_artifact();
    artifact.title = SafeContent::Encrypted;
    artifact.body = SafeContent::Encrypted;
    artifact.supersedes[0].title = SafeContent::Encrypted;

    let text = rendered_text(&artifact);

    assert!(text.contains("[encrypted"));
    assert!(text.contains("Confidence"));
    assert!(text.contains("Policy Decisions"));
    assert!(text.contains("Privacy Scan"));
    assert!(text.contains("Sync State"));
    assert!(!text.contains("Deploy target is production ECS"));
    assert!(!text.contains("The ECS cluster in us-east-1"));
    assert!(!text.contains("Deploy target ECS (initial)"));
}

#[test]
fn test_provenance_chain_renders_chronologically() {
    let text = rendered_text(&full_plaintext_artifact());
    let provenance = text
        .split("--- Provenance ---")
        .nth(1)
        .expect("provenance section should render")
        .split("--- Policy Decisions ---")
        .next()
        .expect("policy section should follow provenance");

    let written = provenance.find("2026-04-30T14:22:00").expect("written event should render");
    let recalled = provenance.find("2026-05-01T11:02:00").expect("recalled event should render");

    assert!(written < recalled, "provenance must render oldest first:\n{provenance}");
}

#[test]
fn test_policy_decision_expands_all_governance_fields() {
    let text = rendered_text(&full_plaintext_artifact());

    for field in ["conf_floor:", "grounding:", "contradiction:", "tombstone:", "sensitivity_gate:"] {
        assert!(text.contains(field), "missing policy field {field} in:\n{text}");
    }
}

#[test]
fn quarantined_status_renders_with_bad_severity_color() {
    let mut artifact = full_plaintext_artifact();
    artifact.status = "quarantined".to_owned();

    let style = span_style_for(&artifact, "status: quarantined", "quarantined");
    assert_eq!(style, styles().bad, "quarantined status should use the theme bad/error color");
}

#[test]
fn active_status_renders_with_ok_severity_color() {
    let style = span_style_for(&full_plaintext_artifact(), "status: active", "active");
    assert_eq!(style, styles().ok, "active status should use the theme ok color");
}

#[test]
fn high_confidence_drift_renders_current_as_bad() {
    let mut artifact = full_plaintext_artifact();
    artifact.original_confidence = "0.90".to_owned();
    artifact.current_confidence = "0.30".to_owned();

    let style = span_style_for(&artifact, "Current: 0.30", "0.30");
    assert_eq!(style, styles().bad, "a >50% confidence drop should color current confidence bad");
}

#[test]
fn conflicted_merge_status_renders_with_warn_color() {
    let mut artifact = full_plaintext_artifact();
    artifact.sync_state.merge_status = "modified".to_owned();

    let style = span_style_for(&artifact, "Merge status: modified", "modified");
    assert_eq!(style, styles().warn, "a non-clean merge status should color as warn drift");
}

#[test]
fn failing_policy_gate_renders_with_bad_color() {
    let mut artifact = full_plaintext_artifact();
    artifact.policy_decisions[0].grounding_satisfied = "fail (0 source refs)".to_owned();

    let style = span_style_for(&artifact, "fail (0 source refs)", "fail");
    assert_eq!(style, styles().bad, "a failing governance gate should color bad");
}

#[test]
fn detected_privacy_labels_render_with_warn_color() {
    let mut artifact = full_plaintext_artifact();
    artifact.privacy_scan.labels_detected = vec!["pii.email".to_owned()];

    let style = span_style_for(&artifact, "Labels detected: pii.email", "pii.email");
    assert_eq!(style, styles().warn, "detected privacy labels should color as warn");
}
