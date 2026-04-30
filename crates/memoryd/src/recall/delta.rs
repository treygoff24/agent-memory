use memory_substrate::{ChunkQuery, Substrate};

use crate::recall::budget::estimated_tokens;
use crate::recall::error::RecallError;
use crate::recall::render::{escape_xml_attr, escape_xml_text};
use crate::recall::types::{DeltaRequest, DeltaResponse, DEFAULT_DELTA_BUDGET_TOKENS};

pub async fn build_delta_response(substrate: &Substrate, request: DeltaRequest) -> Result<DeltaResponse, RecallError> {
    validate_delta_request(&request).await?;
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
        let rendered = render_delta_item(chunk.memory_id.as_str(), &chunk.text);
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

fn render_delta_item(memory_id: &str, text: &str) -> String {
    format!("  <item id=\"{}\">{}</item>\n", escape_xml_attr(memory_id), escape_xml_text(text))
}

async fn validate_delta_request(request: &DeltaRequest) -> Result<(), RecallError> {
    if request.message.trim().is_empty() {
        return Err(RecallError::invalid_request("message must be non-empty"));
    }
    let budget = request.budget_tokens.unwrap_or(DEFAULT_DELTA_BUDGET_TOKENS);
    if !(128..=8_000).contains(&budget) {
        return Err(RecallError::invalid_request("budget_tokens must be in 128..=8000"));
    }
    crate::recall::binding::validate_session_fields(&request.cwd, &request.session_id, &request.harness).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render_delta_item;

    #[test]
    fn delta_item_escapes_id_as_xml_attribute() {
        let rendered = render_delta_item("mem\" onclick=\"evil", "safe <text>");

        assert!(rendered.contains("id=\"mem&quot; onclick=&quot;evil\""));
        assert!(rendered.contains("safe &lt;text&gt;"));
        assert!(!rendered.contains("onclick=\"evil"));
    }
}
