use memory_substrate::{MemoryStatus, RecallIndexQuery, Substrate, SubstrateError};

use crate::recall::budget::estimated_tokens;
use crate::recall::candidates::{collect_recall_candidates_from_index, RecallCollectionRequest};
use crate::recall::error::RecallError;
use crate::recall::rank::{select_ranked_candidates, RankingContext};
use crate::recall::render::{
    escape_xml_text, render_memory_entry, render_startup_frame, RecallEntry, RenderedRecallSection,
};
use crate::recall::types::{
    bounded_omissions, RecallExplanation, RecallSectionExplanation, RecallSectionName, SessionBinding, StartupRequest,
    StartupResponse, DEFAULT_STARTUP_BUDGET_TOKENS,
};
use crate::recall::validate_startup_request;

pub async fn build_startup_response(
    substrate: &Substrate,
    request: StartupRequest,
) -> Result<StartupResponse, RecallError> {
    let budget_tokens = request.budget_tokens.unwrap_or(DEFAULT_STARTUP_BUDGET_TOKENS);
    let include_recent = request.include_recent;
    let since_event_id = request.since_event_id.clone();
    let session_binding = validate_startup_request(request)?;

    if since_event_id.as_ref().is_some_and(|value| !value.trim().is_empty()) {
        return Err(RecallError::not_implemented("event-based startup deltas are not implemented in Stream E v0.5"));
    }

    let collection = collect_recall_candidates_from_index(
        substrate,
        RecallCollectionRequest {
            section: RecallSectionName::RecentMemory,
            namespace_prefixes: session_binding.namespaces_in_scope.clone(),
            updated_since: None,
        },
    )
    .await
    .map_err(map_substrate_error)?;
    let candidate_attention_count = count_candidate_attention(substrate, &session_binding.namespaces_in_scope).await?;

    let project_namespace = session_binding.project.as_ref().map(|project| project.canonical_id.clone());
    let ranking_now = collection.facts.iter().map(|candidate| candidate.row.updated_at).max().unwrap_or_default();
    let selected = select_ranked_candidates(
        RecallSectionName::RecentMemory,
        collection.facts,
        RankingContext { now: ranking_now, exact_project_namespace: project_namespace },
        budget_tokens.saturating_sub(128).max(1),
    );

    let recent_body = if include_recent {
        selected
            .selected
            .iter()
            .map(|candidate| {
                render_memory_entry(&RecallEntry {
                    id: candidate.id.clone(),
                    summary: candidate.candidate.row.summary.clone(),
                    snippet: None,
                    updated: candidate.candidate.row.updated_at.to_rfc3339(),
                    source_kind: candidate.candidate.row.source_kind.to_string(),
                    confidence: format!("{:.2}", candidate.candidate.row.confidence),
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let pending_attention_count = collection.pending_attention_count + candidate_attention_count;
    let pending_attention_body = if pending_attention_count == 0 {
        String::new()
    } else {
        format!("- {pending_attention_count} memory item(s) require review before factual recall.")
    };

    let sections = vec![
        RenderedRecallSection { name: RecallSectionName::Identity, body: identity_body(&session_binding) },
        RenderedRecallSection { name: RecallSectionName::ProjectState, body: project_body(&session_binding) },
        RenderedRecallSection { name: RecallSectionName::EntityRecall, body: String::new() },
        RenderedRecallSection { name: RecallSectionName::RecentMemory, body: recent_body },
        RenderedRecallSection { name: RecallSectionName::PendingAttention, body: pending_attention_body },
        RenderedRecallSection {
            name: RecallSectionName::RecallExplanation,
            body: "Deterministic passive recall from Stream A index rows.".to_owned(),
        },
    ];

    let section_token_estimates = section_token_estimates(&sections);
    let mut omissions = collection.omitted;
    omissions.extend(selected.omitted);
    let bounded = bounded_omissions(omissions);
    let mut explanation = RecallExplanation {
        budget_tokens,
        budget_used_tokens: 0,
        policy: crate::recall::STREAM_E_POLICY.to_owned(),
        sections: section_explanations(
            &section_token_estimates,
            selected.selected.iter().map(|candidate| candidate.id.clone()).collect(),
            bounded.omitted.len() as u32 + bounded.omitted_truncated_count,
        ),
        omitted: bounded.omitted,
        omitted_truncated_count: bounded.omitted_truncated_count,
    };
    let recall_block = render_startup_frame_with_stable_budget(&session_binding, &mut explanation, &sections);

    Ok(StartupResponse {
        session_binding,
        recall_block,
        budget_used_tokens: explanation.budget_used_tokens,
        recall_explanation: explanation,
        guidance: "Stream E passive recall assembled from read-only Stream A index projections.".to_owned(),
    })
}

async fn count_candidate_attention(substrate: &Substrate, namespace_prefixes: &[String]) -> Result<usize, RecallError> {
    let mut total = 0usize;
    for namespace_prefix in namespace_prefixes {
        let rows = substrate
            .query_recall_index(RecallIndexQuery {
                namespace_prefix: Some(namespace_prefix.clone()),
                statuses: vec![MemoryStatus::Candidate, MemoryStatus::Quarantined],
                passive_recall_only: true,
                updated_since: None,
                match_terms: Vec::new(),
            })
            .await
            .map_err(map_substrate_error)?;
        total += rows.len();
    }
    Ok(total)
}

fn identity_body(session_binding: &SessionBinding) -> String {
    format!(
        "- harness: {}\n- session: {}\n- cwd: {}",
        escape_xml_text(&session_binding.harness),
        escape_xml_text(&session_binding.session_id),
        escape_xml_text(&session_binding.cwd)
    )
}

fn project_body(session_binding: &SessionBinding) -> String {
    match &session_binding.project {
        Some(project) => {
            let display = project.alias.as_deref().unwrap_or(&project.canonical_id);
            format!(
                "- project: {}\n- namespace: project:{}",
                escape_xml_text(display),
                escape_xml_text(&project.canonical_id)
            )
        }
        None => "- project: none".to_owned(),
    }
}

fn render_startup_frame_with_stable_budget(
    session_binding: &SessionBinding,
    explanation: &mut RecallExplanation,
    sections: &[RenderedRecallSection],
) -> String {
    for _ in 0..4 {
        let recall_block = render_startup_frame(session_binding, explanation, sections);
        let measured = estimated_tokens(&recall_block);
        if explanation.budget_used_tokens == measured {
            return recall_block;
        }
        explanation.budget_used_tokens = measured;
    }
    let recall_block = render_startup_frame(session_binding, explanation, sections);
    explanation.budget_used_tokens = estimated_tokens(&recall_block);
    render_startup_frame(session_binding, explanation, sections)
}

fn section_token_estimates(sections: &[RenderedRecallSection]) -> Vec<(RecallSectionName, usize)> {
    sections.iter().map(|section| (section.name, estimated_tokens(&section.body))).collect()
}

fn section_explanations(
    section_token_estimates: &[(RecallSectionName, usize)],
    recent_selected_ids: Vec<String>,
    recent_omitted_count: u32,
) -> Vec<RecallSectionExplanation> {
    RecallSectionName::STARTUP_ORDER
        .into_iter()
        .map(|name| {
            let selected_ids =
                if name == RecallSectionName::RecentMemory { recent_selected_ids.clone() } else { Vec::new() };
            let omitted_count = if name == RecallSectionName::RecentMemory { recent_omitted_count } else { 0 };
            RecallSectionExplanation {
                name,
                selected_ids,
                matched_entities: Vec::new(),
                budget_used_tokens: section_token_estimates
                    .iter()
                    .find_map(|(section, tokens)| (*section == name).then_some(*tokens))
                    .unwrap_or(0),
                omitted_count,
            }
        })
        .collect()
}

fn map_substrate_error(error: SubstrateError) -> RecallError {
    match error {
        SubstrateError::InvalidQuery { message, .. } => RecallError::invalid_request(message),
        other => RecallError::substrate_error(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::types::{ProjectBinding, ProjectBindingSource};

    #[test]
    fn startup_frame_budget_tokens_converge_at_digit_boundary() {
        let session_binding = SessionBinding {
            session_id: "sess".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            cwd: "/tmp".to_owned(),
            project: None,
            namespaces_in_scope: vec!["me".to_owned()],
        };
        let sections = RecallSectionName::STARTUP_ORDER
            .into_iter()
            .map(|name| RenderedRecallSection {
                name,
                body: if name == RecallSectionName::RecentMemory { "x".repeat(3_850) } else { String::new() },
            })
            .collect::<Vec<_>>();
        let mut explanation = RecallExplanation {
            budget_tokens: 3_600,
            budget_used_tokens: 0,
            policy: crate::recall::STREAM_E_POLICY.to_owned(),
            sections: Vec::new(),
            omitted: Vec::new(),
            omitted_truncated_count: 0,
        };

        let recall_block = render_startup_frame_with_stable_budget(&session_binding, &mut explanation, &sections);

        assert_eq!(explanation.budget_used_tokens, estimated_tokens(&recall_block));
        assert!(recall_block.contains(&format!("used-tokens=\"{}\"", explanation.budget_used_tokens)));
    }

    #[test]
    fn identity_and_project_bodies_escape_xml_element_content() {
        let binding = SessionBinding {
            session_id: "sess</memory-recall><script>".to_owned(),
            harness: "codex&evil".to_owned(),
            harness_version: None,
            cwd: "/tmp/<cwd>".to_owned(),
            project: Some(ProjectBinding {
                canonical_id: "proj&agent".to_owned(),
                alias: Some("alias</project-state>".to_owned()),
                resolved_via: ProjectBindingSource::YamlOverride,
            }),
            namespaces_in_scope: Vec::new(),
        };

        let rendered = format!("{}\n{}", identity_body(&binding), project_body(&binding));

        assert!(rendered.contains("codex&amp;evil"));
        assert!(rendered.contains("sess&lt;/memory-recall&gt;&lt;script&gt;"));
        assert!(rendered.contains("/tmp/&lt;cwd&gt;"));
        assert!(rendered.contains("alias&lt;/project-state&gt;"));
        assert!(rendered.contains("proj&amp;agent"));
        assert!(!rendered.contains("</memory-recall><script>"));
    }
}
