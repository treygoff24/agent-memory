use std::fs;
use std::path::PathBuf;

use clap::Parser;
use memory_substrate::merge::{merge_markdown, MergeError, MergeInput, MergeResult};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    base: String,
    #[arg(long)]
    ours: String,
    #[arg(long)]
    theirs: String,
    #[arg(long)]
    path: String,
}

fn main() {
    let args = Args::parse();
    if let Err(err) = run(args) {
        eprintln!("merge-driver: {err}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), DriverError> {
    let base = read_input(&args.base)?;
    let ours = read_input(&args.ours)?;
    let theirs = read_input(&args.theirs)?;
    let merged = merge_markdown(MergeInput { base: &base, ours: &ours, theirs: &theirs, path: &args.path })
        .map_err(DriverError::Merge)?;
    let text = match merged {
        MergeResult::Clean(text) | MergeResult::Quarantine(text) => text,
    };
    fs::write(&args.ours, text).map_err(|err| DriverError::Io { path: args.ours.clone(), source: err })?;
    Ok(())
}

fn read_input(path: &str) -> Result<String, DriverError> {
    fs::read_to_string(path).map_err(|source| DriverError::Io { path: path.to_string(), source })
}

#[derive(Debug)]
enum DriverError {
    Merge(MergeError),
    Io { path: String, source: std::io::Error },
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriverError::Merge(err) => write!(f, "{err}"),
            DriverError::Io { path, source } => {
                let display = PathBuf::from(path).display().to_string();
                write!(f, "read {display}: {source}")
            }
        }
    }
}

impl std::error::Error for DriverError {}
