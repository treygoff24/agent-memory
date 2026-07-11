use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "memorum-eval-enrich", about = "Create resumable abstraction/cue benchmark sidecars")]
struct Cli {
    #[arg(long, default_value = "datasets")]
    dataset_dir: PathBuf,
    #[arg(long, default_value = "codex")]
    harness: String,
    #[arg(long)]
    structural_only: bool,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    locomo_qa_per_conversation: Option<usize>,
    #[arg(long, default_value_t = 60)]
    longmemeval_per_split: usize,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = memorum_eval::enrichment::EnrichmentOptions {
        structural_only: cli.structural_only,
        harness: cli.harness,
        limit: cli.limit,
        locomo_qa_per_conversation: cli.locomo_qa_per_conversation,
        longmemeval_per_split: cli.longmemeval_per_split,
    };
    match memorum_eval::enrichment::enrich_dataset_dir(&cli.dataset_dir, &options) {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("enrichment failed: {error}");
            ExitCode::FAILURE
        }
    }
}
