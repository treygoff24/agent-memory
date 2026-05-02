pub mod assertions;
pub mod daemon_scaffold;
pub mod harness_runner;
pub mod orchestrator;
pub mod simulator;

use std::path::PathBuf;

use clap::Parser;
use orchestrator::{HarnessMode, OutputFormat};

#[derive(Debug, Parser)]
#[command(name = "memorum-eval", bin_name = "memorum-eval", version, about = "Memorum eval harness orchestrator")]
pub struct EvalCli {
    /// Which harness backs real-harness tests.
    #[arg(long, value_enum, default_value = "mock")]
    pub harness: HarnessMode,

    /// Run only tests matching a glob-like pattern on test name or number.
    #[arg(long)]
    pub filter: Option<String>,

    /// Output format. Defaults to text on TTY and JSON otherwise.
    #[arg(long, value_enum)]
    pub output: Option<OutputFormat>,

    /// Write JSON output to this file in addition to stdout.
    #[arg(long)]
    pub output_file: Option<PathBuf>,

    /// Global per-test timeout override, in seconds.
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Parallel worker count for the parallel group.
    #[arg(long, default_value_t = 4)]
    pub workers: usize,

    /// Do not delete temp trees after tests complete.
    #[arg(long)]
    pub no_cleanup: bool,

    /// List the Stream H eval catalog and exit.
    #[arg(long)]
    pub list: bool,

    /// Print per-step output as tests run.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

impl EvalCli {
    pub fn command() -> clap::Command {
        <Self as clap::CommandFactory>::command()
    }

    pub fn run_config(&self) -> orchestrator::EvalRunConfig {
        orchestrator::EvalRunConfig {
            harness_mode: self.harness,
            filter: self.filter.clone(),
            timeout_seconds: self.timeout,
            workers: self.workers,
            no_cleanup: self.no_cleanup,
            verbose: self.verbose,
        }
    }
}
