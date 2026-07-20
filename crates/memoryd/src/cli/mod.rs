use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use crate::protocol::{PeerActivityFormat, QuarantineResolutionMode, RealityCheckRequest, RequestPayload};

#[derive(Debug, Parser)]
#[command(name = "memoryd", version)]
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
    Mcp(McpArgs),
    /// Query daemon health.
    Status(SocketArgs),
    /// Check local substrate and daemon configuration.
    Doctor(DoctorArgs),
    /// Configure the active embedding lane for this Memorum repository.
    Config(ConfigArgs),
    /// Print the machine-readable CLI agent contract (envelope, exit codes,
    /// per-command schemas). Generated from the implementing types.
    Schema(SchemaArgs),
    /// Search memory through the daemon.
    Search(SearchArgs),
    /// Read one memory by id through the daemon.
    Get(GetArgs),
    /// Record a low-friction substrate note.
    WriteNote(WriteNoteArgs),
    /// Write a governed structured memory.
    Write(WriteMemoryArgs),
    /// Capture source artifacts for grounded memory writes.
    Source(SourceArgs),
    /// Reveal decrypted content of an encrypted memory. Audited: a successful
    /// reveal writes an `EncryptedContentRevealed` event. Requires `--allow-reveal`.
    ///
    /// Example:
    ///   memoryd reveal mem_20260708_a1b2c3d4e5f60718_000001 --reason "user asked to see it" --allow-reveal
    Reveal(RevealArgs),
    /// Record a Stream F substrate observation (observation/pattern/signal).
    ///
    /// Example:
    ///   memoryd observe "the deploy step flakes on cold caches" --kind signal --entity ent_deploy
    Observe(ObserveArgs),
    /// Supersede an existing memory through governance.
    Supersede(SupersedeArgs),
    /// Tombstone a memory through governance.
    Forget(ForgetArgs),
    /// Admin review queue commands.
    Review(ReviewArgs),
    /// Merge-quarantine inspection and resolution commands.
    Quarantine(QuarantineArgs),
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
    /// Export a portable JSON snapshot of substrate contents.
    ///
    /// Opening the substrate triggers standard runtime-initialization side
    /// effects even though export does not write memory content, including
    /// runtime-dir creation, index-repair replay, and event-log mirror rebuild.
    /// Stop any running `memoryd serve` daemon before exporting against the
    /// same `--repo` / `--runtime` pair.
    Export(crate::export::ExportArgs),
    /// Backfill prior Claude Code and Codex CLI memory into Memorum.
    ///
    /// Non-destructive and idempotent: source files are never modified, and
    /// re-runs skip sources whose content hash hasn't changed since the last
    /// import. Per the locked design, every memory goes through the daemon
    /// socket so privacy, governance, and event-log machinery all fire.
    Import(ImportArgs),
    /// First-run setup: detect prior harness memory, import it, provision the
    /// daemon, and wire the passive-recall lifecycle hooks. The MCP bridge is
    /// opt-in (`--wire-mcp`), not wired by default. Interactive wizard on a TTY;
    /// scripted JSON path via `--non-interactive`.
    Init(InitArgs),
    /// Reverse what `memoryd init` / `scripts/install-memorum.sh` set up: stop
    /// the daemon, remove the launchd plist, unwire MCP configs, and optionally
    /// purge the repo/runtime data. The clean exit. Scripted JSON path via
    /// `--non-interactive --json`; `--print-only` previews with zero side effects.
    Uninstall(UninstallArgs),
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Canonical Memorum repo root (default `$MEMORUM_REPO` or `~/memorum`).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Local per-device runtime directory (default `<repo>/.memoryd`).
    #[arg(long)]
    pub runtime: Option<PathBuf>,
    /// Run without prompts; drive teardown from flags and emit a
    /// machine-readable report. Suitable for CI and agent teardown.
    #[arg(long, default_value_t = false)]
    pub non_interactive: bool,
    /// Emit machine-readable JSON to stdout; diagnostics always go to stderr.
    #[arg(long, default_value_t = false)]
    pub json: bool,
    /// Plan and report every step without applying side effects.
    #[arg(long, default_value_t = false)]
    pub print_only: bool,
    /// Delete the repo and runtime directories. Without this flag, data is
    /// preserved and the purge step reports `skipped`.
    #[arg(long, default_value_t = false)]
    pub purge: bool,
    /// Harness configs to unwire. Omitted: all detected harnesses.
    #[arg(long, value_enum)]
    pub harness: Option<HarnessTargetArg>,
}

