use std::collections::HashSet;

use memorum_coordination::{CoordinationInsertion, PeerPresenceEntry, PeerUpdateEntry};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use memory_substrate::{MemoryId, Substrate};

use crate::recall::budget::{estimated_tokens, truncate_utf8_bytes, TOKEN_ESTIMATOR_BYTES_PER_TOKEN};
use crate::recall::types::{
    RecallExplanation, RecallSectionName, SessionBinding, HOOK_BLOCK_CHAR_CAP, STREAM_E_POLICY,
};

const SUMMARY_MAX_BYTES: usize = 240;
const SNIPPET_MAX_BYTES: usize = 360;
/// Floor for rendering a truncated delta item: below this many remaining
/// tokens the abbreviated text would be too small to carry the fact, so the
/// frame closes instead.
const DELTA_ITEM_MIN_TOKENS: usize = 48;
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
    /// Salient entity ids for the current session, used to populate the
    /// `entities=` attribute on `<entity-recall>`. Per spec §4.3, Stream E
    /// populates this attribute so that later startup-recall parsing can
    /// re-derive `SessionContext.salient_entities` from the recall block
    /// via `entity_recall_attribute_ids`. When `None` the attribute is
    /// emitted empty (legacy / non-coordination callers).
    pub salient_entities: Option<&'a HashSet<String>>,
}

pub fn render_memory_entry(entry: &RecallEntry) -> String {
    render_memory_entry_inner(entry, false)
}

/// Passive (hook-mode) memory entry. Identical to [`render_memory_entry`] except
/// the summary/snippet prose is run through [`neutralize_imperative_prose`] so an
/// imperative memory ("Always do X") injected into a session reads as a reported
/// fact, not an instruction (plan Decision 8 — "sanitize, don't just frame").
pub fn render_memory_entry_passive(entry: &RecallEntry) -> String {
    render_memory_entry_inner(entry, true)
}

fn render_memory_entry_inner(entry: &RecallEntry, passive: bool) -> String {
    let summary = truncate_utf8_bytes(&entry.summary, SUMMARY_MAX_BYTES).value;
    // An optional `<snippet>` is omitted entirely when empty or whitespace-only
    // rather than rendered as a hollow `<snippet></snippet>`: the empty tag is
    // pure scaffolding that also implies content the recall index never carried.
    // A present snippet (Tier 2 / direct callers) renders verbatim — the trim is
    // only the emptiness test, never applied to the rendered value, so meaningful
    // leading/trailing whitespace survives. Omission is a pure function of the
    // entry, so the passive block stays byte-deterministic (cache-safety invariant).
    let snippet = entry
        .snippet
        .as_deref()
        .filter(|snippet| !snippet.trim().is_empty())
        .map(|snippet| truncate_utf8_bytes(snippet, SNIPPET_MAX_BYTES).value);
    let (summary, snippet) = if passive {
        (neutralize_imperative_prose(&summary), snippet.map(|snippet| neutralize_imperative_prose(&snippet)))
    } else {
        (summary, snippet)
    };
    let snippet_element = match &snippet {
        Some(snippet) => format!("\n  <snippet>{}</snippet>", escape_xml_text(snippet)),
        None => String::new(),
    };

    format!(
        "<memory ref=\"{}\" updated=\"{}\" source=\"{}\" confidence=\"{}\">\n  <summary>{}</summary>{}\n</memory>",
        escape_xml_attr(&entry.id),
        escape_xml_attr(&entry.updated),
        escape_xml_attr(&entry.source_kind),
        escape_xml_attr(&entry.confidence),
        escape_xml_text(&summary),
        snippet_element,
    )
}

