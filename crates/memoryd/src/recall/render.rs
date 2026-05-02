use std::collections::HashSet;

use memorum_coordination::{CoordinationInsertion, PeerPresenceEntry, PeerUpdateEntry};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use memory_substrate::{MemoryId, Substrate};

use crate::recall::budget::{estimated_tokens, truncate_utf8_bytes};
use crate::recall::types::{RecallExplanation, RecallSectionName, SessionBinding, STREAM_E_POLICY};

const SUMMARY_MAX_BYTES: usize = 240;
const SNIPPET_MAX_BYTES: usize = 360;
const PENDING_ATTENTION_TOTAL_CAP: usize = 6;
const COORDINATION_POLICY: &str = "stream-i-v0.1";
const PRIVACY_FILTERED_SUMMARY: &str = "[content not available — privacy classification pending]";
const REALITY_CHECK_DUE_ITEM_TEXT: &str =
    "Weekly Reality Check is ready — run `memoryd reality-check run` or open TUI panel 8.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallEntry {
    pub id: String,
    pub summary: String,
    pub snippet: Option<String>,
    pub updated: String,
    pub source_kind: String,
    pub confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedRecallSection {
    pub name: RecallSectionName,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPendingAttention {
    pub body: String,
    pub omitted_count: u64,
    pub reality_check_due_emitted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaRecallItem {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDeltaFrame {
    pub block: String,
    pub budget_used_tokens: usize,
    pub included_item_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrossDeviceStartupUpdates {
    pub from_sync_date: String,
    pub peer_updates: Vec<PeerUpdateEntry>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StartupCoordinationRender<'a> {
    pub same_device: Option<&'a CoordinationInsertion>,
    pub cross_device: Option<&'a CrossDeviceStartupUpdates>,
}

pub fn render_memory_entry(entry: &RecallEntry) -> String {
    let summary = truncate_utf8_bytes(&entry.summary, SUMMARY_MAX_BYTES).value;
    let suffix = format!(
        " (updated {}; source {}; confidence {})",
        escape_xml_text(&entry.updated),
        escape_xml_text(&entry.source_kind),
        escape_xml_text(&entry.confidence)
    );

    let mut rendered = format!("- [{}] {}", escape_xml_text(&entry.id), escape_xml_text(&summary));
    if let Some(snippet) = &entry.snippet {
        let snippet = truncate_utf8_bytes(snippet, SNIPPET_MAX_BYTES).value;
        rendered.push_str(" — ");
        rendered.push_str(&escape_xml_text(&snippet));
    }
    rendered.push_str(&suffix);
    rendered
}

pub fn render_pending_attention_body(
    existing_items: Vec<String>,
    include_reality_check_due: bool,
) -> RenderedPendingAttention {
    let mut items = existing_items;
    let mut omitted_count = 0;
    let mut reality_check_due_emitted = false;

    if include_reality_check_due {
        if items.len() < PENDING_ATTENTION_TOTAL_CAP {
            items.push(render_reality_check_due_item());
            reality_check_due_emitted = true;
        } else {
            omitted_count += 1;
        }
    }

    if items.len() > PENDING_ATTENTION_TOTAL_CAP {
        omitted_count += items.len().saturating_sub(PENDING_ATTENTION_TOTAL_CAP) as u64;
        items.truncate(PENDING_ATTENTION_TOTAL_CAP);
    }

    RenderedPendingAttention { body: items.join("\n"), omitted_count, reality_check_due_emitted }
}

pub(crate) fn emit_recall_hits<'a>(substrate: &Substrate, included_memory_ids: impl IntoIterator<Item = &'a str>) {
    let ids = unique_memory_ids(included_memory_ids);
    if ids.is_empty() {
        return;
    }

    for id in ids {
        let memory_id = id.to_string();
        if let Err(error) = substrate.record_recall_hit(id) {
            warn_recall_hit(format_args!("RecallHit event append failed for {memory_id}: {error}"));
        }
    }
}

pub fn render_startup_frame(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
) -> String {
    render_startup_frame_with_coordination(session_binding, explanation, sections, None)
}

pub fn render_startup_frame_with_coordination(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
    coordination: Option<&CoordinationInsertion>,
) -> String {
    render_startup_frame_with_cross_device_updates(
        session_binding,
        explanation,
        sections,
        StartupCoordinationRender { same_device: coordination, cross_device: None },
    )
}

pub fn render_startup_frame_with_cross_device_updates(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
    startup_coordination: StartupCoordinationRender<'_>,
) -> String {
    let rendered_peer_updates = startup_coordination.same_device.map(render_peer_update_elements).unwrap_or_default();
    let rendered_cross_device_updates =
        startup_coordination.cross_device.map(render_cross_device_updates).unwrap_or_default();
    let coordination_attr = if rendered_peer_updates.is_empty() && rendered_cross_device_updates.is_empty() {
        String::new()
    } else {
        format!(" coordination=\"{}\"", escape_xml_attr(COORDINATION_POLICY))
    };
    let mut frame = String::new();
    frame.push_str(&format!(
        "<memory-recall version=\"{}\" harness=\"{}\" session=\"{}\"{}>\n",
        STREAM_E_POLICY,
        escape_xml_attr(&session_binding.harness),
        escape_xml_attr(&session_binding.session_id),
        coordination_attr
    ));

    for section_name in RecallSectionName::STARTUP_ORDER {
        let body = sections
            .iter()
            .find(|section| section.name == section_name)
            .map(|section| section.body.as_str())
            .unwrap_or("");
        let opening = opening_tag(session_binding, explanation, section_name);
        let merged_body;
        let body = if section_name == RecallSectionName::EntityRecall
            && (!rendered_peer_updates.is_empty() || !rendered_cross_device_updates.is_empty())
        {
            merged_body = format!("{rendered_peer_updates}{body}{rendered_cross_device_updates}");
            merged_body.as_str()
        } else {
            body
        };
        render_section(&mut frame, section_name, &opening, body);
    }

    frame.push_str("</memory-recall>\n");
    frame
}

pub fn render_delta_frame(
    items: &[DeltaRecallItem],
    budget_tokens: usize,
    coordination: Option<&CoordinationInsertion>,
) -> RenderedDeltaFrame {
    let mut body = String::new();
    let mut used_tokens = 0usize;
    let mut included_item_ids = Vec::new();
    let mut rendered_coordination_entries = false;

    if let Some(insertion) = coordination {
        let coordination_body = render_delta_coordination_within_budget(insertion, budget_tokens, &mut used_tokens);
        rendered_coordination_entries = !coordination_body.is_empty();
        body.push_str(&coordination_body);

        if let Some(pending_attention) = render_coordination_pending_attention(insertion) {
            push_if_within_budget(&mut body, pending_attention, budget_tokens, &mut used_tokens);
        }
    }

    for item in items {
        let rendered = render_delta_item(&item.id, &item.text);
        if !push_if_within_budget(&mut body, rendered, budget_tokens, &mut used_tokens) {
            break;
        }
        included_item_ids.push(item.id.clone());
    }

    if body.is_empty() {
        return RenderedDeltaFrame {
            block: "<memory-delta empty=\"true\" />\n".to_owned(),
            budget_used_tokens: 0,
            included_item_ids,
        };
    }

    let coordination_attr = if rendered_coordination_entries {
        format!(" coordination=\"{}\"", escape_xml_attr(COORDINATION_POLICY))
    } else {
        String::new()
    };
    RenderedDeltaFrame {
        block: format!("<memory-delta{coordination_attr}>\n{body}</memory-delta>\n"),
        budget_used_tokens: used_tokens,
        included_item_ids,
    }
}

pub fn escape_xml_text(value: &str) -> String {
    escape_xml(value, false)
}

pub fn escape_xml_attr(value: &str) -> String {
    escape_xml(value, true)
}

fn render_reality_check_due_item() -> String {
    format!("<item kind=\"reality_check_due\" count=\"1\">{REALITY_CHECK_DUE_ITEM_TEXT}</item>")
}

fn render_section(frame: &mut String, section_name: RecallSectionName, opening: &str, body: &str) {
    frame.push_str("  ");
    frame.push_str(opening);
    frame.push('\n');
    render_section_body(frame, body);
    frame.push_str("  </");
    frame.push_str(section_name.as_str());
    frame.push_str(">\n");
}

fn opening_tag(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    section_name: RecallSectionName,
) -> String {
    match section_name {
        RecallSectionName::ProjectState => project_state_opening_tag(session_binding),
        RecallSectionName::EntityRecall => "<entity-recall entities=\"\">".to_owned(),
        RecallSectionName::RecallExplanation => format!(
            "<recall-explanation policy=\"{}\" budget-tokens=\"{}\" used-tokens=\"{}\">",
            escape_xml_attr(&explanation.policy),
            explanation.budget_tokens,
            explanation.budget_used_tokens
        ),
        _ => format!("<{}>", section_name.as_str()),
    }
}

fn project_state_opening_tag(session_binding: &SessionBinding) -> String {
    match &session_binding.project {
        Some(project) => {
            let display_name = project.alias.as_deref().unwrap_or(&project.canonical_id);
            format!(
                "<project-state project=\"{}\" resolved-via=\"{}\">",
                escape_xml_attr(display_name),
                escape_xml_attr(project.resolved_via.as_str())
            )
        }
        None => "<project-state>".to_owned(),
    }
}

fn render_section_body(frame: &mut String, body: &str) {
    for line in body.lines() {
        frame.push_str("    ");
        frame.push_str(line);
        frame.push('\n');
    }
}

pub(crate) fn render_delta_item(memory_id: &str, text: &str) -> String {
    format!("  <item id=\"{}\">{}</item>\n", escape_xml_attr(memory_id), escape_xml_text(text))
}

fn render_delta_coordination_within_budget(
    insertion: &CoordinationInsertion,
    budget_tokens: usize,
    used_tokens: &mut usize,
) -> String {
    let mut body = String::new();

    if !insertion.peer_presence.is_empty() {
        let rendered_presence = render_peer_presence_element(&insertion.peer_presence);
        push_if_within_budget(&mut body, rendered_presence, budget_tokens, used_tokens);
    }

    for rendered_update in insertion.peer_updates.iter().map(render_peer_update_element) {
        if !push_if_within_budget(&mut body, rendered_update, budget_tokens, used_tokens) {
            break;
        }
    }

    body
}

fn render_peer_update_elements(insertion: &CoordinationInsertion) -> String {
    insertion.peer_updates.iter().map(render_peer_update_element).collect::<Vec<_>>().join("")
}

fn render_cross_device_updates(updates: &CrossDeviceStartupUpdates) -> String {
    if updates.peer_updates.is_empty() {
        return String::new();
    }

    let mut rendered = format!("  <cross-device-updates from-sync=\"{}\">\n", escape_xml_attr(&updates.from_sync_date));
    for update in &updates.peer_updates {
        let mut update = update.clone();
        update.device = Some("other".to_owned());
        for line in render_peer_update_element(&update).lines() {
            rendered.push_str("  ");
            rendered.push_str(line);
            rendered.push('\n');
        }
    }
    rendered.push_str("  </cross-device-updates>\n");
    rendered
}

fn render_peer_update_element(entry: &PeerUpdateEntry) -> String {
    let mut attributes = format!(
        "from=\"{}\" session=\"{}\" ts=\"{}\" relevance=\"{:.2}\"",
        escape_xml_attr(&entry.harness),
        escape_xml_attr(&display_prefix(&entry.session_id, 8)),
        entry.timestamp.format("%H:%M"),
        entry.relevance.clamp(0.0, 1.0)
    );

    if let Some(lock) = &entry.claim_locked {
        attributes.push_str(&format!(
            " claim_locked=\"{}:{}\"",
            escape_xml_attr(&lock.holder_harness),
            escape_xml_attr(&lock.holder_session_id)
        ));
    }
    if let Some(device) = &entry.device {
        attributes.push_str(&format!(" device=\"{}\"", escape_xml_attr(device)));
    }

    format!(
        "  <peer-update {attributes}>\n    <summary>{}</summary>\n    <ref>{}</ref>\n    <namespace>{}</namespace>\n  </peer-update>\n",
        escape_xml_text(&safe_peer_summary(&entry.summary)),
        escape_xml_text(&entry.reference),
        escape_xml_text(&entry.namespace)
    )
}

fn render_peer_presence_element(entries: &[PeerPresenceEntry]) -> String {
    let mut rendered = String::from("  <peer-presence>\n");
    for entry in entries.iter().take(4) {
        rendered.push_str(&format!(
            "    <session harness=\"{}\" id=\"{}\" entities=\"{}\" started=\"{}\" />\n",
            escape_xml_attr(&entry.harness),
            escape_xml_attr(&display_prefix(&entry.session_id, 6)),
            escape_xml_attr(&entry.salient_entities.iter().take(5).cloned().collect::<Vec<_>>().join(",")),
            entry.started_at.format("%H:%M")
        ));
    }
    rendered.push_str("  </peer-presence>\n");
    rendered
}

fn safe_peer_summary(summary: &str) -> String {
    let classifier = DeterministicPrivacyClassifier::new();
    match safe_plaintext_fragment(&classifier, summary) {
        SafeFragmentDecision::Allow => truncate_utf8_bytes(summary, SUMMARY_MAX_BYTES).value,
        SafeFragmentDecision::OmitEncryptedBodyHidden | SafeFragmentDecision::OmitReviewPending => {
            PRIVACY_FILTERED_SUMMARY.to_owned()
        }
    }
}

fn render_coordination_pending_attention(insertion: &CoordinationInsertion) -> Option<String> {
    let count = insertion.capped_peer_updates.saturating_add(insertion.capped_peer_presence);
    (count > 0).then(|| {
        format!(
            "  <pending-attention>\n    <item kind=\"coordination_overflow\" count=\"{count}\">{count} coordination update(s) omitted by cap.</item>\n  </pending-attention>\n"
        )
    })
}

fn push_if_within_budget(body: &mut String, rendered: String, budget_tokens: usize, used_tokens: &mut usize) -> bool {
    let tokens = estimated_tokens(&rendered);
    if used_tokens.saturating_add(tokens) > budget_tokens {
        return false;
    }
    *used_tokens += tokens;
    body.push_str(&rendered);
    true
}

fn display_prefix(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn unique_memory_ids<'a>(included_memory_ids: impl IntoIterator<Item = &'a str>) -> Vec<MemoryId> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for raw_id in included_memory_ids {
        if !seen.insert(raw_id.to_owned()) {
            continue;
        }
        match MemoryId::try_new(raw_id.to_owned()) {
            Ok(id) => ids.push(id),
            Err(error) => warn_recall_hit(format_args!("RecallHit skipped invalid memory id {raw_id}: {error}")),
        }
    }
    ids
}

fn warn_recall_hit(args: std::fmt::Arguments<'_>) {
    eprintln!("WARN {args}");
}

fn escape_xml(value: &str, escape_quotes: bool) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' if escape_quotes => escaped.push_str("&quot;"),
            '\'' if escape_quotes => escaped.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            _ => escaped.push(character),
        }
    }
    escaped
}
