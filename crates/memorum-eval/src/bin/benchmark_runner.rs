use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use memorum_eval::benchmark::{run_baseline, BenchmarkConfig, BenchmarkEmbeddingLane, Split};
use memorum_eval::judge::{BenchmarkJudge, DeterministicMockJudge, ExternalCommandJudge};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SplitArg {
    Dev,
    Holdout,
    Both,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum EmbeddingArg {
    FtsOnly,
    Daemon,
    GeminiApi,
}

#[derive(Debug, Parser)]
#[command(name = "memorum-eval-benchmark", about = "Run LoCoMo and LongMemEval through real Memorum daemon paths")]
struct Cli {
    #[arg(long, default_value = "datasets")]
    dataset_dir: PathBuf,
    #[arg(long, default_value = "baseline_0.json")]
    output_file: PathBuf,
    #[arg(long, value_enum, default_value = "both")]
    split: SplitArg,
    #[arg(long)]
    locomo_conversations: Option<usize>,
    #[arg(long)]
    locomo_qa_per_conversation: Option<usize>,
    #[arg(long, default_value_t = 60)]
    longmemeval_per_split: usize,
    /// Use the full cleaned haystacks instead of the evidence-only oracle file.
    #[arg(long)]
    longmemeval_cleaned: bool,
    #[arg(long = "embedding-lane", alias = "embedding", value_enum, default_value = "fts-only")]
    embedding: EmbeddingArg,
    /// External judge executable. It receives one JSON record on stdin and must
    /// emit {"score": number, "rationale": string} on stdout.
    #[arg(long)]
    judge_command: Option<String>,
    #[arg(long)]
    judge_arg: Vec<String>,
    /// Exercise deterministic judge plumbing without invoking an external model.
    #[arg(long, conflicts_with = "judge_command")]
    mock_judge: bool,
    /// Expected sensitivity tier for benchmark writes; used to classify mismatches.
    #[arg(long, default_value = "internal")]
    expected_sensitivity: String,
    /// Timeout in seconds for external judge commands.
    #[arg(long, default_value_t = 60)]
    judge_timeout: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("benchmark run failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let splits = match cli.split {
        SplitArg::Dev => vec![Split::Dev],
        SplitArg::Holdout => vec![Split::Holdout],
        SplitArg::Both => vec![Split::Dev, Split::Holdout],
    };
    let config = BenchmarkConfig {
        dataset_dir: cli.dataset_dir,
        splits,
        locomo_conversation_limit: cli.locomo_conversations,
        locomo_qa_per_conversation: cli.locomo_qa_per_conversation,
        longmemeval_per_split: cli.longmemeval_per_split,
        longmemeval_cleaned: cli.longmemeval_cleaned,
        embedding_lane: match cli.embedding {
            EmbeddingArg::FtsOnly => BenchmarkEmbeddingLane::FtsOnly,
            EmbeddingArg::Daemon => BenchmarkEmbeddingLane::DaemonConfigured,
            EmbeddingArg::GeminiApi => BenchmarkEmbeddingLane::GeminiApi,
        },
        expected_sensitivity: cli.expected_sensitivity,
        judge_timeout: cli.judge_timeout,
    };
    let external = cli.judge_command.map(|program| {
        ExternalCommandJudge::new(program, cli.judge_arg).with_timeout(Duration::from_secs(cli.judge_timeout))
    });
    let mock = cli.mock_judge.then_some(DeterministicMockJudge);
    let judge: Option<&dyn BenchmarkJudge> = external
        .as_ref()
        .map(|judge| judge as &dyn BenchmarkJudge)
        .or_else(|| mock.as_ref().map(|judge| judge as &dyn BenchmarkJudge));
    let report = memorum_eval::block_on(run_baseline(&config, judge))?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    std::fs::write(&cli.output_file, format!("{json}\n"))
        .map_err(|error| format!("write {}: {error}", cli.output_file.display()))?;
    println!("{json}");
    eprintln!("wrote baseline_0 report to {}", cli.output_file.display());
    Ok(())
}
