use ratatui::style::Style;
use ratatui::text::{Line, Span};

use memoryd::trust_artifact::{PolicyDecision, SupersessionLink};

use crate::theme_glue::ThemeStyles;

pub use memoryd::trust_artifact::TrustArtifact;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustArtifactModalState {
    scroll_offset: u16,
}

impl TrustArtifactModalState {
    pub const fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn reset(&mut self) {
        self.scroll_offset = 0;
    }
}

pub struct TrustArtifactWidget<'a> {
    artifact: &'a TrustArtifact,
}

impl<'a> TrustArtifactWidget<'a> {
    pub const fn new(artifact: &'a TrustArtifact) -> Self {
        Self { artifact }
    }

    pub fn render_lines(&self, styles: &ThemeStyles) -> Vec<Line<'static>> {
        let artifact = self.artifact;
        let mut lines = vec![
            Line::from(artifact.id.as_str().to_owned()),
            Line::from(format!("\"{}\"", artifact.title.display_text())),
            status_line(artifact, styles),
            Line::from(format!("source: {}", artifact.source)),
            Line::from(web_evidence_summary(artifact)),
            Line::from(format!("trust: {}", artifact.trust_summary)),
            Line::from(""),
            Line::from("Body:"),
            Line::from(format!("  {}", artifact.body.display_text())),
            Line::from(""),
            section("Confidence"),
            confidence_line(artifact, styles),
            Line::from(format!("Reason: {}", artifact.confidence_reason.as_deref().unwrap_or("not recorded"))),
            Line::from(""),
            section("Recall"),
            Line::from(format!(
                "Total: {}  (30d: {})  Last: {}",
                artifact.recall.total,
                artifact.recall.last_30_days,
                artifact.recall.last_recalled_at.map_or_else(|| "never".to_owned(), |time| time.to_rfc3339())
            )),
            Line::from(format!("Strength: {}", artifact.recall.strength)),
            Line::from(""),
            section("Provenance"),
        ];

        let mut provenance = artifact.provenance_chain.clone();
        provenance.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.device.cmp(&right.device))
                .then_with(|| left.kind.cmp(&right.kind))
        });
        if provenance.is_empty() {
            lines.push(Line::from("  (none recorded)"));
        }
        for (index, event) in provenance.iter().enumerate() {
            lines.push(Line::from(format!(" {}. {}  {}", index + 1, event.timestamp.to_rfc3339(), event.summary)));
            lines.push(Line::from(format!("    evidence: {}", event.evidence)));
            lines.push(Line::from(format!("    device: {}", event.device)));
        }

        lines.push(Line::from(""));
        lines.push(section("Policy Decisions"));
        if artifact.policy_decisions.is_empty() {
            lines.push(Line::from("  (none recorded)"));
        }
        for decision in &artifact.policy_decisions {
            lines.extend(policy_decision_lines(decision, styles));
        }

        lines.push(Line::from(""));
        lines.push(section("Privacy Scan"));
        lines.push(privacy_labels_line(artifact, styles));
        lines.push(Line::from(format!(" Storage action: {}", artifact.privacy_scan.storage_action)));

        lines.push(Line::from(""));
        lines.push(section("Supersession"));
        lines.extend(render_supersession_links("Supersedes", &artifact.supersedes));
        lines.extend(render_supersession_links("Superseded by", &artifact.superseded_by));

        lines.push(Line::from(""));
        lines.push(section("Sync State"));
        lines.push(Line::from(format!(" Devices: {}", join_or_none(&artifact.sync_state.devices))));
        lines.push(merge_status_line(artifact, styles));
        if let Some(claim_lock_status) = &artifact.sync_state.claim_lock_status {
            lines.push(Line::from(vec![
                Span::from(" Claim lock: "),
                Span::styled(claim_lock_status.clone(), styles.info),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("j/k: scroll   e: edit   f: forget   p: pin   Esc: close"));
        lines
    }
}

/// Status line. Quarantined/tombstoned memories are headline failures (bad);
/// superseded/archived are warnings; candidate is informational.
fn status_line(artifact: &TrustArtifact, styles: &ThemeStyles) -> Line<'static> {
    let status_style = status_severity(&artifact.status, styles);
    Line::from(vec![
        Span::from(format!("namespace: {}  status: ", artifact.namespace)),
        Span::styled(artifact.status.clone(), status_style),
        Span::from(format!("  sensitivity: {}", artifact.sensitivity)),
    ])
}

pub fn status_severity(status: &str, styles: &ThemeStyles) -> Style {
    match status {
        "quarantined" | "tombstoned" => styles.bad,
        "superseded" | "archived" => styles.warn,
        "candidate" => styles.info,
        _ => styles.ok,
    }
}

