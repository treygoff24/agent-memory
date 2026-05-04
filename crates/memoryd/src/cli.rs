use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use clap::{ArgAction, Args, Parser, Subcommand};

use crate::protocol::{PeerActivityFormat, RealityCheckRequest, RequestPayload};

#[derive(Debug, Parser)]
#[command(name = "memoryd")]
#[command(about = "Local daemon and thin client for agent-memory")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the local daemon.
    Serve(ServeArgs),
    /// Run the stdio MCP server that forwards tool calls to the daemon.
    Mcp(SocketArgs),
    /// Query daemon health.
    Status(SocketArgs),
    /// Check local substrate and daemon configuration.
    Doctor(RootArgs),
    /// Search memory through the daemon.
    Search(SearchArgs),
    /// Read one memory by id through the daemon.
    Get(GetArgs),
    /// Record a low-friction substrate note.
    WriteNote(WriteNoteArgs),
    /// Write a governed structured memory.
    Write(WriteMemoryArgs),
    /// Supersede an existing memory through governance.
    Supersede(SupersedeArgs),
    /// Tombstone a memory through governance.
    Forget(ForgetArgs),
    /// Admin review queue commands.
    Review(ReviewArgs),
    /// Passive recall hook commands.
    Recall(RecallArgs),
    /// Stream F dreaming admin commands.
    Dream(DreamArgs),
    /// Cross-session peer coordination admin commands.
    Peer(PeerArgs),
    /// Launch the local terminal UI.
    Ui(UiArgs),
    /// Local web dashboard lifecycle commands.
    Web(WebArgs),
    /// Weekly Reality Check ritual commands.
    RealityCheck(RealityCheckArgs),
    /// Admin privacy inspection commands.
    Privacy(PrivacyArgs),
    /// Optional Privacy Filter commands.
    PrivacyFilter(PrivacyFilterArgs),
    /// Local encrypted-tier device key commands.
    Device(DeviceArgs),
}

#[derive(Debug, Args)]
pub struct SocketArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
}

#[derive(Debug, Args)]
pub struct UiArgs {
    /// Start with panel N active.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=9))]
    pub panel: u8,
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
}

#[derive(Debug, Args)]
pub struct WebArgs {
    #[command(subcommand)]
    pub command: WebCommand,
}

#[derive(Debug, Subcommand)]
pub enum WebCommand {
    /// Enable the localhost web dashboard.
    Enable(WebEnableArgs),
    /// Disable the localhost web dashboard.
    Disable(SocketArgs),
    /// Show web dashboard status.
    Status(WebStatusArgs),
}

#[derive(Debug, Args)]
pub struct WebEnableArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Localhost port for the dashboard.
    #[arg(long, default_value_t = 7137, value_parser = clap::value_parser!(u16).range(1024..=65535))]
    pub port: u16,
}

#[derive(Debug, Args)]
pub struct WebStatusArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RealityCheckArgs {
    #[command(subcommand)]
    pub command: RealityCheckCommand,
}

#[derive(Debug, Subcommand)]
pub enum RealityCheckCommand {
    /// Start, resume, or list a Reality Check session.
    Run(RealityCheckRunArgs),
    /// Skip the current week's Reality Check.
    Skip(SocketArgs),
    /// Snooze the Reality Check reminder.
    Snooze(RealityCheckSnoozeArgs),
}

#[derive(Debug, Args)]
pub struct RealityCheckRunArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Override the number of scored items returned for this run.
    #[arg(long)]
    pub top_n: Option<usize>,
    /// Restrict scoring to a namespace.
    #[arg(long)]
    pub namespace: Option<String>,
    /// Route the session to an already-open TUI.
    #[arg(long)]
    pub tui: bool,
    /// Print JSON and do not start an interactive session.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RealityCheckSnoozeArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// ISO date (YYYY-MM-DD) to snooze until.
    #[arg(long)]
    pub until: Option<String>,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Unix socket path used to accept memoryd clients.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Initialize the substrate if it has not been bootstrapped yet.
    #[arg(long)]
    pub init: bool,
}

#[derive(Debug, Args)]
pub struct RootArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Query text.
    pub query: String,
    /// Maximum number of results to return.
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    /// Include full bodies instead of bounded summaries.
    #[arg(long)]
    pub include_body: bool,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Memory id to read.
    pub id: String,
    /// Include provenance details when available.
    #[arg(long)]
    pub include_provenance: bool,
}

