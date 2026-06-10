//! Quality-metrics runner for the golden recall corpus (Task 4.2).
//!
//! This is the *measuring instrument* for the dynamics program. It loads the
//! hand-curated golden corpus (`fixtures/golden/`) into a real scratch
//! `Substrate`, replays each labeled query through the **real** recall ranking
//! code — not a reimplementation — and computes graded retrieval metrics
//! against the committed baseline.
//!
//! ## The two ranking seams (both real, no reimplementation)
//!
//! 1. **`memory_search` path.** [`Substrate::query_chunks`] is the exact bm25 FTS
//!    engine the daemon's `search_response` handler calls
//!    (`memoryd::handlers::memory_ops::search_response`). We dedup its chunk hits
//!    down to a per-memory ranking, preserving the engine's score order.
//!
//! 2. **Startup-block assembly path.** The startup recall builder
//!    (`memoryd::recall::startup`) selects candidates via
//!    [`collect_recall_candidates_from_index`] and ranks them via
//!    [`select_ranked_candidates`] (the points-based ranking in
//!    `recall/rank.rs`). We invoke those two public functions directly with the
//!    query case's `namespace_scope`, which is precisely how the startup builder
//!    drives them internally — only the namespace set comes from the labeled
//!    case rather than a cwd-derived session binding.
//!
//! This is FTS + structural-points ranking only. The production vector lane
//! (Tasks 3.0/3.2) exists, but it serves write-path contradiction detection —
//! recall ranking does not vector-search yet. So these numbers are the honest
//! bm25 + points baseline; vector recall lands as a separate, later regression.
//!
//! ## Metrics
//!
//! Per non-abstention case, per seam: precision@K, recall@K, MRR, nDCG. Graded
//! gains: `essential = 2`, `useful = 1`, traps and everything-else `= 0`. nDCG
//! is the headline. We also track **trap-rate@5** — the fraction of cases with a
//! labeled `irrelevant_trap` surfacing in the top 5 — because that metric
//! encodes the superseded/wrong-project lookalike failure mode the embedding
//! model was selected on.
//!
//! Abstention cases (empty `essential` + `useful`) are reported *separately* so
//! their (undefined) recall/nDCG never poison the headline averages. For those
//! we report what surfaced and a `surfaced_trap` flag.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use memory_substrate::{ChunkQuery, InitOptions, Roots, Substrate};
use memoryd::recall::{
    collect_recall_candidates_from_index, select_ranked_candidates, CandidateCollection, RankingContext,
    RecallCollectionRequest, RecallSectionName,
};
use serde::{Deserialize, Serialize};

/// K cutoffs the runner reports metrics at. Trap-rate is always reported at 5.
pub const K_VALUES: [usize; 3] = [3, 5, 10];
const TRAP_K: usize = 5;
/// Quality-gate runs are deliberately structural-only. Do not read
/// `MEMORUM_DYNAMICS` here: if a future dynamics lane is needed, add an explicit
/// runner flag and a separate baseline rather than letting ambient env change
/// the gate.
const QUALITY_GATE_ALPHA_POINTS: u32 = 0;

/// Graded relevance gain used for nDCG and the essential-recall floor.
const GAIN_ESSENTIAL: f64 = 2.0;
const GAIN_USEFUL: f64 = 1.0;

/// Default tolerance band for the regression gate: a seam-level nDCG@5 drop
/// larger than this (absolute) fails the gate. Trap-rate@5 is allowed to *rise*
/// by at most this much. Both are deliberately small — the corpus is
/// deterministic, so any movement is a real ranking change, not noise.
pub const DEFAULT_TOLERANCE: f64 = 0.02;

// ---------------------------------------------------------------------------
// Corpus + query model
// ---------------------------------------------------------------------------

