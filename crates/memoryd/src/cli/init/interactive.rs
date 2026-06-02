//! Interactive (TTY) frontend for `memoryd init`.
//!
//! STUB: T05 fills this in with dialoguer-backed prompts. For now it drives the
//! shared setup engine through [`InteractiveIo`], whose decision methods return
//! [`SetupError::Unsupported`]. The error surfaces as a non-zero exit so the
//! TTY path is visibly unfinished rather than silently doing nothing.

use crate::cli::InitArgs;
use crate::setup::{InteractiveIo, SetupEngine};

use super::resolve_repo_runtime;

/// Drive interactive setup. Returns an error until T05 implements the prompts.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    let mut io = InteractiveIo;
    let engine = SetupEngine::new(repo, runtime);
    // `InteractiveIo` rejects every decision prompt with
    // `SetupError::Unsupported`, so `run` short-circuits before any step
    // executes. T05 swaps in a dialoguer-backed `SetupIo`.
    let _report = engine.run(&mut io).await?;
    Ok(())
}