/// Harness target selection shared by every "which harness(es) does this flag
/// target" surface: `--harness`, `--wire-mcp`, `--wire-hooks` on `init`, and
/// `--harness` on `uninstall`. One conceptual type, one clap value surface
/// (`current`/`claude`/`codex`/`all`/`none`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum HarnessTargetArg {
    Current,
    Claude,
    Codex,
    All,
    None,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Canonical Memorum repo root (default `$MEMORUM_REPO` or `~/memorum`).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Local per-device runtime directory (default `<repo>/.memoryd`).
    #[arg(long)]
    pub runtime: Option<PathBuf>,
    /// Run without prompts; drive the setup engine from flags and emit a
    /// machine-readable report. Suitable for CI and agent bootstrap.
    #[arg(long, default_value_t = false)]
    pub non_interactive: bool,
    /// Emit machine-readable JSON to stdout. Implied by `--non-interactive`
    /// and `--detect-only`; diagnostics always go to stderr.
    #[arg(long, default_value_t = false)]
    pub json: bool,
    /// Run detection only: no decisions, no steps, zero mutation. Emits the
    /// detection summary as JSON and exits.
    #[arg(long, default_value_t = false)]
    pub detect_only: bool,
    /// Import detected harness memory through the daemon during setup. On a
    /// TTY this pre-answers the wizard's import prompt with "yes".
    #[arg(long, default_value_t = false)]
    pub import: bool,
    /// Harness set to import. Omitted: prompted by the wizard on a TTY;
    /// `current` (the single detected harness) on the non-interactive path.
    #[arg(long, value_enum)]
    pub harness: Option<HarnessTargetArg>,
    /// Default placement for imported memories whose cwd is not a git
    /// checkout. Mirrors `memoryd import --non-git-cwd-default`. Omitted:
    /// prompted by the wizard on a TTY; `project` (derive a project namespace;
    /// saved and active) on the non-interactive path.
    #[arg(long, value_enum)]
    pub non_git_cwd_default: Option<NonGitCwdDefault>,
    /// MCP configs to wire. The MCP bridge is an opt-in compatibility surface
    /// under the CLI-first design, so this is off by default. Omitted: prompted
    /// by the wizard on a TTY; `none` (skip MCP wiring) on the non-interactive
    /// path. Pass `current`/`claude`/`codex`/`all` to wire it explicitly.
    #[arg(long, value_enum)]
    pub wire_mcp: Option<HarnessTargetArg>,
    /// Harness configs to wire the passive-recall lifecycle hooks into.
    /// Omitted: prompted by the wizard on a TTY; `current` (the single detected
    /// harness) on the non-interactive path.
    #[arg(long, value_enum)]
    pub wire_hooks: Option<HarnessTargetArg>,
    /// Daemon arrangement to provision during setup. Omitted: prompted by the
    /// wizard on a TTY; `on-demand` on the non-interactive path.
    #[arg(long, value_enum)]
    pub daemon: Option<DaemonMode>,
    /// Plan and report every step without applying side effects (dry-run
    /// import, print-only MCP wiring).
    #[arg(long, default_value_t = false)]
    pub print_only: bool,
    /// Embedding lane to activate after setup completes.
    #[arg(long, value_enum)]
    pub embedding_lane: Option<EmbeddingLane>,
    /// Required acknowledgement before enabling the Gemini API lane in scripted mode.
    #[arg(long, default_value_t = false)]
    pub consent: bool,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    EmbeddingLane(EmbeddingLaneArgs),
    /// Configure daemon-scheduled harness auto-memory imports on this device.
    Harvest(HarvestArgs),
}
#[derive(Debug, Args)]
pub struct EmbeddingLaneArgs {
    #[arg(long, value_enum)]
    pub lane: EmbeddingLane,
    #[arg(long)]
    pub repo: Option<PathBuf>,
    #[arg(long)]
    pub runtime: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub consent: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum EmbeddingLane {
    Local,
    GeminiApi,
}

#[derive(Debug, Args)]
pub struct HarvestArgs {
    /// Canonical Memorum repo root (used to resolve the default runtime).
    #[arg(long, global = true)]
    pub repo: Option<PathBuf>,
    /// Local per-device runtime directory containing local-device.yaml.
    #[arg(long, global = true)]
    pub runtime: Option<PathBuf>,
    #[command(subcommand)]
    pub command: HarvestCommand,
}

#[derive(Debug, Subcommand)]
pub enum HarvestCommand {
    /// Enable scheduled imports, optionally changing their cadence.
    Enable(HarvestEnableArgs),
    /// Disable scheduled imports while preserving the configured cadence.
    Disable,
}

#[derive(Debug, Args)]
pub struct HarvestEnableArgs {
    /// Successful-run cadence in minutes (effective range 5 through 1440).
    #[arg(long)]
    pub interval_minutes: Option<u32>,
}

/// Daemon arrangement for `memoryd init --daemon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum DaemonMode {
    OnDemand,
    Background,
    Launchd,
    None,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Restrict the import to a single harness.
    #[arg(long, value_enum, default_value_t = ImportHarness::All)]
    pub harness: ImportHarness,
    /// Plan and report what would be written, without issuing any daemon
    /// requests or touching the state file.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
    /// Claude Code memory directory to import. Repeatable: pass `--from-claude`
    /// once per root to import the union of several. Omit to auto-detect the
    /// union of all Claude profile roots (the precedence root plus any sibling
    /// `~/.claude-*/projects/`).
    #[arg(long)]
    pub from_claude: Vec<PathBuf>,
    /// Override the Codex CLI memory directory (default: `~/.codex/memories/`).
    #[arg(long)]
    pub from_codex: Option<PathBuf>,
    /// Default placement for memories whose cwd is not a git checkout.
    ///
    /// Omit this flag for the safe default: interactive terminals prompt, while
    /// non-interactive callers derive a project namespace from the cwd path
    /// (`project`) so the memories are saved and land active — never skipped.
    #[arg(long, value_enum)]
    pub non_git_cwd_default: Option<NonGitCwdDefault>,
    /// Write a structured JSON report to this path.
    #[arg(long)]
    pub report: Option<PathBuf>,
    /// Suppress per-write progress lines (still emits the final summary).
    #[arg(long, default_value_t = false)]
    pub quiet: bool,
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Canonical Memorum repo root (state file lives at
    /// `<repo>/.memorum/import-state.json`). Defaults to `$MEMORUM_REPO` →
    /// `~/memorum`, matching `doctor`/`status` — never the caller's cwd.
    #[arg(long)]
    pub repo: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum ImportHarness {
    All,
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum NonGitCwdDefault {
    /// Drop these memories rather than importing them.
    Skip,
    /// Place them under user (`me`) scope.
    Me,
    /// Write a `.memory-project.yaml` in each non-git directory.
    Generate,
    /// Derive a project namespace for the cwd from its path; no file is
    /// written, and the memories land active and recall-visible by default.
    Project,
}

#[derive(Debug, Args)]
pub struct McpArgs {
    /// Unix socket path used to reach memoryd. Defaults to the canonical client socket.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Canonical memory repository root used when auto-starting memoryd (default `$MEMORUM_REPO` or `~/memorum`).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Local per-device runtime root used for socket resolution and auto-start (default `<repo>/.memoryd`).
    #[arg(long)]
    pub runtime: Option<PathBuf>,
    /// Auto-start memoryd when the resolved socket is absent.
    #[arg(long, default_value_t = false, action = ArgAction::Set)]
    pub auto_start: bool,
    /// Expose memory_reveal over the stdio MCP bridge.
    ///
    /// Leave disabled for normal agent dogfood: memory_get/search/startup are safe preview
    /// paths, while reveal returns decrypted encrypted content and should be explicitly
    /// enabled only for a harness/session that has user-directed reveal authority.
    #[arg(long, action = ArgAction::SetTrue)]
    pub allow_reveal: bool,
}

#[derive(Debug, Args)]
pub struct SocketArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct UiArgs {
    /// Start with panel N active.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=9))]
    pub panel: u8,
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Localhost port for the dashboard.
    #[arg(long, default_value_t = 7137, value_parser = clap::value_parser!(u16).range(1024..=65535))]
    pub port: u16,
}

#[derive(Debug, Args)]
pub struct WebStatusArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// ISO date (YYYY-MM-DD) to snooze until.
    #[arg(long)]
    pub until: Option<String>,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Unix socket path used to accept memoryd clients.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Initialize the substrate if it has not been bootstrapped yet.
    #[arg(long)]
    pub init: bool,
    /// Opt into best-effort durability during init. Intended for CI/tests only.
    #[arg(long)]
    pub force_unsafe_durability: bool,
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
pub struct DoctorArgs {
    /// Canonical memory repository root (default `$MEMORUM_REPO` or `~/memorum`).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Local per-device runtime directory (default `<repo>/.memoryd`).
    #[arg(long)]
    pub runtime: Option<PathBuf>,
    /// Rebuild the derived SQLite event-log mirror from canonical JSONL events before reporting health.
    #[arg(long)]
    pub reindex: bool,
    /// Accepted for command symmetry with the socket-backed subcommands; ignored — `doctor`
    /// inspects the substrate in-process and does not talk to the daemon.
    #[arg(long)]
    pub socket: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct SchemaArgs {
    /// Which slice of the contract to print. Defaults to the whole contract.
    #[arg(value_enum, default_value_t = SchemaSection::All)]
    pub section: SchemaSection,
    /// Emit JSON. The contract is machine-facing, so output is always JSON;
    /// this flag is accepted for explicitness and forward compatibility.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum SchemaSection {
    All,
    Commands,
    Envelope,
    ExitCodes,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Query text.
    pub query: String,
    /// Maximum number of results to return.
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    /// Include full bodies instead of bounded summaries.
    #[arg(long)]
    pub include_body: bool,
    /// Search every namespace instead of scoping to the current directory's
    /// visible set (me + project + agent).
    #[arg(long)]
    pub all_namespaces: bool,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Memory id to read.
    pub id: String,
    /// Include provenance details when available.
    #[arg(long)]
    pub include_provenance: bool,
}

#[derive(Debug, Args)]
pub struct WriteNoteArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Optional semantic metadata (`abstraction`, `cues`) as JSON.
    #[arg(long)]
    pub meta: Option<String>,
    /// Note text.
    pub text: String,
}

#[derive(Debug, Args)]
pub struct WriteMemoryArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
pub struct SourceArgs {
    #[command(subcommand)]
    pub command: SourceCommand,
}

#[derive(Debug, Subcommand)]
pub enum SourceCommand {
    /// Capture a public HTTP(S) page and return webcap refs for exact excerpts.
    Capture(SourceCaptureArgs),
}

#[derive(Debug, Args)]
pub struct SourceCaptureArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Public HTTP(S) URL to capture.
    #[arg(long)]
    pub url: Option<String>,
    /// Local text/html artifact to capture.
    #[arg(long = "file")]
    pub file: Option<PathBuf>,
    /// Source capture mode.
    #[arg(long, value_enum, default_value_t = SourceCaptureCliMode::HttpStatic)]
    pub mode: SourceCaptureCliMode,
    /// Exact quote to anchor in extracted page text. Repeat for multiple quotes.
    #[arg(long = "excerpt")]
    pub excerpts: Vec<String>,
    /// Optional safe operator note.
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SourceCaptureCliMode {
    HttpStatic,
    LocalArtifact,
    PdfText,
    BrowserRendered,
    Screenshot,
    Authenticated,
}

#[derive(Debug, Args)]
pub struct RevealArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Encrypted memory id to unmask.
    pub id: String,
    /// Audit reason persisted (redacted) into the event log. Required.
    #[arg(long)]
    pub reason: String,
    /// Acknowledge that reveal decrypts protected content and writes an audit
    /// event. Without this flag the CLI refuses before contacting the daemon.
    #[arg(long, action = ArgAction::SetTrue)]
    pub allow_reveal: bool,
}

#[derive(Debug, Args)]
pub struct ObserveArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Observation text (bounded to 16 KiB).
    pub text: String,
    /// Observation kind.
    #[arg(long, value_enum)]
    pub kind: ObserveKindArg,
    /// Bound entity id (`ent_*`). Repeatable; up to 32, each ≤128 bytes.
    #[arg(long = "entity")]
    pub entities: Vec<String>,
    /// Session id to attribute the observation to. Defaults to `$MEMORUM_SESSION_ID`,
    /// else `cli`.
    #[arg(long)]
    pub session_id: Option<String>,
    /// Harness to attribute the observation to. Defaults to `$MEMORUM_HARNESS`,
    /// else `cli`.
    #[arg(long)]
    pub harness: Option<String>,
}

/// CLI surface for `memory_substrate::ObserveKind`. Kept as a distinct enum so
/// the substrate type needs no clap dependency; mapped in `cli::memory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum ObserveKindArg {
    Observation,
    Pattern,
    Signal,
}

