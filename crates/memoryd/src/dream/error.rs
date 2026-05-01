use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonStage {
    Parse,
}

#[derive(Debug, Error)]
pub enum HarnessCliError {
    #[error("harness CLI is not installed")]
    NotInstalled,
    #[error("harness CLI is not authenticated: {hint}")]
    NotAuthenticated { hint: String },
    #[error("harness CLI timed out after {duration:?}")]
    Timeout { duration: Duration },
    #[error("harness CLI exited unsuccessfully: code={code:?}, stderr redacted")]
    SubprocessExit { code: Option<i32>, stderr_tail: String },
    #[error("harness CLI returned malformed JSON during {stage:?}: raw output redacted")]
    MalformedJson { stage: JsonStage, raw: String },
    #[error("harness CLI I/O error: {0}")]
    Io(#[from] std::io::Error),
}