/// One labeled query case from `queries.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryCase {
    pub id: String,
    pub query: String,
    pub namespace_scope: Vec<String>,
    pub graded: Graded,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Graded {
    #[serde(default)]
    pub essential: Vec<String>,
    #[serde(default)]
    pub useful: Vec<String>,
    #[serde(default)]
    pub irrelevant_traps: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct QueryFile {
    cases: Vec<QueryCase>,
}

impl QueryCase {
    /// A case is an abstention case when nothing is relevant — empty essential
    /// AND useful. The correct behavior is to surface nothing relevant; recall
    /// and nDCG are undefined, so these are scored separately.
    pub fn is_abstention(&self) -> bool {
        self.graded.essential.is_empty() && self.graded.useful.is_empty()
    }

    /// Graded gain for a memory id: essential=2, useful=1, otherwise 0 (traps
    /// and unrelated alike — a trap surfacing is simply a zero-gain hit that
    /// also trips the trap-rate metric).
    fn gain(&self, id: &str) -> f64 {
        if self.graded.essential.iter().any(|e| e == id) {
            GAIN_ESSENTIAL
        } else if self.graded.useful.iter().any(|u| u == id) {
            GAIN_USEFUL
        } else {
            0.0
        }
    }

    fn is_trap(&self, id: &str) -> bool {
        self.graded.irrelevant_traps.iter().any(|t| t == id)
    }

    fn relevant_ids(&self) -> BTreeSet<&str> {
        self.graded.essential.iter().chain(self.graded.useful.iter()).map(String::as_str).collect()
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors raised while assembling or running the quality corpus.
#[derive(Debug)]
pub enum QualityError {
    /// The golden corpus directory was missing or unreadable.
    CorpusMissing(String),
    /// An IO error while staging the corpus on disk.
    Io(std::io::Error),
    /// Failed to parse `queries.yaml`.
    Queries(String),
    /// A substrate open/init/query failure.
    Substrate(String),
}

impl std::fmt::Display for QualityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CorpusMissing(m) => write!(f, "golden corpus missing: {m}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Queries(m) => write!(f, "queries.yaml: {m}"),
            Self::Substrate(m) => write!(f, "substrate: {m}"),
        }
    }
}

impl std::error::Error for QualityError {}

impl From<std::io::Error> for QualityError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Corpus harness — a real Substrate populated from the golden tree
// ---------------------------------------------------------------------------

/// A scratch substrate with the golden corpus indexed through the real reindex
/// path, plus the project-alias → canonical-namespace-id map needed to resolve
/// `project:<alias>` query scopes onto the recall index's
/// `project:<canonical_id>` namespace filter.
pub struct GoldenCorpus {
    substrate: Substrate,
    /// `atlas` -> `proj_2170411deb73`, etc., derived from the corpus frontmatter.
    alias_to_canonical: BTreeMap<String, String>,
    _temp: tempfile::TempDir,
}

impl GoldenCorpus {
    /// Locate the golden corpus fixtures directory shipped with this crate.
    pub fn fixtures_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/golden")
    }

    /// Stage the golden corpus into a fresh scratch substrate and index it.
    ///
    /// The corpus files already live in the canonical spec-§6 tree layout
    /// (`me/`, `projects/`, `agent/`), so we copy the tree verbatim into the
    /// repo root and let [`Substrate::init`] index it via the real reindex
    /// path — no re-serialization, no synthetic write path. What gets indexed
    /// (FTS body inclusion, encrypted/metadata-only projection, status) is
    /// exactly what the production indexer would produce for these files.
    pub async fn load() -> Result<Self, QualityError> {
        Self::load_from_root(&Self::fixtures_root()).await
    }

    /// [`Self::load`] against an arbitrary corpus root (`<root>/memories` +
    /// `<root>/queries.yaml`). Powers bring-your-own-corpus runs — e.g. a
    /// private, machine-local corpus distilled from real projects. The
    /// regression gate never uses this; it stays pinned to the committed
    /// fixtures via [`Self::load`].
    pub async fn load_from_root(root: &Path) -> Result<Self, QualityError> {
        let memories_src = root.join("memories");
        if !memories_src.is_dir() {
            return Err(QualityError::CorpusMissing(memories_src.display().to_string()));
        }

        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        fs::create_dir_all(&repo)?;

        // Copy the canonical subtrees (me/, projects/, agent/) to the repo root
        // verbatim — including `supersedes:` edges. The substrate's bulk reindex
        // FK-guards each supersession edge and runs a deferred resync pass, so a
        // supersessor indexed before its target no longer aborts the load.
        copy_tree(&memories_src, &repo)?;

        let alias_to_canonical = derive_alias_map(&memories_src)?;

        let roots = Roots::new(repo, runtime);
        let substrate = Substrate::init(
            roots,
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_qualityeval01".to_string()) },
        )
        .await
        .map_err(|e| QualityError::Substrate(format!("init: {e:?}")))?;

        Ok(Self { substrate, alias_to_canonical, _temp: temp })
    }

    /// Load and parse `queries.yaml` into labeled cases.
    pub fn load_queries() -> Result<Vec<QueryCase>, QualityError> {
        Self::load_queries_from_root(&Self::fixtures_root())
    }

    /// [`Self::load_queries`] against an arbitrary corpus root.
    pub fn load_queries_from_root(root: &Path) -> Result<Vec<QueryCase>, QualityError> {
        let path = root.join("queries.yaml");
        let text = fs::read_to_string(&path)?;
        let parsed: QueryFile = serde_yaml::from_str(&text).map_err(|e| QualityError::Queries(e.to_string()))?;
        Ok(parsed.cases)
    }

    /// Map a query case's `namespace_scope` entry onto the recall index's
    /// namespace-prefix grammar (`me` / `agent` / `project:<canonical_id>`).
    ///
    /// Returns `None` for an unresolved alias (skipped rather than silently
    /// widening scope). Bare `project` means "all projects": there is no single
    /// canonical id, so it expands to one prefix per known project.
    fn resolve_namespace_prefixes(&self, scope: &[String]) -> Vec<String> {
        let mut prefixes = Vec::new();
        for raw in scope {
            match raw.as_str() {
                "me" => prefixes.push("me".to_string()),
                "agent" => prefixes.push("agent".to_string()),
                "project" => {
                    // Whole project namespace in scope: every known project.
                    for canonical in self.alias_to_canonical.values() {
                        prefixes.push(format!("project:{canonical}"));
                    }
                }
                other if other.starts_with("project:") => {
                    let alias = other.trim_start_matches("project:");
                    if let Some(canonical) = self.alias_to_canonical.get(alias) {
                        prefixes.push(format!("project:{canonical}"));
                    }
                }
                _ => {}
            }
        }
        prefixes.sort();
        prefixes.dedup();
        prefixes
    }

    /// **memory_search seam.** Run the query through the real bm25 FTS engine
    /// (`Substrate::query_chunks`) and reduce chunk hits to a per-memory
    /// ranking, preserving the engine's score order (first chunk wins per
    /// memory). This is the exact engine the daemon's search handler uses.
    async fn rank_via_search(&self, query: &str) -> Result<Vec<String>, QualityError> {
        let chunks = self
            .substrate
            .query_chunks(ChunkQuery { text: Some(query.to_string()), triple: None, vector: None })
            .await
            .map_err(|e| QualityError::Substrate(format!("query_chunks: {e:?}")))?;

        let mut seen = BTreeSet::new();
        let mut ranked = Vec::new();
        for chunk in chunks {
            let id = chunk.memory_id.as_str().to_string();
            if seen.insert(id.clone()) {
                ranked.push(id);
            }
        }
        Ok(ranked)
    }

    /// **Startup-block assembly seam.** Select candidates by calling the same
    /// recall-index collection function the startup builder uses, then rank them
    /// via the real `select_ranked_candidates` points ranking. The namespace set
    /// is the query case's `namespace_scope`, exactly as the startup builder
    /// would pass `session_binding.namespaces_in_scope`.
    async fn rank_via_startup(&self, scope: &[String]) -> Result<Vec<String>, QualityError> {
        let prefixes = self.resolve_namespace_prefixes(scope);

        let CandidateCollection { facts, .. } = collect_recall_candidates_from_index(
            &self.substrate,
            RecallCollectionRequest {
                section: RecallSectionName::RecentMemory,
                namespace_prefixes: prefixes.clone(),
                updated_since: None,
            },
        )
        .await
        .map_err(|e| QualityError::Substrate(format!("collect_recall_candidates_from_index: {e:?}")))?;

        // Ranking context: `now` = newest candidate (matches the startup
        // builder's `ranking_now`); single-project scope sets the exact-project
        // bonus, multi-scope leaves it None.
        let now = facts.iter().map(|c| c.row.updated_at).max().unwrap_or_default();
        let exact_project_namespace = single_project_canonical(&prefixes);

        let context = RankingContext { now, exact_project_namespace, alpha_points: QUALITY_GATE_ALPHA_POINTS };

        // Large budget so token truncation never confounds *ranking* quality —
        // we are measuring order, not the startup token cap. Selection still
        // runs the real budget loop; the cap is just generous.
        let selection = select_ranked_candidates(RecallSectionName::RecentMemory, facts, context, usize::MAX);
        Ok(selection.selected.into_iter().map(|c| c.id).collect())
    }
}

