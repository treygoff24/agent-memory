pub mod assertions;
#[cfg(all(feature = "quality", feature = "judge"))]
pub mod benchmark;
pub mod daemon_scaffold;
pub mod harness_runner;
#[cfg(feature = "judge")]
pub mod judge;
pub mod orchestrator;
#[cfg(feature = "quality")]
pub mod quality;
pub mod simulator;
mod support;

/// Drive a future to completion from a synchronous context.
///
/// Uses a per-call single-threaded tokio runtime. Smoke tests and the
/// orchestrator both call into this; a `current_thread` runtime is lightweight
/// and avoids the busy-spin + `yield_now()` of the prior home-rolled no-op-waker
/// executor. This is the single canonical `block_on` for the crate — the
/// orchestrator's worker threads call it too. (H-R2)
pub use support::block_on;
#[cfg(test)]
mod json_escape_tests {
    use super::support::json_escape;

    #[test]
    fn escapes_double_quote() {
        assert_eq!(json_escape(r#"say "hi""#), r#"say \"hi\""#);
    }

    #[test]
    fn escapes_backslash() {
        assert_eq!(json_escape(r"a\b"), r"a\\b");
    }

    #[test]
    fn escapes_newline_cr_tab() {
        assert_eq!(json_escape("a\nb\rc\td"), r"a\nb\rc\td");
    }

    #[test]
    fn escapes_other_control_chars() {
        // NUL (U+0000), BEL (U+0007), ESC (U+001B) must become \uXXXX
        assert_eq!(json_escape("\x00"), "\\u0000");
        assert_eq!(json_escape("\x07"), "\\u0007");
        assert_eq!(json_escape("\x1b"), "\\u001b");
    }

    #[test]
    fn preserves_plain_ascii_and_unicode() {
        assert_eq!(json_escape("hello world"), "hello world");
        assert_eq!(json_escape("café ñoño"), "café ñoño");
    }

    #[test]
    fn combined_sequence() {
        assert_eq!(json_escape("line1\nline2\t\"end\"\\done\x01"), "line1\\nline2\\t\\\"end\\\"\\\\done\\u0001",);
    }
}

use std::cell::Cell;

#[cfg(feature = "quality")]
pub mod enrichment;

thread_local! {
    /// Per-test assertion counter. Incremented by `eval_assert!` / `eval_assert_eq!`
    /// macros. Printed as `MEMORUM_EVAL_ASSERTIONS=<n>` at test end so the
    /// orchestrator can report accurate per-test assertion granularity. (H-B3)
    static EVAL_ASSERTION_COUNTER: Cell<usize> = const { Cell::new(0) };
}

/// Increment the thread-local eval assertion counter and return the new count.
pub fn eval_assertion_tick() -> usize {
    EVAL_ASSERTION_COUNTER.with(|cell| {
        let next = cell.get().saturating_add(1);
        cell.set(next);
        next
    })
}

/// Print the assertion count in a format the orchestrator can parse.
///
/// Call this at the end of an eval test to report per-test assertion granularity.
/// The orchestrator (`run_cargo_test`) scans cargo test stdout for
/// `MEMORUM_EVAL_ASSERTIONS=<n>` and uses it to populate the JSON report's
/// `assertions` / `assertions_passed` fields.
pub fn eval_flush_assertion_count() {
    let count = EVAL_ASSERTION_COUNTER.with(Cell::get);
    // EVAL_ASSERTION_COUNT_MARKER already ends with "=", so this prints
    // MEMORUM_EVAL_ASSERTIONS=<count> as a complete marker line.
    println!("{}{count}", orchestrator::EVAL_ASSERTION_COUNT_MARKER);
}

/// Assert a condition and increment the eval assertion counter.
///
/// Use in place of `assert!` in eval tests to enable accurate assertion-count
/// reporting in the orchestrator JSON output.
#[macro_export]
macro_rules! eval_assert {
    ($cond:expr $(, $msg:expr)* $(,)?) => {{
        $crate::eval_assertion_tick();
        assert!($cond $(, $msg)*);
    }};
}

/// Assert equality and increment the eval assertion counter.
#[macro_export]
macro_rules! eval_assert_eq {
    ($left:expr, $right:expr $(, $msg:expr)* $(,)?) => {{
        $crate::eval_assertion_tick();
        assert_eq!($left, $right $(, $msg)*);
    }};
}

use std::path::PathBuf;

use clap::Parser;
use orchestrator::{HarnessMode, OutputFormat, RequiredReleaseSet};

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

    /// Fail if a required release coverage set still has deferred semantic tests.
    #[arg(long, value_enum)]
    pub required_release_set: Option<RequiredReleaseSet>,
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
            required_release_set: self.required_release_set,
        }
    }
}
