use ratatui::text::Line;

use memoryd::trust_artifact::SupersessionLink;

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

    pub fn render_lines(&self) -> Vec<Line<'static>> {
        let artifact = self.artifact;
        let mut lines = vec![
            Line::from(artifact.id.as_str().to_owned()),
            Line::from(format!("\"{}\"", artifact.title.display_text())),
            Line::from(format!(
                "namespace: {}  status: {}  sensitivity: {}",
                artifact.namespace, artifact.status, artifact.sensitivity
            )),
            Line::from(format!("source: {}", artifact.source)),
            Line::from(format!("trust: {}", artifact.trust_summary)),
            Line::from(""),
            Line::from("Body:"),
            Line::from(format!("  {}", artifact.body.display_text())),
            Line::from(""),
            section("Confidence"),
            Line::from(format!("Current: {}  Original: {}", artifact.current_confidence, artifact.original_confidence)),
            Line::from(format!("Reason: {}", artifact.confidence_reason.as_deref().unwrap_or("not recorded"))),
            Line::from(""),
            section("Recall"),
            Line::from(format!(
                "Total: {}  (30d: {})  Last: {}",
                artifact.recall.total,
                artifact.recall.last_30_days,
                artifact.recall.last_recalled_at.map_or_else(|| "never".to_owned(), |time| time.to_rfc3339())
            )),
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
            lines.push(Line::from(format!("  {} ({})", decision.policy_applied, decision.policy_source)));
            lines.push(Line::from(format!("    conf_floor: {}", decision.confidence_floor_pass)));
            lines.push(Line::from(format!("    grounding: {}", decision.grounding_satisfied)));
            lines.push(Line::from(format!("    contradiction: {}", decision.contradiction_result)));
            lines.push(Line::from(format!("    tombstone: {}", decision.tombstone_enforced)));
            lines.push(Line::from(format!("    sensitivity_gate: {}", decision.sensitivity_gate_result)));
        }

        lines.push(Line::from(""));
        lines.push(section("Privacy Scan"));
        lines.push(Line::from(format!(" Labels detected: {}", join_or_none(&artifact.privacy_scan.labels_detected))));
        lines.push(Line::from(format!(" Storage action: {}", artifact.privacy_scan.storage_action)));

        lines.push(Line::from(""));
        lines.push(section("Supersession"));
        lines.extend(render_supersession_links("Supersedes", &artifact.supersedes));
        lines.extend(render_supersession_links("Superseded by", &artifact.superseded_by));

        lines.push(Line::from(""));
        lines.push(section("Sync State"));
        lines.push(Line::from(format!(" Devices: {}", join_or_none(&artifact.sync_state.devices))));
        lines.push(Line::from(format!(" Merge status: {}", artifact.sync_state.merge_status)));
        if let Some(claim_lock_status) = &artifact.sync_state.claim_lock_status {
            lines.push(Line::from(format!(" Claim lock: {claim_lock_status}")));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("j/k: scroll   e: edit   f: forget   p: pin   Esc: close"));
        lines
    }
}

fn section(title: &str) -> Line<'static> {
    Line::from(format!("--- {title} ---"))
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
