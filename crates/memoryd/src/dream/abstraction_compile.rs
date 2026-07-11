//! Dream job that compiles abstractions/cues through harness CLIs.

use std::sync::Arc;
use std::time::Duration;

use memory_privacy::{PrivacyNamespace, PrivacyStorageAction};
use memory_substrate::{Memory, Roots, Scope, Substrate};
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
    pub new_id: Option<String>,
    pub source: &'static str,
    pub outcome: String,
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
    let failures = super::merge::reconcile_applying(&substrate).await;
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
        exclusion_policy: "metadata-only and encrypted memories are not eligible for abstraction_compile",
        items: Vec::with_capacity(ids.len()),
    };
    for id in ids {
        let memory = match substrate.read_memory(&id).await {
            Ok(memory) => memory,
            Err(error) => {
                report.skipped += 1;
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source: "read",
                    outcome: error.to_string(),
                });
                continue;
            }
        };
        let (source, generated) = match &harness {
            Some(harness) => ("harness", generate_with_harness(harness, &memory).await),
            None => {
                report.structural += 1;
                ("structural", Ok(structural_output(&memory)))
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
                    outcome: error,
                });
                continue;
            }
        };
        let (abstraction, cues) = match generation_privacy_rebind(&memory, generated) {
            Ok(fields) => fields,
            Err(error) => {
                report.skipped += 1;
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source,
                    outcome: error.to_string(),
                });
                continue;
            }
        };
        let response = crate::handlers::governance::governance_supersede_response(
            &substrate,
            None,
            crate::handlers::governance::GovernanceSupersedeRequest {
                old_id: id.as_str().to_string(),
                content: memory.body.clone(),
                reason: "abstraction_compile".to_string(),
                preserve_frontmatter: true,
                meta: generation_meta(&memory, abstraction, cues, source),
            },
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                report.skipped += 1;
                report.items.push(AbstractionCompileItem {
                    old_id: id.as_str().to_string(),
                    new_id: None,
                    source,
                    outcome: error.message,
                });
                continue;
            }
        };
        let crate::protocol::ResponsePayload::GovernanceSupersede(response) = response else {
            unreachable!("supersede handler response variant")
        };
        let applied = matches!(response.status, crate::protocol::GovernanceStatus::Promoted);
        report.applied += usize::from(applied);
        report.skipped += usize::from(!applied);
        report.items.push(AbstractionCompileItem {
            old_id: id.as_str().to_string(),
            new_id: response.new_id,
            source,
            outcome: format!("{:?}", response.status).to_lowercase(),
        });
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