impl ObserveKindArg {
    pub fn to_protocol(self) -> crate::protocol::ObserveKind {
        use crate::protocol::ObserveKind;
        match self {
            Self::Observation => ObserveKind::Observation,
            Self::Pattern => ObserveKind::Pattern,
            Self::Signal => ObserveKind::Signal,
        }
    }
}

#[derive(Debug, Args)]
pub struct SupersedeArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    /// Review device-local merge proposals.
    Merges(ReviewMergesArgs),
}

#[derive(Debug, Args)]
pub struct ReviewMergesArgs {
    #[command(subcommand)]
    pub command: ReviewMergesCommand,
}

#[derive(Debug, Subcommand)]
pub enum ReviewMergesCommand {
    /// List merge proposals.
    List(SocketArgs),
    /// Approve and apply a merge proposal.
    Approve(ReviewMergeApproveArgs),
    /// Reject a merge proposal.
    Reject(ReviewMergeRejectArgs),
}

#[derive(Debug, Args)]
pub struct ReviewMergeApproveArgs {
    #[arg(long)]
    pub socket: Option<PathBuf>,
    pub proposal_id: String,
    #[arg(long = "approve-pinned")]
    pub approve_pinned: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ReviewMergeRejectArgs {
    #[arg(long)]
    pub socket: Option<PathBuf>,
    pub proposal_id: String,
}

#[derive(Debug, Args)]
pub struct ReviewQueueArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Maximum number of review items to return.
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct ReviewApproveArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Memory id to approve.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct ReviewRejectArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Reason for rejecting the memory.
    #[arg(long)]
    pub reason: String,
    /// Memory id to reject.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct QuarantineArgs {
    #[command(subcommand)]
    pub command: QuarantineCommand,
}