#[derive(Debug, Args)]
pub struct WriteNoteArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Note text.
    pub text: String,
}

#[derive(Debug, Args)]
pub struct WriteMemoryArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Optional memory title.
    #[arg(long)]
    pub title: Option<String>,
    /// Memory tags.
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    /// Optional governance metadata as JSON.
    #[arg(long)]
    pub meta: Option<String>,
    /// Markdown body.
    pub body: String,
}

#[derive(Debug, Args)]
pub struct SupersedeArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Existing memory id to supersede.
    pub old_id: String,
    /// Replacement markdown body.
    pub content: String,
    /// Governance supersession reason.
    #[arg(long)]
    pub reason: String,
    /// Optional governance metadata as JSON.
    #[arg(long)]
    pub meta: Option<String>,
}

#[derive(Debug, Args)]
pub struct ForgetArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Memory id to tombstone.
    pub id: String,
    /// Tombstone reason.
    #[arg(long)]
    pub reason: String,
}

#[derive(Debug, Args)]
pub struct ReviewArgs {
    #[command(subcommand)]
    pub command: ReviewCommand,
}

#[derive(Debug, Subcommand)]
pub enum ReviewCommand {
    /// List memories that require admin review.
    Queue(ReviewQueueArgs),
    /// Approve a memory from the review queue.
    Approve(ReviewApproveArgs),
    /// Reject a memory from the review queue.
    Reject(ReviewRejectArgs),
}

#[derive(Debug, Args)]
pub struct ReviewQueueArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Maximum number of review items to return.
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct ReviewApproveArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Memory id to approve.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct ReviewRejectArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Reason for rejecting the memory.
    #[arg(long)]
    pub reason: String,
    /// Memory id to reject.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct RecallArgs {
    #[command(subcommand)]
    pub command: RecallCommand,
}

#[derive(Debug, Args)]
pub struct PeerArgs {
    #[command(subcommand)]
    pub command: PeerCommand,
}

#[derive(Debug, Subcommand)]
pub enum PeerCommand {
    /// Show current cross-session coordination state.
    Status(PeerStatusArgs),
    /// Show recent peer-update delivery audit entries.
    Activity(PeerActivityArgs),
    /// Forcibly release one advisory claim lock.
    ReleaseLock(PeerReleaseLockArgs),
}

#[derive(Debug, Args)]
pub struct PeerStatusArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
}

#[derive(Debug, Args)]
pub struct PeerActivityArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Filter to deliveries from or to one session id.
    #[arg(long)]
    pub session: Option<String>,
    /// Filter by time: HH:MM, YYYY-MM-DD, or RFC3339.
    #[arg(long)]
    pub since: Option<String>,
    /// Maximum number of audit entries to print.
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    pub format: PeerActivityFormat,
}

#[derive(Debug, Args)]
pub struct PeerReleaseLockArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long, default_value = "/tmp/memoryd.sock")]
    pub socket: PathBuf,
    /// Memory id whose advisory claim lock should be released.
    pub memory_id: String,
    /// Skip the interactive y/N confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct DreamArgs {
    #[command(subcommand)]
    pub command: DreamCommand,
}

#[derive(Debug, Subcommand)]
pub enum DreamCommand {
    /// Report dreaming status, inventory, leases, and recent run summaries.
    Status(DreamStatusArgs),
    /// Run a manual dream for one scope, failing fast on lease errors.
    Now(DreamNowArgs),
    /// Run the scheduled lease-elected dream path for one scope, with bounded retry.
    Scheduled(DreamScheduledArgs),
    /// Run the scheduled cleanup pass for this device.
    Cleanup(DreamCleanupArgs),
    /// Review recent dream journal, question, candidate, and cleanup outputs.
    Review(DreamReviewArgs),
    /// Enable dreaming on this device by removing the local disabled sentinel.
    Enable(DreamToggleArgs),
    /// Disable dreaming on this device by creating the local disabled sentinel.
    Disable(DreamToggleArgs),
}

