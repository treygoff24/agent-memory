use memoryd::trust_artifact::{SafeContent, TrustArtifact};
use memoryd_tui::widgets::trust_artifact::TrustArtifactWidget;

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

fn rendered_text(artifact: &TrustArtifact) -> String {
    TrustArtifactWidget::new(artifact)
        .render_lines()
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
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
