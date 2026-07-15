//! Dream job that compiles abstractions/cues through harness CLIs.

use std::sync::Arc;
use std::time::Duration;

use memory_substrate::{Memory, MemoryContent, ReadError, Roots, Substrate};
use serde::{Deserialize, Serialize};

use super::harness::HarnessCli;

#[derive(Debug, Serialize)]
pub struct AbstractionCompileReport {
    pub selected: usize,
    pub applied: usize,
    pub skipped: usize,
    pub structural: usize,
    pub exclusion_policy: &'static str,
    pub items: Vec<AbstractionCompileItem>,
}

#[derive(Debug, Serialize)]
pub struct AbstractionCompileItem {
    pub old_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_id: Option<String>,
    pub source: &'static str,
    pub outcome: AbstractionCompileOutcome,
    /// Closed refusal/validation reason from CLI contract §8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbstractionCompileOutcome {
    Amended,
    Unchanged,
    Refused,
    ValidationSkipped,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessOutput {
    abstraction: String,
    #[serde(default)]
    cues: Vec<String>,
}

pub async fn run(args: crate::cli::DreamAbstractionCompileArgs) -> anyhow::Result<AbstractionCompileReport> {
    let substrate = Substrate::open(Roots::new(args.repo, args.runtime)).await?;
    let failures = super::merge::reconcile_applying(&substrate, None).await;
    if !failures.is_empty() {
        anyhow::bail!("merge reconciliation failed before dream entry: {failures:?}");
    }
    let ids = substrate.abstraction_compile_candidates(args.limit)?;
    let harness = available_harness(args.cli_override.as_deref()).await;
    let mut report = AbstractionCompileReport {
        selected: ids.len(),
        applied: 0,
        skipped: 0,
        structural: 0,
        exclusion_policy: "active and pinned canonical memories only",
        items: Vec::with_capacity(ids.len()),
    };
    for id in ids {
        let (envelope, expected_base_hash) = match substrate.read_memory_envelope_with_hash(&id).await {
            Ok(read) => read,
            Err(ReadError::NotFound(_)) => {
                report.skipped += 1;
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source: "read",
                    outcome: AbstractionCompileOutcome::Refused,
                    reason: Some("metadata_amendment_missing_id".to_string()),
                });
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let memory = &envelope.metadata;
        let plaintext_body = match &envelope.content {
            MemoryContent::Plaintext(body) => Some(body.as_str()),
            MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
        };
        let (source, generated) = match &harness {
            Some(harness) => ("harness", generate_with_harness(harness, memory, plaintext_body).await),
            None => {
                report.structural += 1;
                ("structural", Ok(structural_output(memory)))
            }
        };
        let generated = match generated.and_then(validate_output) {
            Ok(output) => output,
            Err(error) => {
                tracing::warn!(memory_id = %id, %error, "abstraction_compile skipped malformed output");
                report.skipped += 1;
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source,
                    outcome: AbstractionCompileOutcome::ValidationSkipped,
                    reason: Some("validation_failed".to_string()),
                });
                continue;
            }
        };
        let result = crate::handlers::governance::metadata_amend(
            &substrate,
            "memoryd-abstraction-compile",
            crate::handlers::governance::MetadataAmendRequest {
                id: id.as_str().to_string(),
                expected_base_hash,
                abstraction: Some(generated.abstraction),
                cues: generated.cues,
            },
        )
        .await;
        match result {
            Ok(outcome) => {
                let item_outcome = if outcome.changed {
                    report.applied += 1;
                    AbstractionCompileOutcome::Amended
                } else {
                    report.skipped += 1;
                    AbstractionCompileOutcome::Unchanged
                };
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source,
                    outcome: item_outcome,
                    reason: None,
                });
            }
            Err(error) if error.refusal_reason().is_some() => {
                report.skipped += 1;
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source,
                    outcome: AbstractionCompileOutcome::Refused,
                    reason: error.refusal_reason().map(str::to_string),
                });
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(report)
}

async fn available_harness(override_name: Option<&str>) -> Option<Arc<dyn HarnessCli>> {
    let registry = super::registry::HarnessCliRegistry::builtin_v0_2();
    if let Some(name) = override_name {
        let harness = registry.get(name)?;
        return (harness.is_installed() && harness.auth_probe().await.is_ok()).then_some(harness);
    }
    registry.select_first_available(&["claude".to_string(), "codex".to_string()]).await
}