/// When exactly one project prefix is in scope, return its canonical id so the
/// ranker can award the exact-project bonus. Otherwise `None`.
fn single_project_canonical(prefixes: &[String]) -> Option<String> {
    let mut projects = prefixes.iter().filter_map(|p| p.strip_prefix("project:"));
    let first = projects.next()?;
    if projects.next().is_some() {
        return None;
    }
    Some(first.to_string())
}

/// Recursively copy `src` directory contents into `dst` verbatim.
///
/// Memory `.md` files are copied byte-identical (including their `supersedes:`
/// edge lists): the substrate's bulk reindex FK-guards each supersession edge
/// and runs a deferred resync, so a supersessor indexed before its target no
/// longer aborts the load.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), QualityError> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            fs::create_dir_all(&to)?;
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Build the `alias -> canonical_namespace_id` map by reading each project
/// memory's frontmatter (`namespace: <alias>/<sub>` + `canonical_namespace_id`).
fn derive_alias_map(memories_src: &Path) -> Result<BTreeMap<String, String>, QualityError> {
    let projects_dir = memories_src.join("projects");
    let mut map = BTreeMap::new();
    let mut files = Vec::new();
    collect_markdown(&projects_dir, &mut files);
    for path in files {
        let text = fs::read_to_string(&path)?;
        let (mut namespace, mut canonical) = (None, None);
        for line in text.lines() {
            if line == "---" && namespace.is_some() {
                break; // out of frontmatter
            }
            if let Some(rest) = line.strip_prefix("namespace:") {
                namespace = Some(rest.trim().trim_matches('"').to_string());
            } else if let Some(rest) = line.strip_prefix("canonical_namespace_id:") {
                canonical = Some(rest.trim().trim_matches('"').to_string());
            }
        }
        if let (Some(ns), Some(canonical)) = (namespace, canonical) {
            // alias is the first path segment of `atlas/billing`.
            let alias = ns.split('/').next().unwrap_or(&ns).to_string();
            map.entry(alias).or_insert(canonical);
        }
    }
    Ok(map)
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Aggregated metrics for one ranking seam over the non-abstention cases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeamMetrics {
    /// Number of non-abstention (scored) cases.
    pub scored_cases: usize,
    /// precision@K, keyed by K (as a string for stable JSON).
    pub precision_at_k: BTreeMap<String, f64>,
    /// recall@K, keyed by K.
    pub recall_at_k: BTreeMap<String, f64>,
    /// Mean reciprocal rank of the first relevant hit.
    pub mrr: f64,
    /// nDCG@K (graded), keyed by K. nDCG@5 is the headline.
    pub ndcg_at_k: BTreeMap<String, f64>,
    /// Fraction of scored cases that surfaced a labeled trap in the top-5.
    pub trap_rate_at_5: f64,
}

/// What an abstention case surfaced — reported separately, never averaged into
/// the headline metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AbstentionOutcome {
    pub case_id: String,
    pub seam: String,
    /// Number of memories the seam surfaced (ideally low — nothing is relevant).
    pub surfaced_count: usize,
    /// First few surfaced ids, for human triage of false positives.
    pub surfaced_top: Vec<String>,
    /// Whether a labeled trap surfaced in the top-5 (a precision failure).
    pub surfaced_trap_at_5: bool,
}