#[derive(Debug, Args)]
pub struct DreamStatusArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Emit a structured JSON DreamStatusReport.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DreamNowArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Dream scope: `me`, `agent`, `project:<id>`, or `org:<id>`.
    #[arg(long)]
    pub scope: String,
    /// Override an active foreign lease.
    #[arg(long)]
    pub force: bool,
    /// Harness CLI name to use for this manual run.
    #[arg(long = "cli")]
    pub cli_override: Option<String>,
    /// Emit JSON. Manual dream reports are currently JSON in both modes.
    #[arg(long)]
    pub json: bool,
}

impl DreamNowArgs {
    pub fn cli_used(&self) -> Option<String> {
        self.cli_override.clone()
    }
}

#[derive(Debug, Args)]
pub struct DreamScheduledArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Dream scope: `me`, `agent`, `project:<id>`, or `org:<id>`.
    #[arg(long)]
    pub scope: String,
    /// Harness CLI name to use for this scheduled run.
    #[arg(long = "cli")]
    pub cli_override: Option<String>,
    /// Emit JSON. Scheduled dream reports are currently JSON in both modes.
    #[arg(long)]
    pub json: bool,
}

impl DreamScheduledArgs {
    pub fn cli_used(&self) -> Option<String> {
        self.cli_override.clone()
    }
}

#[derive(Debug, Args)]
pub struct DreamCleanupArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Device id to write cleanup reports under. Defaults to local-device.yaml.
    #[arg(long)]
    pub device_id: Option<String>,
    /// Cleanup timestamp as RFC3339. Defaults to current UTC time.
    #[arg(long)]
    pub now: Option<String>,
    /// Emit JSON. Cleanup reports are currently JSON in both modes.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DreamReviewArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root. Accepted for command symmetry; review reads git-synced outputs.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Review window such as 7d, 24h, or 60m.
    #[arg(long)]
    pub since: String,
    /// Optional dream scope: `me`, `agent`, `project:<id>`, or `org:<id>`.
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Args)]
pub struct DreamToggleArgs {
    /// Local per-device runtime root containing the device-local dream-disabled sentinel.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum RecallCommand {
    /// Print a Stream E startup recall XML block.
    StartupBlock(RecallStartupArgs),
    /// Print a Stream E per-turn delta recall XML block.
    DeltaBlock(RecallDeltaArgs),
}

#[derive(Debug, Args)]
pub struct RecallSocketArgs {
    /// Canonical memory repository root. Present for hook contract clarity; daemon socket is authoritative.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root. The default recall socket is `<runtime>/memoryd.sock`.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Optional Unix socket override.
    #[arg(long)]
    pub socket: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct RecallStartupArgs {
    #[command(flatten)]
    pub socket: RecallSocketArgs,
    #[arg(long)]
    pub cwd: PathBuf,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub harness: String,
    #[arg(long)]
    pub harness_version: Option<String>,
    #[arg(long, default_value_t = true, conflicts_with = "no_include_recent")]
    pub include_recent: bool,
    #[arg(long = "no-include-recent", action = ArgAction::SetTrue)]
    pub no_include_recent: bool,
    #[arg(long)]
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Args)]
pub struct RecallDeltaArgs {
    #[command(flatten)]
    pub socket: RecallSocketArgs,
    #[arg(long)]
    pub cwd: PathBuf,
    #[arg(long)]
    pub session_id: String,
    #[arg(long)]
    pub harness: String,
    #[arg(long)]
    pub message: String,
    #[arg(long)]
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Args)]
pub struct PrivacyArgs {
    #[command(subcommand)]
    pub command: PrivacyCommand,
}

#[derive(Debug, Subcommand)]
pub enum PrivacyCommand {
    /// Show Stream D privacy status.
    Status(RootArgs),
    /// Classify text or a file with the always-on Layer 1 scanner.
    Scan(PrivacyScanArgs),
    /// Scan staged git delta text with Layer 1 before commit.
    ScanDelta(PrivacyScanDeltaArgs),
}

#[derive(Debug, Args)]
pub struct PrivacyScanArgs {
    /// Text to classify.
    #[arg(long, conflicts_with = "file")]
    pub text: Option<String>,
    /// File whose contents should be classified.
    #[arg(long, conflicts_with = "text")]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct PrivacyScanDeltaArgs {
    /// Repository whose staged delta should be scanned.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
}

