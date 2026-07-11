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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match memorum_eval::enrichment::enrich_dataset_dir(&cli.dataset_dir, cli.structural_only, &cli.harness, cli.limit) {
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
