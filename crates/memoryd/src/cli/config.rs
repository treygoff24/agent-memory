//! Device-local and repository config ceremonies. Embedding-lane changes need
//! a daemon restart; harvest changes are picked up on the next scheduler wake.

use std::io::IsTerminal;

use anyhow::Context;
use memory_substrate::{EmbeddingTriple, Roots, Substrate};
use serde::Serialize;

use crate::cli::{ConfigArgs, ConfigCommand, EmbeddingLane, EmbeddingLaneArgs, HarvestArgs, HarvestCommand};

/// Gemini embedding price per the ratified plan's scout table ($0.20/M standard
/// tier; batch is $0.10/M). Verify against the live price sheet during T4.1.
pub const GEMINI_EMBEDDING_USD_PER_MILLION_TOKENS: f64 = 0.20;
const APPROXIMATE_BYTES_PER_TOKEN: u64 = 4;

pub async fn run(args: ConfigArgs) -> anyhow::Result<()> {
    match args.command {
        ConfigCommand::EmbeddingLane(args) => {
            let agent_mode = !std::io::stdout().is_terminal();
            let report = match switch_lane(args, agent_mode).await {
                Ok(report) => report,
                Err(error) if agent_mode => crate::cli::output::emit_client_error_and_exit(
                    "config_update_failed",
                    error.to_string(),
                    65,
                    Some(
                        "correct the repository/runtime configuration and retry `memoryd config embedding-lane`".into(),
                    ),
                ),
                Err(error) => return Err(error),
            };
            print_report(&report, agent_mode)
        }
        ConfigCommand::Harvest(args) => {
            let agent_mode = !std::io::stdout().is_terminal();
            let report = match update_harvest(args) {
                Ok(report) => report,
                Err(error) if agent_mode => crate::cli::output::emit_client_error_and_exit(
                    "config_update_failed",
                    error.to_string(),
                    65,
                    Some("repair local-device.yaml and retry `memoryd config harvest`".into()),
                ),
                Err(error) => return Err(error),
            };
            print_report(&report, agent_mode)
        }
    }
}

fn update_harvest(args: HarvestArgs) -> anyhow::Result<memory_substrate::config::HarvestConfig> {
    let (_, runtime) = crate::paths::resolve_repo_runtime_paths(args.repo, args.runtime);
    let current = memory_substrate::config::load_local_device_config(&runtime)
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} is missing; initialize or adopt the Memorum runtime first",
                runtime.join("local-device.yaml").display()
            )
        })?
        .harvest
        .unwrap_or_default();
    let next = match args.command {
        HarvestCommand::Enable(enable) => memory_substrate::config::HarvestConfig {
            enabled: true,
            interval_minutes: enable.interval_minutes.unwrap_or(current.interval_minutes),
        }
        .normalized(),
        HarvestCommand::Disable => memory_substrate::config::HarvestConfig { enabled: false, ..current },
    };
    memory_substrate::config::store_harvest_config(&runtime, next).map_err(anyhow::Error::msg)?;
    Ok(next)
}

pub async fn configure_init(args: &crate::cli::InitArgs) -> anyhow::Result<()> {
    let Some(lane) = args.embedding_lane else {
        return Ok(());
    };
    if args.print_only {
        // Dry-run init must not mutate config or enqueue re-embeds.
        return Ok(());
    }
    let (repo, runtime) = super::init::resolve_repo_runtime(args);
    // Interactive init at a TTY gets the same consent dialog as the standalone
    // config command; scripted init still hard-requires --consent.
    let agent_mode = !std::io::stdout().is_terminal();
    switch_lane(EmbeddingLaneArgs { lane, repo: Some(repo), runtime: Some(runtime), consent: args.consent }, agent_mode)
        .await
        .map(|_| ())
}