#[derive(Debug, Args)]
pub struct PrivacyFilterArgs {
    #[command(subcommand)]
    pub command: PrivacyFilterCommand,
}

#[derive(Debug, Subcommand)]
pub enum PrivacyFilterCommand {
    /// Explain how Privacy Filter installation is handled.
    Install,
    /// Enable the optional Privacy Filter provider when installed.
    Enable,
    /// Disable the optional Privacy Filter provider.
    Disable,
    /// Show optional Privacy Filter status.
    Status,
}

#[derive(Debug, Args)]
pub struct DeviceArgs {
    #[command(subcommand)]
    pub command: DeviceCommand,
}

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// Generate local age key material in the runtime privacy key store.
    Onboard(DeviceOnboardArgs),
    /// Rotate local age key material.
    RotateKeys(DeviceOnboardArgs),
    /// Mark a device revoke request as operator-required.
    Revoke(DeviceRevokeArgs),
}

#[derive(Debug, Args)]
pub struct DeviceOnboardArgs {
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
}

#[derive(Debug, Args)]
pub struct DeviceRevokeArgs {
    /// Device id to revoke.
    pub device_id: String,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiLaunchError {
    NonInteractiveStdin,
    BinaryMissing,
}

impl UiLaunchError {
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::NonInteractiveStdin => 2,
            Self::BinaryMissing => 4,
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::NonInteractiveStdin => "memoryd ui requires an interactive terminal.",
            Self::BinaryMissing => {
                "memoryd-tui binary not found; reinstall with `cargo install memoryd-tui` or ensure both binaries are in the same prefix"
            }
        }
    }
}

pub fn validate_ui_stdin(stdin_is_tty: bool) -> Result<(), UiLaunchError> {
    if stdin_is_tty {
        Ok(())
    } else {
        Err(UiLaunchError::NonInteractiveStdin)
    }
}

pub fn ui_subprocess_args(args: &UiArgs) -> Vec<OsString> {
    vec![
        OsString::from("--panel"),
        OsString::from(args.panel.to_string()),
        OsString::from("--socket"),
        args.socket.as_os_str().to_owned(),
    ]
}

pub fn resolve_memoryd_tui_binary(current_exe: &Path, path_env: Option<&OsStr>) -> Result<PathBuf, UiLaunchError> {
    if let Some(sibling) = current_exe.parent().map(|dir| dir.join("memoryd-tui")).filter(|path| path.is_file()) {
        return Ok(sibling);
    }

    let Some(path_env) = path_env else {
        return Err(UiLaunchError::BinaryMissing);
    };
    std::env::split_paths(path_env)
        .map(|dir| dir.join("memoryd-tui"))
        .find(|path| path.is_file())
        .ok_or(UiLaunchError::BinaryMissing)
}

pub fn web_request_payload(command: &WebCommand) -> RequestPayload {
    match command {
        WebCommand::Enable(args) => {
            RequestPayload::WebEnable { port: args.port, socket_path: args.socket.to_string_lossy().into_owned() }
        }
        WebCommand::Disable(_) => RequestPayload::WebDisable,
        WebCommand::Status(_) => RequestPayload::WebStatus,
    }
}

pub fn reality_check_request_payload(command: &RealityCheckCommand) -> Result<RequestPayload, i32> {
    match command {
        RealityCheckCommand::Run(args) if args.json => Ok(RequestPayload::RealityCheck(RealityCheckRequest::List {
            namespace: args.namespace.clone(),
            limit: args.top_n,
        })),
        RealityCheckCommand::Run(args) => Ok(RequestPayload::RealityCheck(RealityCheckRequest::Run {
            session_id: None,
            namespace: args.namespace.clone(),
            limit: args.top_n,
        })),
        RealityCheckCommand::Skip(_) => Ok(RequestPayload::RealityCheck(RealityCheckRequest::Skip)),
        RealityCheckCommand::Snooze(args) => {
            validate_snooze_until(args.until.as_deref())?;
            Ok(RequestPayload::RealityCheck(RealityCheckRequest::Snooze {
                until: validate_snooze_until(args.until.as_deref())?
                    .map(|date| date.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc()),
            }))
        }
    }
}

pub fn validate_snooze_until(raw: Option<&str>) -> Result<Option<NaiveDate>, i32> {
    raw.map(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| 1)).transpose()
}