/// Confidence line. When current confidence has decayed below the original, the
/// drop is colored as drift (warn if it slipped at all, bad if it more than
/// halved).
fn confidence_line(artifact: &TrustArtifact, styles: &ThemeStyles) -> Line<'static> {
    let current = artifact.current_confidence.parse::<f64>().ok();
    let original = artifact.original_confidence.parse::<f64>().ok();
    let drift_style = match (current, original) {
        (Some(current), Some(original)) if original > 0.0 && current < original => {
            if current <= original / 2.0 {
                styles.bad
            } else {
                styles.warn
            }
        }
        _ => styles.ok,
    };
    Line::from(vec![
        Span::from("Current: "),
        Span::styled(artifact.current_confidence.clone(), drift_style),
        Span::from(format!("  Original: {}", artifact.original_confidence)),
    ])
}

fn policy_decision_lines(decision: &PolicyDecision, styles: &ThemeStyles) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("  {} ({})", decision.policy_applied, decision.policy_source)),
        policy_field_line("conf_floor", &decision.confidence_floor_pass, styles),
        policy_field_line("grounding", &decision.grounding_satisfied, styles),
        policy_field_line("contradiction", &decision.contradiction_result, styles),
        policy_field_line("tombstone", &decision.tombstone_enforced, styles),
        policy_field_line("sensitivity_gate", &decision.sensitivity_gate_result, styles),
    ]
}

/// A single policy-decision field, colored by whether the gate passed. Values
/// signalling a failure/block are bad; positive outcomes are ok; everything else
/// (e.g. "not recorded", "not applicable") stays muted.
fn policy_field_line(label: &str, value: &str, styles: &ThemeStyles) -> Line<'static> {
    Line::from(vec![
        Span::from(format!("    {label}: ")),
        Span::styled(value.to_owned(), policy_value_severity(value, styles)),
    ])
}

pub fn policy_value_severity(value: &str, styles: &ThemeStyles) -> Style {
    let lowered = value.to_ascii_lowercase();
    if lowered.contains("fail")
        || lowered.contains("refus")
        || lowered.contains("block")
        || lowered.contains("violat")
        || lowered.contains("not satisfied")
        || lowered.contains("not_satisfied")
        || lowered.contains("conflict")
    {
        styles.bad
    } else if lowered.contains("pass") || lowered.contains("satisfied") || lowered.contains("enforced") {
        styles.ok
    } else {
        styles.muted
    }
}

fn privacy_labels_line(artifact: &TrustArtifact, styles: &ThemeStyles) -> Line<'static> {
    let labels = &artifact.privacy_scan.labels_detected;
    let benign = labels.is_empty() || labels.iter().all(|label| label == "none");
    let style = if benign { styles.ok } else { styles.warn };
    Line::from(vec![Span::from(" Labels detected: "), Span::styled(join_or_none(labels), style)])
}

/// Merge status. A non-clean working tree (modified/conflicted) signals sync
/// drift; "unknown" is muted; "clean" is ok.
fn merge_status_line(artifact: &TrustArtifact, styles: &ThemeStyles) -> Line<'static> {
    let merge_status = &artifact.sync_state.merge_status;
    let style = match merge_status.as_str() {
        "clean" => styles.ok,
        "unknown" => styles.muted,
        _ => styles.warn,
    };
    Line::from(vec![Span::from(" Merge status: "), Span::styled(merge_status.clone(), style)])
}

fn section(title: &str) -> Line<'static> {
    Line::from(format!("--- {title} ---"))
}

fn web_evidence_summary(artifact: &TrustArtifact) -> String {
    let Some(evidence) = &artifact.source_evidence else {
        return "web evidence: none".to_string();
    };
    let status = if evidence.available { "available" } else { "unavailable" };
    let mut summary = format!("web evidence: {}#{} ({status})", evidence.artifact_id, evidence.excerpt_id);
    if let Some(final_url) = &evidence.final_url {
        summary.push_str(&format!(" final_url={final_url}"));
    }
    if let Some(quote) = &evidence.quote {
        summary.push_str(&format!(" quote=\"{quote}\""));
    }
    summary
}

fn render_supersession_links(label: &str, links: &[SupersessionLink]) -> Vec<Line<'static>> {
    if links.is_empty() {
        return vec![Line::from(format!(" {label}: (none)"))];
    }

    links
        .iter()
        .map(|link| {
            let timestamp = link.timestamp.map_or_else(|| "unknown".to_owned(), |time| time.to_rfc3339());
            Line::from(format!(" {label}: {} ({})  \"{}\"", link.id.as_str(), timestamp, link.title.display_text()))
        })
        .collect()
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}