async fn generate_with_harness(
    harness: &Arc<dyn HarnessCli>,
    memory: &Memory,
    plaintext_body: Option<&str>,
) -> Result<HarnessOutput, String> {
    let body = plaintext_body.unwrap_or("[encrypted body unavailable]");
    let prompt = format!(
        "Return only JSON {{\"abstraction\":string,\"cues\":[string]}}. Abstraction: at most 8 words. Cues: 0-3 phrases, each 2-4 words, pattern [Main Entity] + [Key Aspect].\nSummary: {}\nBody:\n{}",
        memory.frontmatter.summary, body
    );
    let raw = harness.complete(&prompt, true, Duration::from_secs(60)).await.map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| format!("malformed abstraction_compile JSON: {error}"))
}

fn structural_output(memory: &Memory) -> HarnessOutput {
    HarnessOutput {
        abstraction: memory.frontmatter.summary.split_whitespace().take(8).collect::<Vec<_>>().join(" "),
        cues: Vec::new(),
    }
}

fn validate_output(mut output: HarnessOutput) -> Result<HarnessOutput, String> {
    output.abstraction = memory_substrate::frontmatter::normalize_abstraction_value(Some(output.abstraction))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "empty abstraction".to_string())?;
    output.cues =
        memory_substrate::frontmatter::normalize_cue_values(output.cues).map_err(|error| error.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{structural_output, AbstractionCompileItem, AbstractionCompileOutcome};

    #[test]
    fn structural_fallback_truncates_summary_to_eight_words() {
        let memory = memory("one two three four five six seven eight nine ten", "body");
        let output = structural_output(&memory);
        assert_eq!(output.abstraction, "one two three four five six seven eight");
        assert!(output.cues.is_empty());
    }

    #[test]
    fn report_rows_serialize_to_cli_contract_vocabulary() {
        let rows = [
            (AbstractionCompileOutcome::Amended, None),
            (AbstractionCompileOutcome::Unchanged, None),
            (AbstractionCompileOutcome::Refused, Some("metadata_amendment_stale_base".to_string())),
            (AbstractionCompileOutcome::Refused, Some("metadata_amendment_tier_increase_refused".to_string())),
            (AbstractionCompileOutcome::Refused, Some("metadata_amendment_validation_failed".to_string())),
            (AbstractionCompileOutcome::Refused, Some("metadata_amendment_missing_id".to_string())),
            (AbstractionCompileOutcome::Refused, Some("metadata_amendment_actor_mismatch".to_string())),
            (AbstractionCompileOutcome::Refused, Some("secret_refused".to_string())),
            (AbstractionCompileOutcome::Refused, Some("metadata_amendment_lifecycle_not_amendable".to_string())),
            (AbstractionCompileOutcome::ValidationSkipped, Some("validation_failed".to_string())),
        ];
        let encoded = rows
            .into_iter()
            .map(|(outcome, reason)| {
                serde_json::to_value(AbstractionCompileItem {
                    old_id: "mem_20260710_aaaaaaaaaaaaaaaa_000001".to_string(),
                    new_id: None,
                    source: "structural",
                    outcome,
                    reason,
                })
                .expect("serialize row")
            })
            .collect::<Vec<_>>();
        assert_eq!(encoded[0]["outcome"], "amended");
        assert_eq!(encoded[1]["outcome"], "unchanged");
        assert!(encoded[2..9].iter().all(|row| row["outcome"] == "refused"));
        assert_eq!(encoded[9]["outcome"], "validation_skipped");
        assert!(encoded[..2].iter().all(|row| row.get("reason").is_none()));
        assert!(encoded.iter().all(|row| row.get("new_id").is_none()));
    }

    fn memory(summary: &str, body: &str) -> memory_substrate::Memory {
        let id = "mem_20260710_aaaaaaaaaaaaaaaa_000001";
        memory_substrate::frontmatter::parse_document(
            &format!(
                "---\nschema_version: 1\nid: {id}\ntype: pattern\nscope: agent\nsummary: {summary}\nconfidence: 0.9\ntrust_level: trusted\nsensitivity: internal\nstatus: active\ncreated_at: 2026-07-10T00:00:00Z\nupdated_at: 2026-07-10T00:00:00Z\nauthor:\n  kind: system\n  component: test\n---\n{body}"
            ),
            None,
        )
        .expect("fixture")
        .memory
    }
}