async fn switch_lane(args: EmbeddingLaneArgs, agent_mode: bool) -> anyhow::Result<LaneSwitchReport> {
    let repo = args.repo.unwrap_or_else(|| crate::paths::resolve_repo_runtime_paths(None, None).0);
    let runtime = args.runtime.unwrap_or_else(|| repo.join(".memoryd"));
    let substrate =
        Substrate::open(Roots::new(&repo, &runtime)).await.context("open Memorum substrate before lane switch")?;
    let (chunks, text_bytes) = substrate.api_lane_corpus_stats().await?;
    let estimate = CostEstimate::from_corpus(chunks, text_bytes);
    if args.lane == EmbeddingLane::GeminiApi && !args.consent {
        if agent_mode || !std::io::stdin().is_terminal() {
            anyhow::bail!("gemini-api requires explicit --consent: query text and public/internal memory bodies transit to Google; confidential/personal/encrypted content never leaves this machine; Google's paid Gemini API is not used to train models and retains logs for up to 55 days unless approved ZDR applies. Estimated re-embed cost: ${:.4}.", estimate.usd);
        }
        print_consent(&estimate);
        if !dialoguer::Confirm::new().with_prompt("Enable Gemini API embeddings?").default(false).interact()? {
            anyhow::bail!("Gemini API lane not enabled: consent was declined");
        }
    }
    let triple = lane_triple(args.lane);
    if args.lane == EmbeddingLane::GeminiApi {
        // One atomic document write: no window where the API triple exists
        // without consent (the daemon gate would fail closed but silently).
        memory_substrate::config::store_active_embedding_with_consent(&repo, &triple).map_err(anyhow::Error::msg)?;
    } else {
        memory_substrate::config::store_active_embedding(&repo, &triple).map_err(anyhow::Error::msg)?;
    }
    // A fresh handle observes the new config triple and uses existing reconcile
    // machinery to enqueue missing vectors. Existing triple tables are retained.
    let reopened = Substrate::open(Roots::new(&repo, &runtime)).await?;
    let queued = reopened.pending_embedding_job_count(crate::embedding::embedding_lane_eligibility(&triple))?;
    Ok(LaneSwitchReport {
        active_embedding: triple,
        eligible_chunks: estimate.chunks,
        approximate_tokens: estimate.tokens,
        estimated_usd: estimate.usd,
        pending_reembed_jobs: queued as u64,
        restart_required: true,
        guidance: "Restart memoryd serve to load the new embedding provider. Existing vector tables were retained.",
    })
}

fn print_report(report: &impl Serialize, agent_mode: bool) -> anyhow::Result<()> {
    let data = serde_json::to_value(report)?;
    if agent_mode {
        let render = crate::cli::output::render_local_success(data, Vec::new());
        println!("{}", serde_json::to_string_pretty(&render.envelope)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&data)?);
    }
    Ok(())
}

fn lane_triple(lane: EmbeddingLane) -> EmbeddingTriple {
    match lane {
        EmbeddingLane::Local => EmbeddingTriple {
            provider: crate::embedding::FASTEMBED_CANDLE_PROVIDER.to_string(),
            model_ref: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_MODEL_REF.to_string(),
            dimension: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_DIMENSION,
        },
        EmbeddingLane::GeminiApi => EmbeddingTriple {
            provider: crate::embedding::GEMINI_API_PROVIDER.to_string(),
            model_ref: crate::embedding::GEMINI_API_DEFAULT_MODEL_REF.to_string(),
            dimension: crate::embedding::GEMINI_API_RECOMMENDED_DIMENSION,
        },
    }
}

