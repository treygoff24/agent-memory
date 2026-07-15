use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use memorum_eval::benchmark::Split;
use memorum_eval::enrichment::Generation;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SplitArg {
    Dev,
    Holdout,
    Both,
}

#[derive(Debug, Parser)]
#[command(name = "memorum-eval-enrich", about = "Create resumable abstraction/cue benchmark sidecars")]
struct Cli {
    #[arg(long, default_value = "datasets")]
    dataset_dir: PathBuf,
    #[arg(long, default_value = "codex")]
    harness: String,
    #[arg(long, value_enum, default_value = "v1")]
    generation: Generation,
    #[arg(
        long,
        value_enum,
        default_value = "dev",
        help = "Dataset split to enrich; defaults to dev to protect holdout from prompt tuning"
    )]
    split: SplitArg,
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
    let splits = match cli.split {
        SplitArg::Dev => vec![Split::Dev],
        SplitArg::Holdout => vec![Split::Holdout],
        SplitArg::Both => vec![Split::Dev, Split::Holdout],
    };
    let options = memorum_eval::enrichment::EnrichmentOptions {
        generation: cli.generation,
        splits,
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, FromArgMatches};

    use super::{Cli, SplitArg};

    #[test]
    fn split_defaults_to_dev_and_help_explains_why() {
        let matches = Cli::command().try_get_matches_from(["memorum-eval-enrich"]).expect("defaults parse");
        assert!(matches!(Cli::from_arg_matches(&matches).expect("cli").split, SplitArg::Dev));
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("defaults to dev to protect holdout"));
    }
}
