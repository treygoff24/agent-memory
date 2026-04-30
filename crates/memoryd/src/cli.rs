use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

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