/// The full quality report. Serializes to the JSON the baseline gate compares.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityReport {
    /// Schema version of this report shape.
    pub schema: u32,
    /// Ranking lane note — bm25 + structural points today, no vector lane.
    pub ranking_lane: String,
    /// Total labeled cases.
    pub total_cases: usize,
    /// Abstention case count.
    pub abstention_cases: usize,
    /// Per-seam aggregated metrics (keys: `search`, `startup`).
    pub seams: BTreeMap<String, SeamMetrics>,
    /// Per-seam interpretation notes (what the seam exercises, caveats).
    pub seam_notes: BTreeMap<String, String>,
    /// Per-abstention-case outcomes (both seams).
    pub abstentions: Vec<AbstentionOutcome>,
}

/// Accumulator while sweeping the scored cases for one seam.
#[derive(Default)]
struct SeamAccumulator {
    scored: usize,
    precision: BTreeMap<usize, f64>,
    recall: BTreeMap<usize, f64>,
    ndcg: BTreeMap<usize, f64>,
    mrr_sum: f64,
    trap_hits: usize,
}

impl SeamAccumulator {
    fn add_case(&mut self, case: &QueryCase, ranked: &[String]) {
        self.scored += 1;
        let relevant = case.relevant_ids();

        for &k in &K_VALUES {
            let topk = &ranked[..ranked.len().min(k)];
            // precision@K: relevant hits in top-K / K (the standard denominator
            // is K, not min(K, |ranked|) — surfacing fewer than K is itself a
            // precision signal here).
            let hits = topk.iter().filter(|id| relevant.contains(id.as_str())).count();
            *self.precision.entry(k).or_default() += hits as f64 / k as f64;

            // recall@K: relevant hits in top-K / |relevant|. Non-abstention
            // cases always have >=1 relevant id, so the denominator is nonzero.
            *self.recall.entry(k).or_default() += hits as f64 / relevant.len().max(1) as f64;

            // nDCG@K with graded gains and an ideal ranking of the labels.
            *self.ndcg.entry(k).or_default() += ndcg_at_k(case, ranked, k);
        }

        // MRR: reciprocal rank of the first relevant hit (0 if none surface).
        if let Some(pos) = ranked.iter().position(|id| relevant.contains(id.as_str())) {
            self.mrr_sum += 1.0 / (pos as f64 + 1.0);
        }

        // trap-rate@5: did any labeled trap surface in the top-5?
        let top5 = &ranked[..ranked.len().min(TRAP_K)];
        if top5.iter().any(|id| case.is_trap(id)) {
            self.trap_hits += 1;
        }
    }

