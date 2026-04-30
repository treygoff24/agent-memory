use crate::recall::budget::truncate_utf8_bytes;
use crate::recall::types::{RecallExplanation, RecallSectionName, SessionBinding, STREAM_E_POLICY};

const SUMMARY_MAX_BYTES: usize = 240;
const SNIPPET_MAX_BYTES: usize = 360;

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

pub fn render_startup_frame(
    session_binding: &SessionBinding,
    explanation: &RecallExplanation,
    sections: &[RenderedRecallSection],
) -> String {
    let mut frame = String::new();
    frame.push_str(&format!(
        "<memory-recall version=\"{}\" harness=\"{}\" session=\"{}\">\n",
        STREAM_E_POLICY,
        escape_xml_attr(&session_binding.harness),
        escape_xml_attr(&session_binding.session_id)
    ));

    for section_name in RecallSectionName::STARTUP_ORDER {
        let body = sections
            .iter()
            .find(|section| section.name == section_name)
            .map(|section| section.body.as_str())
            .unwrap_or("");
        let opening = opening_tag(session_binding, explanation, section_name);
        render_section(&mut frame, section_name, &opening, body);
    }

    frame.push_str("</memory-recall>\n");
    frame
}

pub fn escape_xml_text(value: &str) -> String {
    escape_xml(value, false)
}

pub fn escape_xml_attr(value: &str) -> String {
    escape_xml(value, true)
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

fn escape_xml(value: &str, escape_quotes: bool) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' if escape_quotes => escaped.push_str("&quot;"),
            '\'' if escape_quotes => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
