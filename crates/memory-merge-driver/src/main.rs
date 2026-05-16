use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    persist_merged_output(&args.ours, &text)?;
    Ok(())
}

fn read_input(path: &str) -> Result<String, DriverError> {
    fs::read_to_string(path).map_err(|source| DriverError::Io { operation: "read", path: path.to_string(), source })
}

fn persist_merged_output(path: &str, text: &str) -> Result<(), DriverError> {
    let output = Path::new(path);
    let result = write_atomically(output, text.as_bytes());
    if let Err(source) = result {
        let _ = source.temp_path.as_ref().map(fs::remove_file);
        return Err(DriverError::Io { operation: "write", path: path.to_string(), source: source.error });
    }
    Ok(())
}

fn write_atomically(output: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    let directory = output.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let file_name = output
        .file_name()
        .ok_or_else(|| AtomicWriteError::without_temp(invalid_input("output path has no file name")))?;
    let temp_path =
        directory.join(format!(".{}.{}.{}.tmp", file_name.to_string_lossy(), std::process::id(), unique_suffix()));

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| AtomicWriteError::with_temp(error, temp_path.clone()))?;
    file.write_all(bytes).map_err(|error| AtomicWriteError::with_temp(error, temp_path.clone()))?;
    file.sync_all().map_err(|error| AtomicWriteError::with_temp(error, temp_path.clone()))?;
    drop(file);
    fs::rename(&temp_path, output).map_err(|error| AtomicWriteError::with_temp(error, temp_path))?;
    Ok(())
}

#[derive(Debug)]
struct AtomicWriteError {
    error: std::io::Error,
    temp_path: Option<PathBuf>,
}

impl AtomicWriteError {
    fn with_temp(error: std::io::Error, temp_path: PathBuf) -> Self {
        Self { error, temp_path: Some(temp_path) }
    }

    fn without_temp(error: std::io::Error) -> Self {
        Self { error, temp_path: None }
    }
}

fn invalid_input(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_nanos())
}

#[derive(Debug)]
enum DriverError {
    Merge(MergeError),
    Io { operation: &'static str, path: String, source: std::io::Error },
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriverError::Merge(err) => write!(f, "{err}"),
            DriverError::Io { operation, path, source } => {
                let display = PathBuf::from(path).display().to_string();
                write!(f, "{operation} {display}: {source}")
            }
        }
    }
}

impl std::error::Error for DriverError {}