    fn finish(self) -> SeamMetrics {
        let n = self.scored.max(1) as f64;
        let avg = |m: BTreeMap<usize, f64>| -> BTreeMap<String, f64> {
            m.into_iter().map(|(k, v)| (k.to_string(), v / n)).collect()
        };
        SeamMetrics {
            scored_cases: self.scored,
            precision_at_k: avg(self.precision),
            recall_at_k: avg(self.recall),
            mrr: self.mrr_sum / n,
            ndcg_at_k: avg(self.ndcg),
            trap_rate_at_5: self.trap_hits as f64 / n,
        }
    }
}

/// nDCG@K for one case against one ranked id list, using graded gains and the
/// ideal ranking of the case's labels.
fn ndcg_at_k(case: &QueryCase, ranked: &[String], k: usize) -> f64 {
    let dcg: f64 = ranked.iter().take(k).enumerate().map(|(i, id)| dcg_term(case.gain(id), i)).sum();

    // Ideal DCG: place all graded gains in descending order.
    let mut ideal_gains: Vec<f64> = case
        .graded
        .essential
        .iter()
        .map(|_| GAIN_ESSENTIAL)
        .chain(case.graded.useful.iter().map(|_| GAIN_USEFUL))
        .collect();
    ideal_gains.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let idcg: f64 = ideal_gains.iter().take(k).enumerate().map(|(i, &g)| dcg_term(g, i)).sum();

    if idcg == 0.0 {
        0.0
    } else {
        dcg / idcg
    }
}

fn dcg_term(gain: f64, rank_index: usize) -> f64 {
    // Standard log2(rank+2) discount (rank_index is 0-based).
    gain / ((rank_index as f64 + 2.0).log2())
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Run both ranking seams over the golden corpus and produce the full report.
pub async fn run_quality_report() -> Result<QualityReport, QualityError> {
    Ok(run_quality_report_with_cases_for_root(&GoldenCorpus::fixtures_root()).await?.0)
}

/// Per-case outcome detail emitted alongside the aggregate report — what each
/// seam actually surfaced (top [`CASE_DUMP_TOP_K`]) next to the case's answer
/// key. Powers `--dump-cases` and human corpus review tooling; never read by
/// the regression gate.
#[derive(Debug, Clone, Serialize)]
pub struct CaseOutcome {
    pub id: String,
    pub query: String,
    pub scope: Vec<String>,
    pub essential: Vec<String>,
    pub useful: Vec<String>,
    pub traps: Vec<String>,
    pub search_top: Vec<String>,
    pub search_total: usize,
    pub startup_top: Vec<String>,
    pub startup_total: usize,
}

/// How many surfaced ids per seam a [`CaseOutcome`] carries.
pub const CASE_DUMP_TOP_K: usize = 10;

/// [`run_quality_report`] against an arbitrary corpus root, also returning the
/// per-case outcomes (one ranking pass feeds both).
pub async fn run_quality_report_with_cases_for_root(
    root: &Path,
) -> Result<(QualityReport, Vec<CaseOutcome>), QualityError> {
    let corpus = GoldenCorpus::load_from_root(root).await?;
    let cases = GoldenCorpus::load_queries_from_root(root)?;

    let mut search_acc = SeamAccumulator::default();
    let mut startup_acc = SeamAccumulator::default();
    let mut abstentions = Vec::new();
    let mut abstention_count = 0usize;
    let mut outcomes = Vec::with_capacity(cases.len());

    for case in &cases {
        let search_ranked = corpus.rank_via_search(&case.query).await?;
        let startup_ranked = corpus.rank_via_startup(&case.namespace_scope).await?;

        outcomes.push(CaseOutcome {
            id: case.id.clone(),
            query: case.query.clone(),
            scope: case.namespace_scope.clone(),
            essential: case.graded.essential.clone(),
            useful: case.graded.useful.clone(),
            traps: case.graded.irrelevant_traps.clone(),
            search_top: search_ranked.iter().take(CASE_DUMP_TOP_K).cloned().collect(),
            search_total: search_ranked.len(),
            startup_top: startup_ranked.iter().take(CASE_DUMP_TOP_K).cloned().collect(),
            startup_total: startup_ranked.len(),
        });

        if case.is_abstention() {
            abstention_count += 1;
            abstentions.push(abstention_outcome(case, "search", &search_ranked));
            abstentions.push(abstention_outcome(case, "startup", &startup_ranked));
        } else {
            search_acc.add_case(case, &search_ranked);
            startup_acc.add_case(case, &startup_ranked);
        }
    }

    let mut seams = BTreeMap::new();
    seams.insert("search".to_string(), search_acc.finish());
    seams.insert("startup".to_string(), startup_acc.finish());

    let mut seam_notes = BTreeMap::new();
    seam_notes.insert(
        "search".to_string(),
        "memory_search seam: raw query -> Substrate::query_chunks (bm25 FTS), exactly as the \
         daemon's search_response handler. FTS5 sanitization ANDs each token as a phrase, so \
         long natural-language / session-context queries that no single chunk satisfies score \
         near zero — this is the honest behavior of keyword search, not a runner defect. \
         Exact-MemoryId queries (q01-q03) also score zero here: memory_search is chunk-body \
         search, not id lookup. The short keyword/entity cases (q51-q56) give this seam \
         positive dynamic range; the startup seam carries namespace-scoped structural ranking."
            .to_string(),
    );
    seam_notes.insert(
        "startup".to_string(),
        "startup-block assembly seam: namespace-scoped candidate selection \
         by calling collect_recall_candidates_from_index, plus the real structural points \
         ranking (select_ranked_candidates / recall/rank.rs), driven by the case's \
         namespace_scope. Dynamics strength is pinned off for this quality gate, so ambient \
         MEMORUM_DYNAMICS cannot change the baseline. No vector lane today (Task 3.x)."
            .to_string(),
    );

    let report = QualityReport {
        schema: 1,
        ranking_lane: "fts_bm25(search)+structural_points(startup); no vector lane (Task 3.x pending)".to_string(),
        total_cases: cases.len(),
        abstention_cases: abstention_count,
        seams,
        seam_notes,
        abstentions,
    };
    Ok((report, outcomes))
}

fn abstention_outcome(case: &QueryCase, seam: &str, ranked: &[String]) -> AbstentionOutcome {
    let top5 = &ranked[..ranked.len().min(TRAP_K)];
    AbstentionOutcome {
        case_id: case.id.clone(),
        seam: seam.to_string(),
        surfaced_count: ranked.len(),
        surfaced_top: ranked.iter().take(5).cloned().collect(),
        surfaced_trap_at_5: top5.iter().any(|id| case.is_trap(id)),
    }
}

/// Serialize the report to pretty JSON.
pub fn report_to_json(report: &QualityReport) -> String {
    serde_json::to_string_pretty(report).expect("QualityReport serializes infallibly")
}

// ---------------------------------------------------------------------------
// Baseline gate
// ---------------------------------------------------------------------------

/// Canonical path of the human-committed quality baseline.
///
/// IMPORTANT: this file is **human-committed only**, and
/// `scripts/check-baseline-discipline.sh` enforces the same guard as other
/// canonical bench JSON. The runner never writes it. The gate skips cleanly
/// when it does not yet exist — Trey commits the initial baseline after
/// reviewing an emitted JSON report.
pub fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/quality-baseline.json")
}