async fn generate_with_harness(harness: &Arc<dyn HarnessCli>, memory: &Memory) -> Result<HarnessOutput, String> {
    let prompt = format!(
        "Return only JSON {{\"abstraction\":string,\"cues\":[string]}}. Abstraction: at most 8 words. Cues: 0-3 phrases, each 2-4 words, pattern [Main Entity] + [Key Aspect].\nSummary: {}\nBody:\n{}",
        memory.frontmatter.summary, memory.body
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

fn generation_privacy_rebind(memory: &Memory, output: HarnessOutput) -> anyhow::Result<(Option<String>, Vec<String>)> {
    // Probe the strictness contributed by the generated fields under the lowest
    // privacy floor (Agent / Internal / Plaintext). The memory's real namespace
    // determines the final persisted classification, but the Agent probe lets us
    // detect whether the abstraction/cues are what made the combined payload
    // stricter than the body alone.
    let body_text = format!("{}\n{}", memory.frontmatter.summary, memory.body);
    let combined = format!("{body_text}\n{}\n{}", output.abstraction, output.cues.join("\n"));
    let body = crate::handlers::governance::classify_privacy(&body_text, PrivacyNamespace::Agent, None)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let combined = crate::handlers::governance::classify_privacy(&combined, PrivacyNamespace::Agent, None)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    if combined.storage_action.refuses_storage() || body.storage_action.refuses_storage() {
        anyhow::bail!("secret refused before disk effects");
    }
    if matches!(combined.storage_action, PrivacyStorageAction::EncryptAtRest)
        && matches!(body.storage_action, PrivacyStorageAction::Plaintext)
    {
        return Ok((None, Vec::new()));
    }
    Ok((Some(output.abstraction), output.cues))
}

fn generation_meta(memory: &Memory, abstraction: Option<String>, cues: Vec<String>, source: &str) -> serde_json::Value {
    let namespace = match memory.frontmatter.scope {
        Scope::User => "me",
        Scope::Project | Scope::Org => "project",
        Scope::Agent | Scope::Subagent => "agent",
    };
    serde_json::json!({
        "namespace": namespace,
        "canonical_namespace_id": memory.frontmatter.canonical_namespace_id,
        "type": memory.frontmatter.memory_type.as_db_str(),
        "summary": memory.frontmatter.summary,
        "confidence": memory.frontmatter.confidence,
        "source_kind": "agent_primary",
        "harness": format!("abstraction_compile:{source}"),
        "abstraction": abstraction,
        "cues": cues,
    })
}

#[cfg(test)]
mod tests {
    use super::{generation_privacy_rebind, structural_output, HarnessOutput};

    #[test]
    fn sensitive_generated_fields_drop_and_rebind_to_public_body() {
        let memory = memory("Public summary", "Public deployment procedure");
        let (abstraction, cues) = generation_privacy_rebind(
            &memory,
            HarnessOutput {
                abstraction: "Contact reviewer@example.com".to_string(),
                cues: vec!["Review contact".to_string()],
            },
        )
        .expect("body-only rebind");
        assert_eq!(abstraction, None);
        assert!(cues.is_empty());
    }

    #[test]
    fn user_scoped_sensitive_cue_drops_generated_fields_and_keeps_body() {
        let memory = memory_with_scope("user", "Public summary", "Public deployment procedure");
        let (abstraction, cues) = generation_privacy_rebind(
            &memory,
            HarnessOutput {
                abstraction: "Contact reviewer@example.com".to_string(),
                cues: vec!["Review contact".to_string()],
            },
        )
        .expect("body-only rebind");
        assert_eq!(abstraction, None);
        assert!(cues.is_empty());
    }

    #[test]
    fn user_scoped_benign_cue_keeps_generated_fields() {
        let memory = memory_with_scope("user", "Public summary", "Public deployment procedure");
        let (abstraction, cues) = generation_privacy_rebind(
            &memory,
            HarnessOutput { abstraction: "Public deployment".to_string(), cues: vec!["Public procedure".to_string()] },
        )
        .expect("fields stay");
        assert_eq!(abstraction.as_deref(), Some("Public deployment"));
        assert_eq!(cues, vec!["Public procedure".to_string()]);
    }

    #[test]
    fn user_scoped_secret_cue_refuses_before_disk() {
        let memory = memory_with_scope("user", "Public summary", "Public deployment procedure");
        let result = generation_privacy_rebind(
            &memory,
            HarnessOutput { abstraction: "Card 4111111111111111".to_string(), cues: vec!["Secret card".to_string()] },
        );
        assert!(result.is_err(), "secret generated content must be refused before any disk effect");
    }

    #[tokio::test]
    async fn sensitive_generated_fields_persist_body_only_without_aux_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = memory_substrate::Substrate::init(
            memory_substrate::Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
            memory_substrate::InitOptions {
                force_unsafe_durability: true,
                device_id: Some("dev_abstractioncompile".to_string()),
            },
        )
        .await
        .expect("substrate");
        let mut memory = memory("Public summary", "Public deployment procedure");
        memory.path = Some(memory_substrate::RepoPath::new("agent/patterns/abstraction-compile.md"));
        let (abstraction, cues) = generation_privacy_rebind(
            &memory,
            HarnessOutput {
                abstraction: "Contact reviewer@example.com".to_string(),
                cues: vec!["Review contact".to_string()],
            },
        )
        .expect("body-only rebind");
        memory.frontmatter.abstraction = abstraction;
        memory.frontmatter.cues = cues;
        memory.frontmatter.sensitivity = memory_substrate::Sensitivity::Internal;
        let id = memory.frontmatter.id.clone();
        substrate
            .write_memory(memory_substrate::WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: memory_substrate::WriteMode::CreateNew,
                index_projection: None,
                event_context: memory_substrate::EventContext::default(),
                allow_best_effort_durability: true,
                classification: memory_substrate::ClassificationOutcome::Trusted,
            })
            .await
            .expect("body-only write");
        let persisted = substrate.read_memory(&id).await.expect("persisted body");
        assert_eq!(persisted.body, "Public deployment procedure");
        assert_eq!(persisted.frontmatter.abstraction, None);
        assert!(persisted.frontmatter.cues.is_empty());
        let counts = substrate
            .embedding_row_kind_counts(memory_substrate::EmbeddingLaneEligibility::AllTiers)
            .expect("embedding counts");
        assert_eq!(counts["abstraction_indexed"], 0);
        assert_eq!(counts["cue_indexed"], 0);
        assert_eq!(counts["abstraction_pending"], 0);
        assert_eq!(counts["cue_pending"], 0);
    }

    #[test]
    fn structural_fallback_truncates_summary_to_eight_words() {
        let memory = memory("one two three four five six seven eight nine ten", "body");
        let output = structural_output(&memory);
        assert_eq!(output.abstraction, "one two three four five six seven eight");
        assert!(output.cues.is_empty());
    }

    fn memory(summary: &str, body: &str) -> memory_substrate::Memory {
        memory_with_scope_and_id("agent", summary, body, "mem_20260710_aaaaaaaaaaaaaaaa_000001")
    }

    fn memory_with_scope(scope: &str, summary: &str, body: &str) -> memory_substrate::Memory {
        memory_with_scope_and_id(scope, summary, body, "mem_20260710_aaaaaaaaaaaaaaaa_000002")
    }

    fn memory_with_scope_and_id(scope: &str, summary: &str, body: &str, id: &str) -> memory_substrate::Memory {
        memory_substrate::frontmatter::parse_document(
            &format!(
                "---\nschema_version: 1\nid: {id}\ntype: pattern\nscope: {scope}\nsummary: {summary}\nconfidence: 0.9\ntrust_level: trusted\nsensitivity: public\nstatus: active\ncreated_at: 2026-07-10T00:00:00Z\nupdated_at: 2026-07-10T00:00:00Z\nauthor:\n  kind: system\n  component: test\n---\n{body}"
            ),
            None,
        )
        .expect("fixture")
        .memory
    }
}
