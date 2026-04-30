use memory_substrate::{ChunkQuery, Substrate};

use crate::recall::budget::estimated_tokens;
use crate::recall::error::RecallError;
use crate::recall::render::escape_xml_text;
use crate::recall::types::{DeltaRequest, DeltaResponse};

const DEFAULT_DELTA_BUDGET_TOKENS: usize = 400;

pub async fn build_delta_response(substrate: &Substrate, request: DeltaRequest) -> Result<DeltaResponse, RecallError> {
    validate_delta_request(&request)?;
    let budget_tokens = request.budget_tokens.unwrap_or(DEFAULT_DELTA_BUDGET_TOKENS);
    let message = request.message.trim();
    let chunks = substrate
        .query_chunks(ChunkQuery { text: Some(message.to_owned()), triple: None, vector: None })
        .await
        .map_err(|error| RecallError::substrate_error(error.to_string()))?;

    if chunks.is_empty() {
        return Ok(DeltaResponse {
            delta_block: "<memory-delta empty=\"true\" />\n".to_owned(),
            budget_used_tokens: 0,
            guidance: "No passive recall delta matched this turn.".to_owned(),
        });
    }

    let mut body = String::from("<memory-delta>\n");
    let mut used = 0usize;
    for chunk in chunks {
        let rendered = format!(
            "  <item id=\"{}\">{}</item>\n",
            escape_xml_text(chunk.memory_id.as_str()),
            escape_xml_text(&chunk.text)
        );
        let tokens = estimated_tokens(&rendered);
        if used + tokens > budget_tokens {
            break;
        }
        used += tokens;
        body.push_str(&rendered);
    }
    body.push_str("</memory-delta>\n");

    Ok(DeltaResponse {
        delta_block: body,
        budget_used_tokens: used,
        guidance: "Stream E passive recall delta assembled through daemon protocol.".to_owned(),
    })
}

fn validate_delta_request(request: &DeltaRequest) -> Result<(), RecallError> {
    if request.message.trim().is_empty() {
        return Err(RecallError::invalid_request("message must be non-empty"));
    }
    let budget = request.budget_tokens.unwrap_or(DEFAULT_DELTA_BUDGET_TOKENS);
    if !(128..=8_000).contains(&budget) {
        return Err(RecallError::invalid_request("budget_tokens must be in 128..=8000"));
    }
    crate::recall::validate_startup_request(crate::recall::StartupRequest {
        cwd: request.cwd.clone(),
        session_id: request.session_id.clone(),
        harness: request.harness.clone(),
        harness_version: None,
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(budget.max(512)),
    })?;
    Ok(())
}