#[derive(Debug, Subcommand)]
pub enum QuarantineCommand {
    /// List quarantined memories blocking sync.
    List(QuarantineListArgs),
    /// Resolve one quarantined memory after operator review.
    Resolve(QuarantineResolveArgs),
}

#[derive(Debug, Args)]
pub struct QuarantineListArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Maximum number of quarantined memories to return.
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct QuarantineResolveArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    /// Memory id to resolve.
    pub id: String,
    /// Confirm the conflict was resolved by editing the canonical file by hand.
    ///
    /// This is the only supported resolution: the daemon promotes the current
    /// on-disk body to Active/Trusted after checking it no longer carries git
    /// conflict markers. Side-selection ("accept ours/theirs") is not yet
    /// supported — the substrate has no side-swap API — so resolve the file
    /// manually first, then run this.
    #[arg(long, action = ArgAction::SetTrue, required = true)]
    pub edited: bool,
}

impl QuarantineResolveArgs {
    pub fn mode(&self) -> Option<QuarantineResolutionMode> {
        // Hand-resolution is the only mode the daemon can honor; `--edited` is the
        // operator's explicit acknowledgement and does not change the (single)
        // resolution path.
        self.edited.then_some(QuarantineResolutionMode::Edited)
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser as _;

    #[test]
    fn quarantine_resolve_requires_edited_acknowledgement() {
        let id = "mem_20260508_a1b2c3d4e5f60718_000001";

        assert!(Cli::try_parse_from(["memoryd", "quarantine", "resolve", id]).is_err());
        Cli::try_parse_from(["memoryd", "quarantine", "resolve", "--edited", id])
            .expect("--edited quarantine resolve parses");
    }
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
    #[arg(long)]
    pub socket: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct PeerActivityArgs {
    /// Unix socket path used to reach memoryd.
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    #[arg(long)]
    pub socket: Option<PathBuf>,
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
    /// Report review-decision calibration: accept-rate per confidence decile.
    Calibration(DreamCalibrationArgs),
    /// Compile semantic abstractions/cues for missing or stale memories.
    AbstractionCompile(DreamAbstractionCompileArgs),
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
pub struct DreamCalibrationArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root. Accepted for command symmetry; the report
    /// reads git-synced per-device calibration logs, not runtime state.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Emit a structured JSON CalibrationReport instead of the human table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DreamAbstractionCompileArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Harness CLI override; unavailable harnesses use structural fallback.
    #[arg(long = "cli")]
    pub cli_override: Option<String>,
    /// Maximum memories to process.
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
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
    /// Unified passive-recall hook handler invoked by harness lifecycle events.
    ///
    /// Reads one hook-invocation JSON object on stdin, dispatches on
    /// `hook_event_name`, calls the daemon under a hard deadline, and emits the
    /// recall block wrapped in `hookSpecificOutput.additionalContext`. Fail-open
    /// is absolute: any failure produces zero bytes, no stderr, and exit 0 — it
    /// must never block the agent. Shares no exit path with `StartupBlock`/
    /// `DeltaBlock` (those exit nonzero on error).
    Hook(RecallHookArgs),
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
pub struct RecallHookArgs {
    /// Daemon socket plus the `--runtime` fallback used to resolve it. The
    /// installer always passes `--socket` explicitly; the runtime default is a
    /// backstop so the handler resolves the same way the other recall
    /// subcommands do.
    #[command(flatten)]
    pub socket: RecallSocketArgs,
    /// Canonical harness id, exactly `claude-code` or `codex`. Passed verbatim
    /// into the recall request's `harness` field.
    #[arg(long)]
    pub harness: String,
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
        crate::paths::resolve_socket_arg(&args.socket).as_os_str().to_owned(),
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
        WebCommand::Enable(args) => RequestPayload::WebEnable {
            port: args.port,
            socket_path: crate::paths::resolve_socket_arg(&args.socket).to_string_lossy().into_owned(),
        },
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

// Per-command runners — populated by the 2026-05-28 main.rs refactor.
pub mod config;
pub mod daemon;
pub mod dream;
pub(crate) mod exit;
pub mod import;
pub mod init;
pub mod memory;
pub mod output;
pub mod peer;
pub mod peer_render;
pub mod privacy;
pub mod quarantine;
pub mod reality_check;
pub mod recall;
pub mod recall_hook;
pub mod review;
pub mod schema;
pub mod serve;
pub mod source;
pub mod ui;
pub mod uninstall;
pub mod web;