/// Outcome of comparing a fresh report against the committed baseline.
#[derive(Debug)]
pub enum GateOutcome {
    /// No baseline on disk yet — the gate skips (the first run produces the JSON
    /// a human reviews and commits).
    SkippedNoBaseline,
    /// Within tolerance on every tracked metric.
    Pass,
    /// One or more metrics regressed beyond tolerance.
    Regressed(Vec<String>),
}

/// Compare a fresh report against the committed baseline (if present) within
/// the given absolute tolerance.
///
/// Tracked regressions:
///   - nDCG@5 dropped by more than `tolerance` (headline ranking regression),
///   - recall@5 dropped by more than `tolerance` (lost a relevant hit),
///   - trap-rate@5 rose by more than `tolerance` (surfaced a new trap),
///   - a seam present in either side is missing from the other side,
///   - an abstention case newly surfaces a labeled trap in its top-5.
pub fn compare_to_baseline(report: &QualityReport, tolerance: f64) -> Result<GateOutcome, QualityError> {
    let path = baseline_path();
    if !path.exists() {
        return Ok(GateOutcome::SkippedNoBaseline);
    }
    let text = fs::read_to_string(&path)?;
    let baseline: QualityReport =
        serde_json::from_str(&text).map_err(|e| QualityError::Queries(format!("baseline parse: {e}")))?;

    Ok(compare_reports(report, &baseline, tolerance))
}

fn compare_reports(report: &QualityReport, baseline: &QualityReport, tolerance: f64) -> GateOutcome {
    let mut regressions = Vec::new();
    for (seam, current) in &report.seams {
        let Some(base) = baseline.seams.get(seam) else {
            regressions.push(format!("baseline missing seam `{seam}`"));
            continue;
        };
        let mut check = GateCheck { seam, tolerance, out: &mut regressions };
        check.drop_at_5("ndcg@5", &base.ndcg_at_k, &current.ndcg_at_k);
        check.drop_at_5("recall@5", &base.recall_at_k, &current.recall_at_k);
        check.rise("trap_rate@5", base.trap_rate_at_5, current.trap_rate_at_5);
    }
    for seam in baseline.seams.keys() {
        if !report.seams.contains_key(seam) {
            regressions.push(format!("report missing seam `{seam}`"));
        }
    }
    check_abstention_traps(report, baseline, &mut regressions);

    if regressions.is_empty() {
        GateOutcome::Pass
    } else {
        GateOutcome::Regressed(regressions)
    }
}