/// Neutralize imperative prose so injected memory text reads as a fact, not a
/// command. Deterministic and idempotent: a leading imperative clause is reframed
/// with a `[recalled fact]` marker that an injection detector (and the agent)
/// reads as reported content rather than a directive.
///
/// The detection is intentionally conservative — it fires only when the first
/// word of a line is a bare second-person imperative verb (or an "always/never"
/// adverb that fronts one). Lines that are already declarative are left byte-for-
/// byte unchanged so the dedup/determinism tuple stays stable.
pub fn neutralize_imperative_prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line_leads_with_imperative(line) {
            out.push_str(IMPERATIVE_FACT_MARKER);
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

const IMPERATIVE_FACT_MARKER: &str = "[recalled note]";

/// Second-person imperative verbs that, when fronting a memory line, would read
/// as an instruction to the agent. Sorted for readability; matched
/// case-insensitively against the first token of the line.
const IMPERATIVE_LEAD_WORDS: &[&str] = &[
    "always",
    "avoid",
    "call",
    "configure",
    "delete",
    "disable",
    "do",
    "don't",
    "dont",
    "enable",
    "ensure",
    "install",
    "make",
    "never",
    "prefer",
    "remember",
    "remove",
    "run",
    "set",
    "stop",
    "switch",
    "use",
];

fn line_leads_with_imperative(line: &str) -> bool {
    let trimmed = line.trim_start_matches(|c: char| c == '-' || c == '*' || c == '>' || c.is_whitespace());
    let Some(first) = trimmed.split(|c: char| c.is_whitespace()).next() else {
        return false;
    };
    if first.is_empty() {
        return false;
    }
    // Already neutralized: the marker itself starts with '[', never a lead word.
    let normalized = first.trim_end_matches([',', '.', ':', ';', '!']).to_ascii_lowercase();
    IMPERATIVE_LEAD_WORDS.contains(&normalized.as_str())
}

/// Fixed notice appended when a passive block is truncated at the char cap. It
/// closes the `<memory-recall>` root so the injected block stays well-formed.
const HOOK_TRUNCATION_NOTICE: &str = "\n  <recall-truncated reason=\"hook-char-cap\" />\n</memory-recall>\n";

/// Enforce [`HOOK_BLOCK_CHAR_CAP`] on a fully-rendered passive block.
///
/// The reduced hook budget keeps real blocks well under the cap; this is a
/// deterministic backstop for the pathological case (e.g. many max-length
/// summaries). Truncation depends only on the rendered bytes — not on iteration
/// order or the clock — so the cached prefix stays byte-stable for a given
/// identity tuple (plan Decision 4 + "Size" invariant guard).
pub fn cap_passive_block(block: String) -> String {
    if block.chars().count() <= HOOK_BLOCK_CHAR_CAP {
        return block;
    }
    // Reserve room for the closing notice so the capped result still fits.
    let budget = HOOK_BLOCK_CHAR_CAP.saturating_sub(HOOK_TRUNCATION_NOTICE.chars().count());
    let mut truncated: String = block.chars().take(budget).collect();
    // Cut back to the last newline so we never sever a tag mid-line; this is a
    // pure function of `truncated`, preserving determinism.
    if let Some(last_newline) = truncated.rfind('\n') {
        truncated.truncate(last_newline);
    }
    truncated.push_str(HOOK_TRUNCATION_NOTICE);
    truncated
}

pub fn render_pending_attention_body(
    existing_items: Vec<String>,
    include_reality_check_due: bool,
) -> RenderedPendingAttention {
    let mut seen = HashSet::new();
    let mut items = existing_items.into_iter().filter(|item| seen.insert(item.clone())).collect::<Vec<_>>();
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

    // One batch append: the sequence-state guard runs once for the whole set
    // instead of once per id. Per-id failures are returned and logged exactly as
    // the prior loop did.
    for (memory_id, error) in substrate.record_recall_hits(&ids) {
        warn_recall_hit(format_args!("RecallHit event append failed for {memory_id}: {error}"));
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
        StartupCoordinationRender { same_device: coordination, cross_device: None, salient_entities: None },
    )
}

pub fn render_startup_frame_with_cross_device_updates(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
    startup_coordination: StartupCoordinationRender<'_>,
) -> String {
    render_startup_frame_inner(session_binding, explanation, sections, startup_coordination, false)
}

/// Passive (hook-mode) startup frame. Byte-deterministic across sessions: the
/// `session="..."` attribute is omitted so the cached SessionStart prefix is keyed
/// only on the identity tuple `(memory set, cwd, MEMORY.md head, budget)` and not
/// on the per-session id (plan Decision 4 — cache safety).
///
/// Section bodies are assembled passive-side in [`crate::recall::startup`] (no
/// wall-clock / mutable-state items); this entry point only drops the
/// non-deterministic frame attribute.
pub fn render_startup_frame_passive(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
    startup_coordination: StartupCoordinationRender<'_>,
) -> String {
    render_startup_frame_inner(session_binding, explanation, sections, startup_coordination, true)
}

#[allow(clippy::too_many_arguments)]
fn render_startup_frame_inner(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
    startup_coordination: StartupCoordinationRender<'_>,
    passive: bool,
) -> String {
    let rendered_peer_updates = startup_coordination.same_device.map(render_peer_update_elements).unwrap_or_default();
    let rendered_cross_device_updates =
        startup_coordination.cross_device.map(render_cross_device_updates).unwrap_or_default();
    let coordination_attr = if rendered_peer_updates.is_empty() && rendered_cross_device_updates.is_empty() {
        String::new()
    } else {
        format!(" coordination=\"{}\"", escape_xml_attr(COORDINATION_POLICY))
    };
    // The cached SessionStart prefix must be byte-identical across sessions, so a
    // passive frame omits the per-session `session=` attribute (plan Decision 4).
    let session_attr =
        if passive { String::new() } else { format!(" session=\"{}\"", escape_xml_attr(&session_binding.session_id)) };
    let mut frame = String::new();
    frame.push_str(&format!(
        "<memory-recall version=\"{}\" harness=\"{}\"{}{}>\n",
        STREAM_E_POLICY,
        escape_xml_attr(&session_binding.harness),
        session_attr,
        coordination_attr
    ));

    for section_name in RecallSectionName::STARTUP_ORDER {
        let body = sections
            .iter()
            .find(|section| section.name == section_name)
            .map(|section| section.body.as_str())
            .unwrap_or("");
        let opening = opening_tag(session_binding, explanation, section_name, startup_coordination.salient_entities);
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
    render_delta_frame_inner(items, budget_tokens, coordination, false)
}

/// Passive (hook-mode) delta frame. Identical to [`render_delta_frame`] except
/// each item's prose is neutralized via [`neutralize_imperative_prose`] so the
/// per-turn injection reads as recalled facts rather than instructions (plan
/// Decision 8). The empty-delta sentinel is unchanged.
pub fn render_delta_frame_passive(
    items: &[DeltaRecallItem],
    budget_tokens: usize,
    coordination: Option<&CoordinationInsertion>,
) -> RenderedDeltaFrame {
    render_delta_frame_inner(items, budget_tokens, coordination, true)
}

fn render_delta_frame_inner(
    items: &[DeltaRecallItem],
    budget_tokens: usize,
    coordination: Option<&CoordinationInsertion>,
    passive: bool,
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
        let text = if passive { neutralize_imperative_prose(&item.text) } else { item.text.clone() };
        let rendered = render_delta_item(&item.id, &text);
        if push_if_within_budget(&mut body, rendered, budget_tokens, &mut used_tokens) {
            included_item_ids.push(item.id.clone());
            continue;
        }
        // The full item overflows the remaining budget. Truncate its text to
        // fit instead of dropping it: an abbreviated top-ranked fact beats an
        // empty delta — with one candidate collapsed per memory, the first
        // item is the best match, and breaking here used to serve the empty
        // sentinel whenever that memory's chunk alone exceeded the budget.
        let remaining_tokens = budget_tokens.saturating_sub(used_tokens);
        if remaining_tokens < DELTA_ITEM_MIN_TOKENS {
            break;
        }
        let scaffold_tokens = estimated_tokens(&render_delta_item(&item.id, ""));
        // Conservative text budget: escaping can expand bytes, so verify with
        // the real push below rather than trusting the arithmetic.
        let text_budget_bytes =
            remaining_tokens.saturating_sub(scaffold_tokens).saturating_mul(TOKEN_ESTIMATOR_BYTES_PER_TOKEN);
        let truncated = truncate_utf8_bytes(&text, text_budget_bytes);
        let rendered = render_delta_item(&item.id, &truncated.value);
        if push_if_within_budget(&mut body, rendered, budget_tokens, &mut used_tokens) {
            included_item_ids.push(item.id.clone());
        }
        break;
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
    salient_entities: Option<&HashSet<String>>,
) -> String {
    match section_name {
        RecallSectionName::ProjectState => project_state_opening_tag(session_binding),
        RecallSectionName::EntityRecall => entity_recall_opening_tag(salient_entities),
        RecallSectionName::RecallExplanation => format!(
            "<recall-explanation policy=\"{}\" budget-tokens=\"{}\" used-tokens=\"{}\">",
            escape_xml_attr(&explanation.policy),
            explanation.budget_tokens,
            explanation.budget_used_tokens
        ),
        _ => format!("<{}>", section_name.as_str()),
    }
}

/// Build `<entity-recall entities="...">` with a sorted, XML-escaped,
/// comma-separated list of salient entity ids.
///
/// Sorting is lexicographic on the entity id strings so that the attribute
/// value is deterministic regardless of `HashSet` iteration order — required
/// for two-clone convergence (CLAUDE.md invariant 6).
fn entity_recall_opening_tag(salient_entities: Option<&HashSet<String>>) -> String {
    let entities_attr = match salient_entities {
        None => String::new(),
        Some(entities) if entities.is_empty() => String::new(),
        Some(entities) => {
            let mut sorted: Vec<&str> = entities.iter().map(String::as_str).collect();
            sorted.sort_unstable();
            sorted.iter().map(|e| escape_xml_attr(e)).collect::<Vec<_>>().join(",")
        }
    };
    format!("<entity-recall entities=\"{entities_attr}\">")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (2026-07-19): a top-ranked item whose chunk alone exceeded
    /// the delta budget used to abort the frame and serve the empty sentinel
    /// even with more candidates queued. The renderer now truncates the item
    /// to the remaining budget instead of serving nothing.
    #[test]
    fn oversized_first_delta_item_renders_truncated_instead_of_empty() {
        let items = vec![
            DeltaRecallItem { id: "mem_big".to_owned(), text: "needle fact ".repeat(400) },
            DeltaRecallItem { id: "mem_small".to_owned(), text: "small trailing fact".to_owned() },
        ];

        let frame = render_delta_frame(&items, 400, None);

        assert_ne!(frame.block, "<memory-delta empty=\"true\" />\n", "oversized top item must not empty the frame");
        assert!(frame.block.contains("mem_big"), "top-ranked item should render truncated: {}", frame.block);
        assert!(frame.block.contains('…'), "truncated item text should end with the ellipsis marker");
        assert_eq!(frame.included_item_ids, vec!["mem_big".to_owned()]);
        assert!(
            estimated_tokens(&frame.block) <= 400 + estimated_tokens("<memory-delta>\n</memory-delta>\n"),
            "frame must stay within the budget envelope"
        );

        // Below the minimum-useful floor the frame still closes empty.
        let tiny = render_delta_frame(&items, DELTA_ITEM_MIN_TOKENS - 1, None);
        assert_eq!(tiny.block, "<memory-delta empty=\"true\" />\n");
    }

    #[test]
    fn pending_attention_body_dedupes_before_applying_cap() {
        let rendered = render_pending_attention_body(
            vec!["- duplicate item".to_string(), "- duplicate item".to_string(), "- distinct item".to_string()],
            false,
        );

        assert_eq!(rendered.body.matches("duplicate item").count(), 1);
        assert!(rendered.body.contains("distinct item"));
        assert_eq!(rendered.omitted_count, 0);
    }

    fn passive_test_binding() -> SessionBinding {
        SessionBinding {
            session_id: "sess_should_not_appear".to_owned(),
            harness: "claude-code".to_owned(),
            harness_version: Some("2.1.183".to_owned()),
            cwd: "/tmp/project".to_owned(),
            project: None,
            namespaces_in_scope: vec!["me".to_owned()],
        }
    }

    #[test]
    fn neutralize_imperative_prose_reframes_only_imperative_lines() {
        // Imperative-leading lines get the fact marker.
        assert_eq!(neutralize_imperative_prose("Always wire the hook"), "[recalled note] Always wire the hook");
        assert_eq!(neutralize_imperative_prose("Run scripts/check.sh"), "[recalled note] Run scripts/check.sh");
        assert_eq!(
            neutralize_imperative_prose("- Use merge-base for diffs"),
            "[recalled note] - Use merge-base for diffs"
        );

        // Declarative prose is left byte-for-byte unchanged (determinism).
        let declarative = "The daemon runs under launchd on this host.";
        assert_eq!(neutralize_imperative_prose(declarative), declarative);

        // Idempotent: a second pass does not stack markers.
        let once = neutralize_imperative_prose("Never commit secrets");
        assert_eq!(neutralize_imperative_prose(&once), once);
    }

    #[test]
    fn neutralize_imperative_prose_handles_each_line_independently() {
        let input = "The eval gate is green.\nAlways run the gate at the coordinator.";
        let neutralized = neutralize_imperative_prose(input);
        assert_eq!(neutralized, "The eval gate is green.\n[recalled note] Always run the gate at the coordinator.");
    }

    #[test]
    fn passive_memory_entry_sanitizes_summary_while_active_entry_is_unchanged() {
        let entry = RecallEntry {
            id: "mem_1".to_owned(),
            summary: "Always rebase before pushing".to_owned(),
            snippet: None,
            updated: "2026-06-19".to_owned(),
            source_kind: "agent_primary".to_owned(),
            confidence: "0.90".to_owned(),
        };

        let passive = render_memory_entry_passive(&entry);
        assert!(passive.contains("<summary>[recalled note] Always rebase before pushing</summary>"));

        // Non-passive rendering must be byte-for-byte the legacy output.
        let active = render_memory_entry(&entry);
        assert!(active.contains("<summary>Always rebase before pushing</summary>"));
        assert!(!active.contains("[recalled note]"));
    }

    #[test]
    fn render_memory_entry_omits_empty_snippet_and_renders_present_one() {
        let base = RecallEntry {
            id: "mem_snip".to_owned(),
            summary: "A standalone fact.".to_owned(),
            snippet: None,
            updated: "2026-06-21".to_owned(),
            source_kind: "agent_primary".to_owned(),
            confidence: "0.70".to_owned(),
        };

        // None / empty / whitespace-only snippet → no <snippet> element at all.
        for empty in [None, Some(String::new()), Some("   ".to_owned())] {
            let entry = RecallEntry { snippet: empty, ..base.clone() };
            let rendered = render_memory_entry(&entry);
            assert!(!rendered.contains("<snippet"), "empty snippet must not render an element: {rendered}");
            assert!(rendered.ends_with("<summary>A standalone fact.</summary>\n</memory>"));
        }

        // A present snippet renders verbatim — leading/trailing whitespace is
        // preserved (trim is only the emptiness test, never applied to the value).
        let entry = RecallEntry { snippet: Some("  padded body  ".to_owned()), ..base };
        let rendered = render_memory_entry(&entry);
        assert!(
            rendered.contains("<summary>A standalone fact.</summary>\n  <snippet>  padded body  </snippet>\n</memory>")
        );
    }

    #[test]
    fn passive_startup_frame_omits_session_attribute() {
        let binding = passive_test_binding();
        let explanation = RecallExplanation::empty(1_900);

        let passive = render_startup_frame_passive(&binding, &explanation, &[], StartupCoordinationRender::default());
        let active = render_startup_frame_with_cross_device_updates(
            &binding,
            &explanation,
            &[],
            StartupCoordinationRender::default(),
        );

        assert!(passive.starts_with("<memory-recall version=\"stream-e-v0.7\" harness=\"claude-code\">"));
        assert!(!passive.contains("session=\""), "passive frame must not carry the session attribute");
        assert!(!passive.contains("sess_should_not_appear"));

        // The non-passive frame is unchanged: it still embeds the session id.
        assert!(active.contains("session=\"sess_should_not_appear\""));
    }

    #[test]
    fn cap_passive_block_truncates_deterministically_below_the_cap() {
        let under = "<memory-recall>\n  <line/>\n</memory-recall>\n".to_owned();
        assert_eq!(cap_passive_block(under.clone()), under, "blocks under the cap pass through untouched");

        let mut oversized = String::from("<memory-recall>\n");
        for index in 0..2_000 {
            oversized.push_str(&format!("  <line n=\"{index}\">{}</line>\n", "y".repeat(20)));
        }
        oversized.push_str("</memory-recall>\n");
        assert!(oversized.chars().count() > HOOK_BLOCK_CHAR_CAP);

        let capped = cap_passive_block(oversized.clone());
        assert!(capped.chars().count() <= HOOK_BLOCK_CHAR_CAP, "capped block must fit under the cap");
        assert!(capped.ends_with(HOOK_TRUNCATION_NOTICE));
        // Deterministic: capping the same input twice yields the same bytes.
        assert_eq!(cap_passive_block(oversized).as_bytes(), capped.as_bytes());
    }

    #[test]
    fn passive_delta_frame_sanitizes_items_and_preserves_empty_sentinel() {
        let items = vec![DeltaRecallItem { id: "mem_d1".to_owned(), text: "Run the migration first".to_owned() }];
        let passive = render_delta_frame_passive(&items, 400, None);
        assert!(passive.block.contains("[recalled note] Run the migration first"));

        // Empty item set still emits the exact empty-delta sentinel.
        let empty = render_delta_frame_passive(&[], 400, None);
        assert_eq!(empty.block, "<memory-delta empty=\"true\" />\n");
        assert_eq!(empty.budget_used_tokens, 0);
    }
}
