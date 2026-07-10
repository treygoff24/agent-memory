//! Plain data shapes for the importer pipeline: caller options, planned
//! actions, the plan output, and the discovery summary. Moved verbatim from the
//! former single-file `pipeline.rs`; no logic lives here.

use std::path::PathBuf;

use crate::import::candidate::{Harness, ParsedMemory};
use crate::import::discovery::{ClaudeMemoryRoot, CodexMemoryRoot};
use crate::import::project_map::ScopeBinding;
use crate::import::state::ImportState;
use crate::import::ImportError;

/// Caller-supplied options for the planning phase.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Explicit Claude memory roots (the repeatable `--from-claude` flag). An
    /// empty vec means auto-detect the union of profile roots; a non-empty vec
    /// is honored verbatim, in order, with no scanning.
    pub from_claude: Vec<PathBuf>,
    /// Override the Codex memory root (`--from-codex`).
    pub from_codex: Option<PathBuf>,
    /// Restrict planning to a single harness; `None` means import everything.
    pub harness_filter: Option<HarnessFilter>,
    /// Suppress the per-root discovery summary that `plan` prints to stderr.
    /// Mirrors the CLI `--quiet` flag; defaults to noisy (`false`).
    pub quiet: bool,
    /// Pre-loaded state for idempotency checks. Disk-backed imports should go
    /// through [`run_import_session`] so the state file is loaded under the
    /// import lock.
    ///
    /// [`run_import_session`]: super::run_import_session
    pub state: ImportState,
}

/// `--harness claude|codex|all` selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessFilter {
    Claude,
    Codex,
}

impl HarnessFilter {
    /// Whether this filter accepts the given harness. T06 uses this to drop
    /// post-parse candidates that aren't in scope when the user has restricted
    /// the run; T05's planner already short-circuits at discovery so this
    /// helper is also useful for downstream report rendering.
    pub fn includes(self, harness: Harness) -> bool {
        matches!((self, harness), (Self::Claude, Harness::ClaudeCode) | (Self::Codex, Harness::Codex))
    }
}

/// One topologically-ordered write action for the execute phase.
#[derive(Debug, Clone)]
pub struct PlannedWrite {
    pub source_key: String,
    pub candidate: ParsedMemory,
    pub scope: ScopeBinding,
    pub action: PlanAction,
    /// Wiki-link aliases that the topo sort resolved against later writes.
    /// These become `related: [memory_id]` once the target write completes.
    pub wiki_link_targets_resolvable: Vec<String>,
    /// Wiki-link aliases that form a back-edge in the source-key ordering and
    /// will be left as inert `[[name]]` text in the body (per the
    /// single-pass topological-ordering decision).
    pub wiki_link_targets_back_edge: Vec<String>,
}

/// Per-source action that the execute phase will perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    /// State file already records this source with a matching content hash —
    /// skip. `existing_record_key` is the `state.imports` map key for the
    /// matching record, used to migrate legacy `source_key` entries to the
    /// stable `source_identity` key.
    SkipUnchanged { existing_memory_id: String, existing_record_key: String },
    /// State file records this exact source/content under a different bucket —
    /// supersede the prior memory with identical content in the correct bucket.
    RepairBucket { prior_memory_id: String, prior_content_hash: String, prior_record_key: String },
    /// State file records this source under a different content hash —
    /// supersede the prior memory.
    Supersede { prior_memory_id: String, prior_content_hash: String, prior_record_key: String },
    /// First time we've seen this source — write a fresh memory.
    WriteNew,
    /// Project mapper resolved to "skip" for this candidate's cwd.
    SkipByPrompt,
    /// Historical state contained multiple possible predecessors. Never guess
    /// which one to supersede; surface it and leave canonical state untouched.
    ReportAmbiguous { matching_memory_ids: Vec<String> },
}

/// A back-edge wiki link that the topo sort had to break. Surfaced in the
/// import report for transparency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLinkBackEdge {
    pub source_key: String,
    pub alias: String,
}

/// Output of the planning phase. The execute phase walks `actions` in order,
/// each one ready to be turned into a single daemon request.
#[derive(Debug)]
pub struct ImportPlan {
    pub actions: Vec<PlannedWrite>,
    pub source_discovery_summary: DiscoverySummary,
    pub unresolved_back_edges: Vec<WikiLinkBackEdge>,
    pub parse_errors: Vec<ImportError>,
    /// Source keys whose malformed frontmatter the Claude parser lenient-recovered
    /// rather than dropping. Threaded onto the report so the operator can see
    /// which memories imported with best-effort frontmatter. Same plumbing as
    /// `parse_errors`: collected during the parse pass, copied in `from_plan`.
    pub frontmatter_recovered: Vec<String>,
    /// The Claude profile roots this plan actually parsed, as string paths.
    /// Empty when only Codex was in scope or no roots were discovered.
    pub claude_roots_used: Vec<String>,
    pub state: ImportState,
}

/// Summary of where the parsers read from. Surfaced in the report so the user
/// can see which precedence rung each root came from.
#[derive(Debug, Clone, Default)]
pub struct DiscoverySummary {
    pub claude_root: Option<ClaudeMemoryRoot>,
    pub codex_root: Option<CodexMemoryRoot>,
    pub claude_candidates: usize,
    pub codex_candidates: usize,
}