fn check_abstention_traps(report: &QualityReport, baseline: &QualityReport, regressions: &mut Vec<String>) {
    let baseline_traps: BTreeMap<(&str, &str), bool> = baseline
        .abstentions
        .iter()
        .map(|outcome| ((outcome.case_id.as_str(), outcome.seam.as_str()), outcome.surfaced_trap_at_5))
        .collect();

    for current in &report.abstentions {
        let key = (current.case_id.as_str(), current.seam.as_str());
        if baseline_traps.get(&key) == Some(&false) && current.surfaced_trap_at_5 {
            regressions.push(format!(
                "abstention {}/{}: surfaced_trap_at_5 changed false -> true",
                current.case_id, current.seam
            ));
        }
    }
}

/// One seam's tolerance-banded baseline comparison, accumulating violations.
struct GateCheck<'a> {
    seam: &'a str,
    tolerance: f64,
    out: &'a mut Vec<String>,
}

impl GateCheck<'_> {
    /// Flag `label` when the current `@5` metric drops more than `tolerance` below baseline.
    /// The gate only inspects rank-5 cuts; trap-rate is likewise @5-only.
    fn drop_at_5(&mut self, label: &str, base: &BTreeMap<String, f64>, current: &BTreeMap<String, f64>) {
        let (Some(&b), Some(&c)) = (base.get("5"), current.get("5")) else { return };
        if b - c > self.tolerance {
            let Self { seam, tolerance, .. } = self;
            self.out.push(format!("{seam} {label}: {c:.4} regressed below baseline {b:.4} (tolerance {tolerance})"));
        }
    }

    /// Flag `label` when the current value rises more than `tolerance` above baseline (lower-is-better metrics).
    fn rise(&mut self, label: &str, base: f64, current: f64) {
        if current - base > self.tolerance {
            let Self { seam, tolerance, .. } = self;
            self.out
                .push(format!("{seam} {label}: {current:.4} rose above baseline {base:.4} (tolerance {tolerance})"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(essential: &[&str], useful: &[&str], traps: &[&str]) -> QueryCase {
        QueryCase {
            id: "qtest".to_string(),
            query: "q".to_string(),
            namespace_scope: vec!["me".to_string()],
            graded: Graded {
                essential: essential.iter().map(|s| s.to_string()).collect(),
                useful: useful.iter().map(|s| s.to_string()).collect(),
                irrelevant_traps: traps.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn metric_map(value_at_5: f64) -> BTreeMap<String, f64> {
        BTreeMap::from([("3".to_string(), 0.0), ("5".to_string(), value_at_5), ("10".to_string(), 0.0)])
    }

    fn metrics(ndcg5: f64, recall5: f64, trap_rate_at_5: f64) -> SeamMetrics {
        SeamMetrics {
            scored_cases: 1,
            precision_at_k: metric_map(0.0),
            recall_at_k: metric_map(recall5),
            mrr: 0.0,
            ndcg_at_k: metric_map(ndcg5),
            trap_rate_at_5,
        }
    }

    fn quality_report(seams: &[(&str, SeamMetrics)], abstentions: Vec<AbstentionOutcome>) -> QualityReport {
        QualityReport {
            schema: 1,
            ranking_lane: "test".to_string(),
            total_cases: 1,
            abstention_cases: abstentions.len(),
            seams: seams.iter().map(|(name, metrics)| ((*name).to_string(), metrics.clone())).collect(),
            seam_notes: BTreeMap::new(),
            abstentions,
        }
    }

    #[test]
    fn perfect_ranking_scores_one() {
        let c = case(&["a"], &["b"], &["trap"]);
        let ranked = ids(&["a", "b", "c"]);
        let mut acc = SeamAccumulator::default();
        acc.add_case(&c, &ranked);
        let m = acc.finish();
        assert!((m.ndcg_at_k["5"] - 1.0).abs() < 1e-9, "ideal ranking → nDCG@5 = 1, got {}", m.ndcg_at_k["5"]);
        assert!((m.recall_at_k["5"] - 1.0).abs() < 1e-9);
        assert!((m.mrr - 1.0).abs() < 1e-9);
        assert_eq!(m.trap_rate_at_5, 0.0);
    }

    #[test]
    fn trap_in_top5_trips_trap_rate() {
        let c = case(&["a"], &[], &["trap"]);
        let ranked = ids(&["trap", "a"]);
        let mut acc = SeamAccumulator::default();
        acc.add_case(&c, &ranked);
        let m = acc.finish();
        assert_eq!(m.trap_rate_at_5, 1.0, "trap surfaced in top-5");
        // First relevant hit is at rank 2 → MRR = 0.5.
        assert!((m.mrr - 0.5).abs() < 1e-9, "mrr {}", m.mrr);
    }

    #[test]
    fn missing_relevant_zeroes_recall_and_mrr() {
        let c = case(&["a"], &[], &[]);
        let ranked = ids(&["x", "y", "z"]);
        let mut acc = SeamAccumulator::default();
        acc.add_case(&c, &ranked);
        let m = acc.finish();
        assert_eq!(m.recall_at_k["5"], 0.0);
        assert_eq!(m.mrr, 0.0);
        assert_eq!(m.ndcg_at_k["5"], 0.0);
    }

    #[test]
    fn graded_ndcg_rewards_essential_over_useful_order() {
        // useful-before-essential ranks worse than essential-before-useful.
        let c = case(&["ess"], &["use"], &[]);
        let good = ids(&["ess", "use"]);
        let bad = ids(&["use", "ess"]);
        assert!(ndcg_at_k(&c, &good, 5) > ndcg_at_k(&c, &bad, 5));
        assert!((ndcg_at_k(&c, &good, 5) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn abstention_outcome_flags_trap() {
        let c = case(&[], &[], &["oldlaptop"]);
        assert!(c.is_abstention());
        let out = abstention_outcome(&c, "search", &ids(&["oldlaptop", "other"]));
        assert!(out.surfaced_trap_at_5);
        assert_eq!(out.surfaced_count, 2);
    }

    #[test]
    fn single_project_canonical_only_when_exactly_one() {
        assert_eq!(single_project_canonical(&ids(&["project:p1"])), Some("p1".to_string()));
        assert_eq!(single_project_canonical(&ids(&["project:p1", "project:p2"])), None);
        assert_eq!(single_project_canonical(&ids(&["me"])), None);
    }

    #[test]
    fn precision_denominator_is_k() {
        // One relevant hit, only one item surfaced: precision@3 = 1/3, not 1/1.
        let c = case(&["a"], &[], &[]);
        let mut acc = SeamAccumulator::default();
        acc.add_case(&c, &ids(&["a"]));
        let m = acc.finish();
        assert!((m.precision_at_k["3"] - 1.0 / 3.0).abs() < 1e-9, "precision@3 {}", m.precision_at_k["3"]);
    }

    #[test]
    fn compare_reports_flags_missing_seams_on_either_side() {
        let search = metrics(1.0, 1.0, 0.0);
        let startup = metrics(1.0, 1.0, 0.0);

        let baseline_missing = quality_report(&[("search", search.clone())], Vec::new());
        let current_with_extra =
            quality_report(&[("search", search.clone()), ("startup", startup.clone())], Vec::new());
        match compare_reports(&current_with_extra, &baseline_missing, DEFAULT_TOLERANCE) {
            GateOutcome::Regressed(regressions) => {
                assert!(regressions.iter().any(|r| r == "baseline missing seam `startup`"), "{regressions:?}");
            }
            other => panic!("expected regression, got {other:?}"),
        }

        let baseline_with_extra = quality_report(&[("search", search.clone()), ("startup", startup)], Vec::new());
        let current_missing = quality_report(&[("search", search)], Vec::new());
        match compare_reports(&current_missing, &baseline_with_extra, DEFAULT_TOLERANCE) {
            GateOutcome::Regressed(regressions) => {
                assert!(regressions.iter().any(|r| r == "report missing seam `startup`"), "{regressions:?}");
            }
            other => panic!("expected regression, got {other:?}"),
        }
    }

    #[test]
    fn compare_reports_flags_new_abstention_trap() {
        let clean = AbstentionOutcome {
            case_id: "q-abstain".to_string(),
            seam: "search".to_string(),
            surfaced_count: 1,
            surfaced_top: ids(&["safe"]),
            surfaced_trap_at_5: false,
        };
        let mut trapped = clean.clone();
        trapped.surfaced_top = ids(&["trap"]);
        trapped.surfaced_trap_at_5 = true;

        let baseline = quality_report(&[("search", metrics(1.0, 1.0, 0.0))], vec![clean]);
        let current = quality_report(&[("search", metrics(1.0, 1.0, 0.0))], vec![trapped]);

        match compare_reports(&current, &baseline, DEFAULT_TOLERANCE) {
            GateOutcome::Regressed(regressions) => {
                assert!(
                    regressions
                        .iter()
                        .any(|r| r == "abstention q-abstain/search: surfaced_trap_at_5 changed false -> true"),
                    "{regressions:?}"
                );
            }
            other => panic!("expected regression, got {other:?}"),
        }
    }
}