#[derive(Debug, Serialize)]
struct LaneSwitchReport {
    active_embedding: EmbeddingTriple,
    eligible_chunks: u64,
    approximate_tokens: u64,
    estimated_usd: f64,
    pending_reembed_jobs: u64,
    restart_required: bool,
    guidance: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct CostEstimate {
    chunks: u64,
    tokens: u64,
    usd: f64,
}
impl CostEstimate {
    fn from_corpus(chunks: u64, text_bytes: u64) -> Self {
        let tokens = text_bytes.div_ceil(APPROXIMATE_BYTES_PER_TOKEN);
        Self { chunks, tokens, usd: tokens as f64 / 1_000_000.0 * GEMINI_EMBEDDING_USD_PER_MILLION_TOKENS }
    }
}
fn print_consent(estimate: &CostEstimate) {
    eprintln!("Gemini API embeddings send query text and public/internal plaintext memory bodies to Google.");
    eprintln!(
        "Confidential, personal, and encrypted content never leaves this machine (enforced by the embedding fence)."
    );
    eprintln!("Google's paid Gemini API is not used to train models; logs may be retained up to 55 days unless approved ZDR applies.");
    eprintln!(
        "Estimated re-embed (upper bound; ~4 bytes/token heuristic, excludes ongoing query traffic): {} eligible chunks, about {} tokens, ${:.4} at ${:.2}/M tokens.",
        estimate.chunks, estimate.tokens, estimate.usd, GEMINI_EMBEDDING_USD_PER_MILLION_TOKENS
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{HarvestCommand, HarvestEnableArgs};
    #[test]
    fn cost_estimate_uses_four_bytes_per_token() {
        let e = CostEstimate::from_corpus(2, 9);
        assert_eq!(e.tokens, 3);
        assert!((e.usd - 3.0 / 1_000_000.0 * GEMINI_EMBEDDING_USD_PER_MILLION_TOKENS).abs() < f64::EPSILON);
    }
    #[test]
    fn lane_triples_are_supported() {
        assert!(crate::embedding::is_gemini_api_triple(&lane_triple(EmbeddingLane::GeminiApi)));
        assert!(crate::embedding::is_fastembed_candle_triple(&lane_triple(EmbeddingLane::Local)));
    }

    #[test]
    fn agent_report_uses_the_v1_envelope() {
        let report = LaneSwitchReport {
            active_embedding: lane_triple(EmbeddingLane::GeminiApi),
            eligible_chunks: 2,
            approximate_tokens: 3,
            estimated_usd: 0.00000045,
            pending_reembed_jobs: 2,
            restart_required: true,
            guidance: "restart",
        };
        let render =
            crate::cli::output::render_local_success(serde_json::to_value(report).expect("report JSON"), Vec::new());
        let value = serde_json::to_value(render.envelope).expect("agent envelope JSON");
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["active_embedding"]["provider"], "gemini-api");
        assert_eq!(value["meta"]["schema_version"], "1.0");
    }

    #[test]
    fn harvest_update_creates_section_preserves_device_and_clamps_interval() {
        let temp = tempfile::tempdir().expect("temp");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&runtime).expect("runtime");
        std::fs::write(
            runtime.join("local-device.yaml"),
            "schema_version: 1\ndevice:\n  id: dev_test\n  name: test\n  shard: test\npaths: {}\nprivacy: {}\noperator_key: keep\n",
        )
        .expect("config");

        let report = update_harvest(HarvestArgs {
            repo: Some(temp.path().join("repo")),
            runtime: Some(runtime.clone()),
            command: HarvestCommand::Enable(HarvestEnableArgs { interval_minutes: Some(2) }),
        })
        .expect("update");

        assert_eq!(report, memory_substrate::config::HarvestConfig { enabled: true, interval_minutes: 5 });
        let yaml = std::fs::read_to_string(runtime.join("local-device.yaml")).expect("read");
        assert!(yaml.contains("operator_key: keep"));
        let loaded = memory_substrate::config::load_local_device_config(&runtime).expect("load").expect("present");
        assert_eq!(loaded.device.id, "dev_test");
        assert_eq!(loaded.harvest, Some(report));
    }

    #[test]
    fn harvest_agent_report_uses_the_v1_envelope_without_restart_field() {
        let render = crate::cli::output::render_local_success(
            serde_json::to_value(memory_substrate::config::HarvestConfig { enabled: true, interval_minutes: 30 })
                .expect("report JSON"),
            Vec::new(),
        );
        let value = serde_json::to_value(render.envelope).expect("agent envelope JSON");
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["enabled"], true);
        assert_eq!(value["data"]["interval_minutes"], 30);
        assert!(value["data"].get("restart_required").is_none());
        assert_eq!(value["meta"]["schema_version"], "1.0");
    }
}
