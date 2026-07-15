//! LoCoMo and LongMemEval baseline runner over the real daemon write and recall
//! protocol surfaces.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::de::{DeserializeSeed, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::daemon_scaffold::DaemonScaffold;
use crate::enrichment::{
    enrichment_key, load_sidecar, load_v2_sidecar, v2_prompt_sha256, Enrichment, EnrichmentProvenance,
    EnrichmentSidecar, Generation, V2_WINDOW_POLICY,
};
use crate::judge::{BenchmarkJudge, BenchmarkJudgeInput, BenchmarkJudgeVerdict, JudgeError};

const TOP_K: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Dev,
    Holdout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkEmbeddingLane {
    FtsOnly,
    DaemonConfigured,
    GeminiApi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkFusion {
    Legacy,
    FourLane,
}

#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub dataset_dir: PathBuf,
    pub generation: Generation,
    pub splits: Vec<Split>,
    pub locomo_conversation_limit: Option<usize>,
    pub locomo_qa_per_conversation: Option<usize>,
    pub longmemeval_per_split: usize,
    pub longmemeval_cleaned: bool,
    pub embedding_lane: BenchmarkEmbeddingLane,
    pub fusion: BenchmarkFusion,
    /// Dev-split weight-sweep override for the four RRF lanes; `None` keeps the
    /// daemon defaults. Recorded in the report for artifact provenance.
    pub fusion_weights: Option<FusionWeights>,
    pub expected_sensitivity: String,
    pub judge_timeout: u64,
    /// Write bodies excluded from ingestion in EVERY arm, identified by their
    /// canonical v2 context-item key (Trey ruling 2026-07-15: a paired
    /// comparison stays valid only when an exclusion hits both sides).
    /// Recorded in the report for artifact provenance.
    pub excluded_keys: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FusionWeights {
    pub chunk: f64,
    pub bm25: f64,
    pub abstraction: f64,
    pub cue: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SplitConfig {
    pub generation: Generation,
    pub enrichment_prompt_sha256: Option<String>,
    pub enrichment_window_policy: Option<&'static str>,
    pub splits: Vec<Split>,
    pub locomo_conversation_limit: Option<usize>,
    pub locomo_qa_per_conversation: Option<usize>,
    pub longmemeval_per_split: usize,
    pub longmemeval_cleaned: bool,
    pub embedding_lane: &'static str,
    pub fusion: &'static str,
    pub fusion_weights: Option<FusionWeights>,
    pub excluded_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BaselineReport {
    pub schema_version: &'static str,
    pub report_name: &'static str,
    pub ranking_lanes: Vec<&'static str>,
    pub vector_lane: &'static str,
    pub dataset_sha256s: BTreeMap<String, String>,
    pub enrichment_provenance: BTreeMap<String, EnrichmentProvenance>,
    pub split_config: SplitConfig,
    pub sampling: SamplingReport,
    pub dispositions: DispositionCounts,
    pub governance_drag: GovernanceDrag,
    pub metrics: MetricReport,
    pub ingestion: Vec<IngestionRecord>,
    pub enrichment: EnrichmentIngestionCounts,
    pub items: Vec<ItemOutcome>,
    pub judge_inputs: Vec<BenchmarkJudgeInput>,
    pub judge_identity: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct SamplingReport {
    pub rule: &'static str,
    pub reason: &'static str,
    pub selected: Vec<SampledItem>,
}

#[derive(Debug, Serialize)]
pub struct SampledItem {
    pub dataset: &'static str,
    pub split: Split,
    pub id: String,
    pub category: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct DispositionCounts {
    pub promoted: usize,
    pub candidate: usize,
    pub quarantined: usize,
    pub refused: usize,
    pub approved: usize,
    pub approve_failed: usize,
}

#[derive(Debug, Serialize)]
pub struct IngestionRecord {
    pub id: Option<String>,
    pub source_kind: String,
    pub expected_sensitivity: String,
    pub actual_classification: String,
    pub initial_status: String,
    pub promoted_after_review: bool,
    pub enriched: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct EnrichmentIngestionCounts {
    pub with_enrichment: usize,
    pub without_enrichment: usize,
    pub promoted: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct GovernanceDrag {
    pub by_source_kind: BTreeMap<String, SourceDrag>,
    pub expected_actual_mismatches: usize,
    pub unobservable_classifications: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct SourceDrag {
    pub attempted: usize,
    pub promoted_after_review: usize,
    pub refused_or_unpromoted: usize,
    pub encrypted_not_retrievable: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MetricReport {
    pub scored_items: usize,
    pub excluded_items: usize,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub hit_at_10: f64,
    pub startup_coverage: f64,
    pub context_exact_match: f64,
    pub context_contains: f64,
    pub judge_mean: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ItemOutcome {
    pub dataset: &'static str,
    pub split: Split,
    pub id: String,
    pub category: String,
    pub dispositions: DispositionCounts,
    pub relevant_promoted_ids: Vec<String>,
    pub retrieved_ids: Vec<String>,
    pub startup_context_bytes: usize,
    pub startup_context_memory_ids: Vec<String>,
    pub startup_coverage: f64,
    pub search_hit_count: usize,
    pub search_empty: bool,
    pub hit_at_10: f64,
    pub reciprocal_rank: f64,
    pub recall_at_10: f64,
    pub ndcg_at_10: f64,
    pub context_exact_match: bool,
    pub context_contains: bool,
    pub unmatched_evidence: Vec<String>,
    pub item_error: Option<String>,
    pub judge: Option<BenchmarkJudgeVerdict>,
    pub judge_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LocomoConversation {
    #[serde(default)]
    sample_id: Value,
    conversation: BTreeMap<String, Value>,
    qa: Vec<LocomoQuestion>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocomoQuestion {
    question: String,
    #[serde(default)]
    answer: Value,
    #[serde(default)]
    evidence: Vec<String>,
    category: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct LocomoTurn {
    speaker: String,
    text: String,
    #[serde(default)]
    dia_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LongMemItem {
    question_id: String,
    question_type: String,
    question: String,
    answer: Value,
    #[serde(default)]
    haystack_dates: Vec<String>,
    #[serde(default)]
    haystack_session_ids: Vec<String>,
    #[serde(default)]
    haystack_sessions: Vec<Vec<LongMemTurn>>,
    #[serde(default)]
    answer_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LongMemTurn {
    role: String,
    content: String,
    #[serde(default)]
    dia_id: Option<String>,
    #[serde(default)]
    has_answer: bool,
}

#[derive(Debug)]
struct EvalItem {
    dataset: &'static str,
    split: Split,
    id: String,
    category: String,
    question: String,
    gold: String,
    evidence_turns: BTreeSet<String>,
    item_error: Option<String>,
}

#[derive(Debug)]
struct Session {
    id: String,
    date: Option<String>,
    turns: Vec<Turn>,
}

#[derive(Debug, Clone)]
pub(crate) struct CorpusContext {
    pub dataset: PathBuf,
    pub corpus_instance_id: String,
    pub session_id: String,
    pub target_ordinal: usize,
    pub session_turns: std::sync::Arc<[String]>,
    pub body: String,
    pub date_metadata: bool,
}

struct IngestedSessions {
    turn_ids: BTreeMap<String, Vec<String>>,
    #[allow(dead_code)]
    session_ids: BTreeMap<String, Vec<String>>,
    dispositions: DispositionCounts,
}

#[derive(Debug)]
struct Turn {
    speaker: String,
    text: String,
    dia_id: Option<String>,
    has_answer: bool,
}

fn split_config_from(config: &BenchmarkConfig) -> SplitConfig {
    SplitConfig {
        generation: config.generation,
        enrichment_prompt_sha256: (config.generation == Generation::V2).then(v2_prompt_sha256),
        enrichment_window_policy: (config.generation == Generation::V2).then_some(V2_WINDOW_POLICY),
        splits: config.splits.clone(),
        locomo_conversation_limit: config.locomo_conversation_limit,
        locomo_qa_per_conversation: config.locomo_qa_per_conversation,
        longmemeval_per_split: config.longmemeval_per_split,
        longmemeval_cleaned: config.longmemeval_cleaned,
        embedding_lane: match config.embedding_lane {
            BenchmarkEmbeddingLane::FtsOnly => "fts_only",
            BenchmarkEmbeddingLane::DaemonConfigured => "daemon_configured",
            BenchmarkEmbeddingLane::GeminiApi => "gemini_api",
        },
        fusion: match config.fusion {
            BenchmarkFusion::Legacy => "legacy",
            BenchmarkFusion::FourLane => "four_lane",
        },
        fusion_weights: config.fusion_weights,
        excluded_keys: config.excluded_keys.iter().cloned().collect(),
    }
}

pub async fn run_baseline(
    config: &BenchmarkConfig,
    judge: Option<&dyn BenchmarkJudge>,
) -> Result<BaselineReport, String> {
    let mut report = BaselineReport {
        schema_version: "baseline_0.1",
        report_name: "baseline_0",
        ranking_lanes: match config.fusion {
            BenchmarkFusion::Legacy => vec!["daemon_search_hybrid_legacy", "startup_recall"],
            BenchmarkFusion::FourLane => vec!["daemon_search_four_lane", "startup_recall"],
        },
        vector_lane: match config.embedding_lane {
            BenchmarkEmbeddingLane::FtsOnly => "disabled explicitly; production daemon degraded to FTS-only",
            BenchmarkEmbeddingLane::DaemonConfigured => "daemon-configured vector lane live",
            BenchmarkEmbeddingLane::GeminiApi => "gemini-api/gemini-embedding-2/768",
        },
        dataset_sha256s: BTreeMap::new(),
        enrichment_provenance: BTreeMap::new(),
        split_config: split_config_from(config),
        sampling: SamplingReport {
            rule: "LoCoMo conversation parity; LongMemEval sha256(question_id) last-byte parity",
            reason: "deterministic bounded runtime; category/question-type balanced where limited",
            selected: Vec::new(),
        },
        dispositions: DispositionCounts::default(),
        governance_drag: GovernanceDrag::default(),
        metrics: MetricReport::default(),
        ingestion: Vec::new(),
        enrichment: EnrichmentIngestionCounts::default(),
        items: Vec::new(),
        judge_inputs: Vec::new(),
        judge_identity: None,
    };

    run_locomo(config, judge, &mut report).await?;
    run_longmemeval(config, judge, &mut report).await?;
    report.judge_identity = judge.map(|j| j.identity());
    finish_metrics(&mut report);
    Ok(report)
}

async fn run_locomo(
    config: &BenchmarkConfig,
    judge: Option<&dyn BenchmarkJudge>,
    report: &mut BaselineReport,
) -> Result<(), String> {
    let path = config.dataset_dir.join("locomo/locomo10.json");
    let (conversations, sha256) = sampled_locomo_conversations(config, &path)?;
    let (enrichment, provenance) = load_enrichment(&path, config.generation, &sha256)?;
    report.dataset_sha256s.insert("locomo/locomo10.json".to_string(), sha256);
    if let Some(provenance) = provenance {
        report.enrichment_provenance.insert("locomo/locomo10.json".to_owned(), provenance);
    }
    for (split, conversation) in conversations {
        let scaffold = benchmark_scaffold(config).await;
        let project = scaffold.tree_dir().join("benchmark-project");
        fs::create_dir_all(&project).map_err(|error| error.to_string())?;
        fs::write(
            project.join(".memory-project.yaml"),
            "canonical_id: proj_memorum_benchmark\nalias: memorum-benchmark\n",
        )
        .map_err(|error| error.to_string())?;
        let mut daemon = DaemonClient::new(scaffold.socket_path(), &project);

        let sessions = locomo_sessions(&conversation);
        let sample_id = conversation.sample_id.clone();
        let corpus_instance_id = scalar_text(&sample_id);
        let ingested = ingest_sessions(
            &mut daemon,
            &path,
            &corpus_instance_id,
            &sessions,
            config.generation,
            &config.expected_sensitivity,
            &enrichment,
            &config.excluded_keys,
            report,
        )?;
        let questions = balanced_locomo_questions(conversation.qa, config.locomo_qa_per_conversation);
        for question in questions {
            let id = format!("{}:{}", scalar_text(&sample_id), short_hash(&question.question));
            score_item(
                &mut daemon,
                EvalItem {
                    dataset: "locomo",
                    split,
                    id,
                    category: question.category.to_string(),
                    question: question.question,
                    gold: scalar_text(&question.answer),
                    evidence_turns: question.evidence.iter().cloned().collect(),
                    item_error: None,
                },
                &ingested,
                judge,
                report,
            )?;
        }
    }
    Ok(())
}

async fn run_longmemeval(
    config: &BenchmarkConfig,
    judge: Option<&dyn BenchmarkJudge>,
    report: &mut BaselineReport,
) -> Result<(), String> {
    let name = if config.longmemeval_cleaned { "longmemeval_s_cleaned.json" } else { "longmemeval_oracle.json" };
    let path = config.dataset_dir.join("longmemeval").join(name);

    // First pass: read only the lightweight headers so we can run the split /
    // round-robin selection without loading the full (potentially large)
    // haystacks into memory. Compute the file hash in the same pass.
    let (headers, sha256) = read_longmemeval_headers(&path)?;
    let (enrichment, provenance) = load_enrichment(&path, config.generation, &sha256)?;
    report.dataset_sha256s.insert(format!("longmemeval/{name}"), sha256);
    if let Some(provenance) = provenance {
        report.enrichment_provenance.insert(format!("longmemeval/{name}"), provenance);
    }

    let selected_ids = selected_longmem_ids(config, &headers);

    // Second pass: stream the full file and keep only the selected items.
    // The custom visitor drops non-selected elements as they are parsed.
    let (mut selected_items, _parsed) = read_longmemeval_selected(&path, &selected_ids)?;

    for split in &config.splits {
        let mut split_items: Vec<&mut LongMemItem> =
            selected_items.iter_mut().filter(|item| longmem_split(&item.question_id) == *split).collect();
        split_items.sort_by(|a, b| {
            let a_key = (a.question_type.clone(), hash_bytes(&a.question_id));
            let b_key = (b.question_type.clone(), hash_bytes(&b.question_id));
            a_key.cmp(&b_key)
        });
        for item in split_items {
            let scaffold = benchmark_scaffold(config).await;
            let project = scaffold.tree_dir().join("benchmark-project");
            fs::create_dir_all(&project).map_err(|error| error.to_string())?;
            fs::write(
                project.join(".memory-project.yaml"),
                "canonical_id: proj_memorum_benchmark\nalias: memorum-benchmark\n",
            )
            .map_err(|error| error.to_string())?;
            let mut daemon = DaemonClient::new(scaffold.socket_path(), &project);

            let sessions = longmem_sessions(item);
            let session_ids: BTreeSet<String> = sessions.iter().map(|session| session.id.clone()).collect();
            let missing: Vec<String> = item
                .answer_session_ids
                .iter()
                .filter(|session_id| !session_ids.contains(*session_id))
                .cloned()
                .collect();
            let item_error = if missing.is_empty() {
                None
            } else {
                Some(format!("dangling answer_session_ids: {}", missing.join(", ")))
            };
            let evidence_turns = longmem_gold_turns(&sessions, &item.answer_session_ids);
            let ingested = ingest_sessions(
                &mut daemon,
                &path,
                &item.question_id,
                &sessions,
                config.generation,
                &config.expected_sensitivity,
                &enrichment,
                &config.excluded_keys,
                report,
            )?;
            score_item(
                &mut daemon,
                EvalItem {
                    dataset: "longmemeval",
                    split: *split,
                    id: item.question_id.clone(),
                    category: item.question_type.clone(),
                    question: item.question.clone(),
                    gold: scalar_text(&item.answer),
                    evidence_turns,
                    item_error,
                },
                &ingested,
                judge,
                report,
            )?;
            // Drop the current question's haystack as soon as its scaffold is
            // torn down; haystacks are not needed for aggregate metrics.
            item.haystack_sessions.clear();
            item.haystack_session_ids.clear();
            item.haystack_dates.clear();
        }
    }
    Ok(())
}

fn load_enrichment(
    dataset: &Path,
    generation: Generation,
    dataset_sha256: &str,
) -> Result<(EnrichmentSidecar, Option<EnrichmentProvenance>), String> {
    match generation {
        Generation::V1 => load_sidecar(dataset).map(|entries| (entries, None)),
        Generation::V2 => {
            let sidecar = load_v2_sidecar(dataset, dataset_sha256)?;
            let provenance = sidecar.provenance();
            Ok((sidecar.entries, Some(provenance)))
        }
    }
}

async fn benchmark_scaffold(config: &BenchmarkConfig) -> DaemonScaffold {
    let scaffold = match config.embedding_lane {
        BenchmarkEmbeddingLane::FtsOnly => DaemonScaffold::fresh_fts_only().await,
        BenchmarkEmbeddingLane::DaemonConfigured => DaemonScaffold::fresh().await,
        BenchmarkEmbeddingLane::GeminiApi => DaemonScaffold::fresh_gemini_api().await,
    };
    scaffold.set_four_lane_fusion(config.fusion == BenchmarkFusion::FourLane);
    if let Some(weights) = config.fusion_weights {
        scaffold.set_fusion_weights([weights.chunk, weights.bm25, weights.abstraction, weights.cue]);
    }
    scaffold
}

#[allow(clippy::too_many_arguments)]
fn ingest_sessions(
    daemon: &mut DaemonClient<'_>,
    dataset_path: &Path,
    corpus_instance_id: &str,
    sessions: &[Session],
    generation: Generation,
    expected_sensitivity: &str,
    enrichment: &EnrichmentSidecar,
    excluded_keys: &std::collections::BTreeSet<String>,
    report: &mut BaselineReport,
) -> Result<IngestedSessions, String> {
    let mut turn_ids = BTreeMap::<String, Vec<String>>::new();
    let mut session_ids = BTreeMap::<String, Vec<String>>::new();
    let mut dispositions = DispositionCounts::default();
    // Exclusions match on the canonical v2 context-item key in every arm and
    // generation, so one identifier removes the same body from both sides of a
    // paired comparison.
    let is_excluded = |session_id: &str, ordinal: usize, body: &str| {
        !excluded_keys.is_empty()
            && excluded_keys.contains(&crate::enrichment::context_item_key(
                corpus_instance_id,
                session_id,
                ordinal,
                body,
            ))
    };
    for session in sessions {
        let date_body = session.date.as_ref().map(|date| session_date_body(&session.id, date));
        if let Some(body) = date_body.filter(|body| !is_excluded(&session.id, session.turns.len(), body)) {
            let key = enrichment_key(generation, (corpus_instance_id, &session.id, session.turns.len(), &body));
            if let Some(_id) = ingest_one(
                daemon,
                &body,
                &key,
                generation,
                "agent_primary",
                false,
                dataset_path,
                expected_sensitivity,
                enrichment,
                report,
                &mut dispositions,
            )? {}
        }
        for (turn_index, turn) in session.turns.iter().enumerate() {
            let key =
                turn.dia_id.clone().unwrap_or_else(|| format!("D{}:{}", session_label(&session.id), turn_index + 1));
            let body = turn_body(turn);
            if is_excluded(&session.id, turn_index, &body) {
                continue;
            }
            let enrichment_key = enrichment_key(generation, (corpus_instance_id, &session.id, turn_index, &body));
            if let Some(id) = ingest_one(
                daemon,
                &body,
                &enrichment_key,
                generation,
                "user",
                true,
                dataset_path,
                expected_sensitivity,
                enrichment,
                report,
                &mut dispositions,
            )? {
                turn_ids.entry(key).or_default().push(id.clone());
                session_ids.entry(session.id.clone()).or_default().push(id);
            }
        }
    }
    Ok(IngestedSessions { turn_ids, session_ids, dispositions })
}

#[allow(clippy::too_many_arguments)]
fn ingest_one(
    daemon: &mut DaemonClient<'_>,
    body: &str,
    enrichment_key: &str,
    generation: Generation,
    source_kind: &str,
    explicit_user_context: bool,
    dataset_path: &Path,
    expected_sensitivity: &str,
    enrichment: &EnrichmentSidecar,
    report: &mut BaselineReport,
    item_dispositions: &mut DispositionCounts,
) -> Result<Option<String>, String> {
    let source_ref =
        if source_kind == "agent_primary" { Some(format!("file:{}", dataset_path.display())) } else { None };
    let entry = enrichment.get(enrichment_key);
    if generation == Generation::V2 && entry.is_none() {
        return Err(format!("v2 enrichment incomplete: missing key {enrichment_key} for {}", dataset_path.display()));
    }
    if entry.is_some() {
        report.enrichment.with_enrichment += 1;
    } else {
        report.enrichment.without_enrichment += 1;
    }
    let mut meta = json!({
        "namespace": "project", "type": "project", "confidence": 0.9, "source_kind": source_kind,
        "source_ref": source_ref, "explicit_user_context": explicit_user_context, "cwd": daemon.cwd,
        "session_id": "memorum-eval-benchmark", "harness": "memorum-eval"
    });
    apply_enrichment_meta(&mut meta, entry, generation)?;
    let response = daemon.request(json!({"write_memory": {
        "body": body,
        "title": null,
        "tags": ["benchmark", "baseline_0"],
        "meta": meta
    }}))?;
    // A typed daemon error (e.g. a secret refusal) is a governance outcome,
    // not a harness failure: record it as drag and move on. Per the ingestion
    // contract, refusals are data.
    if let Some(code) = write_error_code(&response) {
        let drag = report.governance_drag.by_source_kind.entry(source_kind.to_owned()).or_default();
        drag.attempted += 1;
        drag.refused_or_unpromoted += 1;
        report.dispositions.refused += 1;
        item_dispositions.refused += 1;
        report.ingestion.push(IngestionRecord {
            id: None,
            source_kind: source_kind.to_owned(),
            expected_sensitivity: expected_sensitivity.to_owned(),
            actual_classification: format!("write_error:{code}"),
            initial_status: "refused".to_owned(),
            promoted_after_review: false,
            enriched: entry.is_some(),
        });
        return Ok(None);
    }
    let payload = success_payload(&response, "governance_write")?;
    let status = payload.get("status").and_then(Value::as_str).unwrap_or("refused");
    let drag = report.governance_drag.by_source_kind.entry(source_kind.to_owned()).or_default();
    drag.attempted += 1;
    match status {
        "promoted" => {
            report.dispositions.promoted += 1;
            item_dispositions.promoted += 1;
        }
        "candidate" => {
            report.dispositions.candidate += 1;
            item_dispositions.candidate += 1;
        }
        "quarantined" => {
            report.dispositions.quarantined += 1;
            item_dispositions.quarantined += 1;
        }
        _ => {
            report.dispositions.refused += 1;
            item_dispositions.refused += 1;
        }
    }
    let Some(id) = payload.get("id").and_then(Value::as_str).map(str::to_owned) else {
        drag.refused_or_unpromoted += 1;
        report.ingestion.push(IngestionRecord {
            id: None,
            source_kind: source_kind.to_owned(),
            expected_sensitivity: expected_sensitivity.to_owned(),
            actual_classification: "unobservable_refusal".to_owned(),
            initial_status: status.to_owned(),
            promoted_after_review: false,
            enriched: entry.is_some(),
        });
        return Ok(None);
    };

    let promoted = if status == "promoted" {
        true
    } else if matches!(status, "candidate" | "quarantined") {
        let approved = approve_until(daemon, &id, 3)?;
        if approved {
            report.dispositions.approved += 1;
            item_dispositions.approved += 1;
        } else {
            report.dispositions.approve_failed += 1;
            item_dispositions.approve_failed += 1;
        }
        approved
    } else {
        false
    };

    if entry.is_some() && promoted {
        report.enrichment.promoted += 1;
    }

    if !promoted {
        drag.refused_or_unpromoted += 1;
        report.ingestion.push(IngestionRecord {
            id: Some(id),
            source_kind: source_kind.to_owned(),
            expected_sensitivity: expected_sensitivity.to_owned(),
            actual_classification: "unobservable_unpromoted".to_owned(),
            initial_status: status.to_owned(),
            promoted_after_review: false,
            enriched: entry.is_some(),
        });
        return Ok(None);
    }

    drag.promoted_after_review += 1;
    let get_response = daemon.request(json!({"get": {"id": &id, "include_provenance": false}}))?;
    let get_payload = success_payload(&get_response, "get")?;
    let actual_classification = get_payload
        .get("sensitivity")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "unobservable".to_owned());
    let get_body = get_payload.get("body").and_then(Value::as_str).unwrap_or("");
    let encrypted = get_body == "[encrypted content omitted]";

    report.ingestion.push(IngestionRecord {
        id: Some(id.clone()),
        source_kind: source_kind.to_owned(),
        expected_sensitivity: expected_sensitivity.to_owned(),
        actual_classification: actual_classification.clone(),
        initial_status: status.to_owned(),
        promoted_after_review: true,
        enriched: entry.is_some(),
    });

    if actual_classification != "unobservable" && actual_classification != expected_sensitivity {
        report.governance_drag.expected_actual_mismatches += 1;
    }
    if encrypted {
        drag.encrypted_not_retrievable += 1;
        return Ok(None);
    }
    if actual_classification == "unobservable" {
        report.governance_drag.unobservable_classifications += 1;
    }
    Ok(Some(id))
}

fn apply_enrichment_meta(meta: &mut Value, entry: Option<&Enrichment>, generation: Generation) -> Result<(), String> {
    if let Some(Enrichment { abstraction, cues, .. }) = entry {
        if generation == Generation::V2 && abstraction.is_none() && !cues.is_empty() {
            return Err("v2 enrichment invalid: null abstraction requires empty cues".to_owned());
        }
        meta["abstraction"] = json!(abstraction);
        meta["cues"] = json!(cues);
    }
    Ok(())
}

fn approve_until(daemon: &mut DaemonClient<'_>, id: &str, max_attempts: usize) -> Result<bool, String> {
    for _ in 0..max_attempts {
        let response = daemon.request(json!({"review_approve": {"id": id}}))?;
        let payload = success_payload(&response, "review_approve")?;
        let status = payload.get("status").and_then(Value::as_str).unwrap_or("rejected");
        if status == "approved" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn score_item(
    daemon: &mut DaemonClient<'_>,
    item: EvalItem,
    ingested: &IngestedSessions,
    judge: Option<&dyn BenchmarkJudge>,
    report: &mut BaselineReport,
) -> Result<(), String> {
    report.sampling.selected.push(SampledItem {
        dataset: item.dataset,
        split: item.split,
        id: item.id.clone(),
        category: item.category.clone(),
    });

    if let Some(item_error) = item.item_error {
        report.items.push(ItemOutcome {
            dataset: item.dataset,
            split: item.split,
            id: item.id,
            category: item.category,
            dispositions: ingested.dispositions.clone(),
            relevant_promoted_ids: Vec::new(),
            retrieved_ids: Vec::new(),
            startup_context_bytes: 0,
            startup_context_memory_ids: Vec::new(),
            startup_coverage: 0.0,
            search_hit_count: 0,
            search_empty: true,
            hit_at_10: 0.0,
            reciprocal_rank: 0.0,
            recall_at_10: 0.0,
            ndcg_at_10: 0.0,
            context_exact_match: false,
            context_contains: false,
            unmatched_evidence: Vec::new(),
            item_error: Some(item_error),
            judge: None,
            judge_error: None,
        });
        return Ok(());
    }

    let relevant: BTreeSet<String> = item
        .evidence_turns
        .iter()
        .flat_map(|turn_id| ingested.turn_ids.get(turn_id).into_iter().flatten().cloned())
        .collect();
    let unmatched_evidence: Vec<String> =
        item.evidence_turns.iter().filter(|turn_id| !ingested.turn_ids.contains_key(*turn_id)).cloned().collect();

    let search = daemon.request(json!({"search": {"query": item.question, "limit": TOP_K, "include_body": true}}))?;
    let hits = success_payload(&search, "search")?.get("hits").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut seen = BTreeSet::<String>::new();
    let mut retrieved_ids = Vec::new();
    let mut contexts = Vec::new();
    for hit in hits {
        let Some(id) = hit.get("id").and_then(Value::as_str) else { continue };
        if seen.insert(id.to_owned()) {
            retrieved_ids.push(id.to_owned());
            let context =
                hit.get("body").or_else(|| hit.get("snippet")).and_then(Value::as_str).unwrap_or("").to_owned();
            contexts.push(context);
        }
    }

    let answer_basis = contexts.join("\n");
    let input = BenchmarkJudgeInput {
        question: item.question.clone(),
        gold: item.gold.clone(),
        retrieved_context: contexts.clone(),
        answer_basis: answer_basis.clone(),
    };

    let (verdict, judge_error) = match judge.map(|judge| judge.judge(&input)) {
        Some(Ok(verdict)) => (Some(verdict), None),
        Some(Err(error)) => (None, Some(JudgeError::to_string(&error))),
        None => (None, None),
    };

    let startup = daemon.request(json!({"startup": {
        "cwd": daemon.cwd,
        "session_id": "memorum-eval-benchmark",
        "harness": "memorum-eval",
        "harness_version": null,
        "include_recent": true,
        "since_event_id": null,
        "budget_tokens": 1200
    }}))?;
    let recall_block = success_payload(&startup, "startup")?.get("recall_block").and_then(Value::as_str).unwrap_or("");
    let startup_context_bytes = recall_block.len();
    let startup_context_memory_ids = extract_memory_ids_from_recall_block(recall_block);
    let startup_relevant: BTreeSet<String> = startup_context_memory_ids.iter().cloned().collect();
    let startup_coverage = if relevant.is_empty() {
        0.0
    } else {
        relevant.intersection(&startup_relevant).count() as f64 / relevant.len() as f64
    };

    let search_hit_count = retrieved_ids.len();
    let search_empty = retrieved_ids.is_empty();
    let rank = rank_metrics(&retrieved_ids, &relevant);
    let normalized_gold = normalize(&item.gold);
    let exact = !normalized_gold.is_empty() && normalize(&answer_basis) == normalized_gold;
    let contains = !normalized_gold.is_empty() && normalize(&answer_basis).contains(&normalized_gold);

    report.judge_inputs.push(input);
    report.items.push(ItemOutcome {
        dataset: item.dataset,
        split: item.split,
        id: item.id,
        category: item.category,
        dispositions: ingested.dispositions.clone(),
        relevant_promoted_ids: relevant.into_iter().collect(),
        retrieved_ids,
        startup_context_bytes,
        startup_context_memory_ids,
        startup_coverage,
        search_hit_count,
        search_empty,
        hit_at_10: rank.hit_at_10,
        reciprocal_rank: rank.reciprocal_rank,
        recall_at_10: rank.recall_at_10,
        ndcg_at_10: rank.ndcg_at_10,
        context_exact_match: exact,
        context_contains: contains,
        unmatched_evidence,
        item_error: item.item_error,
        judge: verdict,
        judge_error,
    });
    Ok(())
}

fn finish_metrics(report: &mut BaselineReport) {
    let total = report.items.len();
    let excluded = report.items.iter().filter(|item| item.item_error.is_some()).count();
    let count = total - excluded;
    report.metrics.scored_items = count;
    report.metrics.excluded_items = excluded;
    if count == 0 {
        return;
    }
    let denominator = count as f64;
    let scored = report.items.iter().filter(|item| item.item_error.is_none());
    report.metrics.recall_at_10 = scored.clone().map(|item| item.recall_at_10).sum::<f64>() / denominator;
    report.metrics.mrr = scored.clone().map(|item| item.reciprocal_rank).sum::<f64>() / denominator;
    report.metrics.ndcg_at_10 = scored.clone().map(|item| item.ndcg_at_10).sum::<f64>() / denominator;
    report.metrics.hit_at_10 = scored.clone().map(|item| item.hit_at_10).sum::<f64>() / denominator;
    report.metrics.startup_coverage = scored.clone().map(|item| item.startup_coverage).sum::<f64>() / denominator;
    report.metrics.context_exact_match =
        scored.clone().filter(|item| item.context_exact_match).count() as f64 / denominator;
    report.metrics.context_contains = scored.clone().filter(|item| item.context_contains).count() as f64 / denominator;
    let judge_scores: Vec<f64> = scored.filter_map(|item| item.judge.as_ref().map(|v| v.score)).collect();
    report.metrics.judge_mean =
        (!judge_scores.is_empty()).then(|| judge_scores.iter().sum::<f64>() / judge_scores.len() as f64);
}

/// Enumerate the exact benchmark write bodies with the same selection and body
/// construction used by `run_baseline`.
pub(crate) fn sampled_corpus_bodies(config: &BenchmarkConfig) -> Result<Vec<(PathBuf, String)>, String> {
    Ok(sampled_corpus_contexts(config)?.into_iter().map(|context| (context.dataset, context.body)).collect())
}

/// Enumerate benchmark write bodies together with the conversational identity
/// needed by enrichment v2.
pub(crate) fn sampled_corpus_contexts(config: &BenchmarkConfig) -> Result<Vec<CorpusContext>, String> {
    let locomo_path = config.dataset_dir.join("locomo/locomo10.json");
    let longmem_name =
        if config.longmemeval_cleaned { "longmemeval_s_cleaned.json" } else { "longmemeval_oracle.json" };
    let longmem_path = config.dataset_dir.join("longmemeval").join(longmem_name);
    let mut bodies = Vec::new();
    if locomo_path.exists() {
        let (conversations, _) = sampled_locomo_conversations(config, &locomo_path)?;
        for (_, conversation) in conversations {
            let corpus_instance_id = scalar_text(&conversation.sample_id);
            bodies.extend(session_contexts(&locomo_path, &corpus_instance_id, &locomo_sessions(&conversation)));
        }
    }
    if longmem_path.exists() {
        let (headers, _) = read_longmemeval_headers(&longmem_path)?;
        let selected_ids = selected_longmem_ids(config, &headers);
        let (items, _) = read_longmemeval_selected(&longmem_path, &selected_ids)?;
        for item in items {
            bodies.extend(session_contexts(&longmem_path, &item.question_id, &longmem_sessions(&item)));
        }
    }
    Ok(bodies)
}

fn session_contexts(dataset: &Path, corpus_instance_id: &str, sessions: &[Session]) -> Vec<CorpusContext> {
    let mut contexts = Vec::new();
    for session in sessions {
        let session_turns: std::sync::Arc<[String]> = session.turns.iter().map(turn_body).collect::<Vec<_>>().into();
        if let Some(date) = &session.date {
            contexts.push(CorpusContext {
                dataset: dataset.to_path_buf(),
                corpus_instance_id: corpus_instance_id.to_owned(),
                session_id: session.id.clone(),
                target_ordinal: session_turns.len(),
                session_turns: std::sync::Arc::clone(&session_turns),
                body: session_date_body(&session.id, date),
                date_metadata: true,
            });
        }
        contexts.extend(session_turns.iter().enumerate().map(|(target_ordinal, body)| CorpusContext {
            dataset: dataset.to_path_buf(),
            corpus_instance_id: corpus_instance_id.to_owned(),
            session_id: session.id.clone(),
            target_ordinal,
            session_turns: std::sync::Arc::clone(&session_turns),
            body: body.clone(),
            date_metadata: false,
        }));
    }
    contexts
}

fn sampled_locomo_conversations(
    config: &BenchmarkConfig,
    path: &Path,
) -> Result<(Vec<(Split, LocomoConversation)>, String), String> {
    let (conversations, sha256): (Vec<LocomoConversation>, String) = read_json_with_sha256(path)?;
    let mut used = 0;
    let selected = conversations
        .into_iter()
        .enumerate()
        .filter_map(|(index, conversation)| {
            let split = if index % 2 == 0 { Split::Dev } else { Split::Holdout };
            if !config.splits.contains(&split) || config.locomo_conversation_limit.is_some_and(|limit| used >= limit) {
                return None;
            }
            used += 1;
            Some((split, conversation))
        })
        .collect();
    Ok((selected, sha256))
}

fn selected_longmem_ids(config: &BenchmarkConfig, headers: &[LongMemItemHeader]) -> BTreeSet<String> {
    let mut selected_ids = BTreeSet::new();
    for split in &config.splits {
        let split_refs: Vec<&LongMemItemHeader> =
            headers.iter().filter(|item| longmem_split(&item.question_id) == *split).collect();
        let selected = round_robin_by(
            split_refs,
            config.longmemeval_per_split,
            |item| item.question_type.clone(),
            |item| hash_bytes(&item.question_id),
        );
        selected_ids.extend(selected.into_iter().map(|item| item.question_id.clone()));
    }
    selected_ids
}

fn session_date_body(id: &str, date: &str) -> String {
    format!("Dataset session {id} occurred at {date}.")
}

fn turn_body(turn: &Turn) -> String {
    format!("{}: {}", turn.speaker, turn.text)
}

fn locomo_sessions(conversation: &LocomoConversation) -> Vec<Session> {
    let mut sessions = Vec::new();
    for (key, value) in &conversation.conversation {
        if !key.starts_with("session_") || key.ends_with("_date_time") {
            continue;
        }
        let turns: Vec<LocomoTurn> = match value.as_array() {
            Some(turns) => match serde_json::from_value(Value::Array(turns.to_vec())) {
                Ok(turns) => turns,
                Err(_) => continue,
            },
            None => continue,
        };
        let turns = turns
            .into_iter()
            .map(|turn| Turn { speaker: turn.speaker, text: turn.text, dia_id: turn.dia_id, has_answer: false })
            .collect();
        let date =
            conversation.conversation.get(&format!("{key}_date_time")).and_then(Value::as_str).map(str::to_owned);
        sessions.push(Session { id: key.clone(), date, turns });
    }
    sessions.sort_by_key(|session| session_number(&session.id));
    sessions
}

fn longmem_sessions(item: &LongMemItem) -> Vec<Session> {
    item.haystack_sessions
        .iter()
        .enumerate()
        .map(|(index, turns)| {
            let id = item.haystack_session_ids.get(index).cloned().unwrap_or_else(|| format!("session_{index}"));
            let label = session_label(&id);
            let date = item.haystack_dates.get(index).cloned();
            let turns = turns
                .iter()
                .enumerate()
                .map(|(turn_index, turn)| Turn {
                    speaker: turn.role.clone(),
                    text: turn.content.clone(),
                    dia_id: turn.dia_id.clone().or_else(|| Some(format!("D{label}:{}", turn_index + 1))),
                    has_answer: turn.has_answer,
                })
                .collect();
            Session { id, date, turns }
        })
        .collect()
}

fn longmem_gold_turns(sessions: &[Session], answer_session_ids: &[String]) -> BTreeSet<String> {
    let mut gold = BTreeSet::new();
    for session in sessions {
        if !answer_session_ids.contains(&session.id) {
            continue;
        }
        let answer_turns: Vec<&Turn> = session.turns.iter().filter(|turn| turn.has_answer).collect();
        let selected_turns: Vec<&Turn> =
            if answer_turns.is_empty() { session.turns.iter().collect() } else { answer_turns };
        for turn in selected_turns {
            if let Some(dia_id) = &turn.dia_id {
                gold.insert(dia_id.clone());
            }
        }
    }
    gold
}

fn balanced_locomo_questions(questions: Vec<LocomoQuestion>, limit: Option<usize>) -> Vec<LocomoQuestion> {
    let Some(limit) = limit else { return questions };
    round_robin_by(
        questions,
        limit,
        |question| question.category.to_string(),
        |question| hash_bytes(&question.question),
    )
}

#[cfg(test)]
fn balanced_longmem_items(items: &mut Vec<LongMemItem>, limit: usize) -> Vec<&LongMemItem> {
    let items_ref: &[LongMemItem] = &*items;
    let indices: Vec<usize> = (0..items_ref.len()).collect();
    let selected_indices = round_robin_by(
        indices,
        limit,
        |&i| items_ref[i].question_type.clone(),
        |&i| hash_bytes(&items_ref[i].question_id),
    );
    let selected_ids: BTreeSet<String> = selected_indices.iter().map(|&i| items_ref[i].question_id.clone()).collect();
    for item in items.iter_mut() {
        if !selected_ids.contains(&item.question_id) {
            item.haystack_sessions.clear();
            item.haystack_session_ids.clear();
            item.haystack_dates.clear();
        }
    }
    selected_indices.into_iter().map(|i| &items[i]).collect()
}

fn round_robin_by<T>(
    mut items: Vec<T>,
    limit: usize,
    key: impl Fn(&T) -> String,
    hash: impl Fn(&T) -> [u8; 32],
) -> Vec<T> {
    items.sort_by_key(|item| (key(item), hash(item)));
    let mut groups = BTreeMap::<String, Vec<T>>::new();
    for item in items {
        groups.entry(key(&item)).or_default().push(item);
    }
    let mut selected = Vec::new();
    while selected.len() < limit && groups.values().any(|group| !group.is_empty()) {
        for group in groups.values_mut() {
            if selected.len() == limit {
                break;
            }
            if !group.is_empty() {
                selected.push(group.remove(0));
            }
        }
    }
    selected
}

fn longmem_split(id: &str) -> Split {
    if hash_bytes(id)[31] % 2 == 0 {
        Split::Dev
    } else {
        Split::Holdout
    }
}

fn hash_bytes(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn short_hash(value: &str) -> String {
    hex::encode(&hash_bytes(value)[..6])
}

fn session_number(id: &str) -> usize {
    id.rsplit('_').next().and_then(|number| number.parse().ok()).unwrap_or(usize::MAX)
}

fn session_label(id: &str) -> String {
    if let Some(suffix) = id.strip_prefix("session_") {
        if let Ok(number) = suffix.parse::<usize>() {
            return number.to_string();
        }
    }
    id.to_owned()
}

fn normalize(value: &str) -> String {
    value.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Per-item rank-cutoff metrics, factored pure so tests can pin hand-computed
/// values (round-3 G6: the pipeline-level fixture had a single relevant item
/// at rank 1 and could not discriminate rank-cutoff, partial-recall, or
/// averaging bugs from a correct implementation).
struct RankMetrics {
    hit_at_10: f64,
    reciprocal_rank: f64,
    recall_at_10: f64,
    ndcg_at_10: f64,
}

fn rank_metrics(retrieved_ids: &[String], relevant: &BTreeSet<String>) -> RankMetrics {
    let hit_at_10 = retrieved_ids.iter().take(TOP_K).any(|id| relevant.contains(id)) as u8 as f64;
    // MRR deliberately scans the full ranked list, not just the top-K cut.
    let first_rank = retrieved_ids.iter().position(|id| relevant.contains(id)).map(|index| index + 1);
    let relevant_retrieved = retrieved_ids.iter().take(TOP_K).filter(|id| relevant.contains(*id)).count();
    let recall_at_10 = if relevant.is_empty() { 0.0 } else { relevant_retrieved as f64 / relevant.len() as f64 };
    RankMetrics {
        hit_at_10,
        reciprocal_rank: first_rank.map_or(0.0, |rank| 1.0 / rank as f64),
        recall_at_10,
        ndcg_at_10: binary_ndcg(retrieved_ids, relevant, TOP_K),
    }
}

fn binary_ndcg(ranked: &[String], relevant: &BTreeSet<String>, k: usize) -> f64 {
    let dcg = ranked
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| relevant.contains(*id))
        .map(|(index, _)| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();
    let ideal = (0..relevant.len().min(k)).map(|index| 1.0 / ((index + 2) as f64).log2()).sum::<f64>();
    if ideal == 0.0 {
        0.0
    } else {
        dcg / ideal
    }
}

#[cfg(test)]
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let file = fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    serde_json::from_reader(BufReader::new(file)).map_err(|error| format!("parse {}: {error}", path.display()))
}

struct Sha256Reader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R: Read> Read for Sha256Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.hasher.update(&buf[..n]);
        }
        Ok(n)
    }
}

fn read_json_with_sha256<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<(T, String), String> {
    let file = fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut reader = Sha256Reader { inner: BufReader::new(file), hasher: Sha256::new() };
    let value = serde_json::from_reader(&mut reader).map_err(|error| format!("parse {}: {error}", path.display()))?;
    let sha256 = hex::encode(reader.hasher.finalize());
    Ok((value, sha256))
}

#[derive(Debug, Clone, Deserialize)]
struct LongMemItemHeader {
    question_id: String,
    question_type: String,
}

fn read_longmemeval_headers(path: &Path) -> Result<(Vec<LongMemItemHeader>, String), String> {
    let file = fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut reader = Sha256Reader { inner: BufReader::new(file), hasher: Sha256::new() };
    let headers: Vec<LongMemItemHeader> =
        serde_json::from_reader(&mut reader).map_err(|error| format!("parse {}: {error}", path.display()))?;
    let sha256 = hex::encode(reader.hasher.finalize());
    Ok((headers, sha256))
}

struct StreamingLongMemItems<'a> {
    selected: &'a BTreeSet<String>,
    parsed: usize,
    items: Vec<LongMemItem>,
}

impl<'a> StreamingLongMemItems<'a> {
    fn new(selected: &'a BTreeSet<String>) -> Self {
        Self { selected, parsed: 0, items: Vec::new() }
    }
}

impl<'de, 'a> serde::de::DeserializeSeed<'de> for StreamingLongMemItems<'a> {
    type Value = (Vec<LongMemItem>, usize);

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'a> Visitor<'de> for StreamingLongMemItems<'a> {
    type Value = (Vec<LongMemItem>, usize);

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an array of LongMemEval items")
    }

    fn visit_seq<A>(mut self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(item) = seq.next_element::<LongMemItem>()? {
            self.parsed += 1;
            if self.selected.contains(&item.question_id) {
                self.items.push(item);
            }
        }
        Ok((self.items, self.parsed))
    }
}

fn read_longmemeval_selected(path: &Path, selected: &BTreeSet<String>) -> Result<(Vec<LongMemItem>, usize), String> {
    let file = fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let (items, parsed) = StreamingLongMemItems::new(selected)
        .deserialize(&mut deserializer)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;
    Ok((items, parsed))
}

fn extract_memory_ids_from_recall_block(block: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut start = 0;
    while let Some(offset) = block[start..].find("ref=\"") {
        start += offset + 5;
        if let Some(end) = block[start..].find('"') {
            let value = &block[start..start + end];
            if value.starts_with("mem_") {
                ids.push(value.to_owned());
            }
            start += end + 1;
        } else {
            break;
        }
    }
    ids
}

struct DaemonClient<'a> {
    socket: &'a Path,
    cwd: String,
    sequence: u64,
}

impl<'a> DaemonClient<'a> {
    fn new(socket: &'a Path, cwd: &'a Path) -> Self {
        Self { socket, cwd: cwd.display().to_string(), sequence: 0 }
    }

    fn request(&mut self, request: Value) -> Result<Value, String> {
        self.sequence += 1;
        let mut stream = UnixStream::connect(self.socket).map_err(|error| format!("connect daemon: {error}"))?;
        serde_json::to_writer(&mut stream, &json!({"id": format!("benchmark-{}", self.sequence), "request": request}))
            .map_err(|error| error.to_string())?;
        stream.write_all(b"\n").map_err(|error| error.to_string())?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).map_err(|error| error.to_string())?;
        serde_json::from_str(&line).map_err(|error| format!("parse daemon response: {error}: {line}"))
    }
}

fn success_payload<'a>(response: &'a Value, name: &str) -> Result<&'a Value, String> {
    response
        .pointer(&format!("/result/success/{name}"))
        .ok_or_else(|| format!("daemon response missing {name}: {response}"))
}

/// Typed daemon error code from a write response, if the daemon refused the
/// write outright (governance/privacy refusals arrive as `result.error`, not
/// as a `governance_write` success payload).
fn write_error_code(response: &Value) -> Option<String> {
    response.pointer("/result/error/code").and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::DeterministicMockJudge;

    #[cfg(test)]
    use std::os::unix::net::UnixListener;

    fn write_fixture<P: AsRef<Path>>(dir: P, sub_path: &str, content: &str) -> PathBuf {
        let path = dir.as_ref().join(sub_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    fn empty_longmemeval_fixture(dir: &Path) -> PathBuf {
        write_fixture(dir, "longmemeval/longmemeval_oracle.json", "[]")
    }

    fn empty_locomo_fixture(dir: &Path) -> PathBuf {
        write_fixture(dir, "locomo/locomo10.json", "[]")
    }

    fn fts_config(dir: &Path) -> BenchmarkConfig {
        BenchmarkConfig {
            dataset_dir: dir.to_path_buf(),
            generation: Generation::V1,
            splits: vec![Split::Dev],
            locomo_conversation_limit: Some(0),
            locomo_qa_per_conversation: Some(1),
            longmemeval_per_split: 1,
            longmemeval_cleaned: false,
            embedding_lane: BenchmarkEmbeddingLane::FtsOnly,
            fusion: BenchmarkFusion::Legacy,
            fusion_weights: None,
            expected_sensitivity: "internal".to_owned(),
            judge_timeout: 60,
            excluded_keys: Default::default(),
        }
    }

    #[test]
    fn preregistered_split_rules_are_stable() {
        assert_eq!(longmem_split("question-a"), longmem_split("question-a"));
        assert_ne!(short_hash("a"), short_hash("b"));
    }

    #[test]
    fn balancing_takes_from_each_category_before_seconds() {
        let selected = round_robin_by(
            vec![("a", 1), ("a", 2), ("b", 3)],
            2,
            |item| item.0.to_owned(),
            |item| hash_bytes(&item.1.to_string()),
        );
        assert_eq!(selected, vec![("a", 1), ("b", 3)]);
    }

    #[test]
    fn mock_judge_is_deterministic() {
        let input = BenchmarkJudgeInput {
            question: "q".to_owned(),
            gold: "GPS failure".to_owned(),
            retrieved_context: vec!["The GPS failed.".to_owned()],
            answer_basis: "The GPS failure was reported.".to_owned(),
        };
        assert_eq!(DeterministicMockJudge.judge(&input).expect("mock verdict").score, 1.0);
    }

    #[test]
    fn fixture_loaders_match_dataset_shapes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let locomo: Vec<LocomoConversation> = read_json(&root.join("locomo_sample.json")).expect("locomo fixture");
        let longmem: Vec<LongMemItem> = read_json(&root.join("longmemeval_sample.json")).expect("longmem fixture");
        assert_eq!(locomo.len(), 2);
        assert_eq!(longmem.len(), 2);
        assert_eq!(locomo_sessions(&locomo[0]).len(), 1);
        assert_eq!(longmem_sessions(&longmem[0]).len(), 1);
    }

    #[test]
    fn locomo_turn_level_gold() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[
  {
    "sample_id": "locomo-fixture-0",
    "conversation": {
      "session_1_date_time": "8 May 2023",
      "session_1": [
        {"speaker": "Caroline", "dia_id": "D1:1", "text": "I attended the support group yesterday."}
      ]
    },
    "qa": [
      {"question": "When was the group?", "answer": "7 May 2023", "evidence": ["D1:1"], "category": 2}
    ]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.locomo_conversation_limit = Some(1);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let item = report.items.iter().find(|item| item.dataset == "locomo").expect("locomo item");
        assert!(!item.relevant_promoted_ids.is_empty());
        assert!(item.relevant_promoted_ids.iter().all(|id| id.starts_with("mem_")));
    }

    #[test]
    fn write_error_code_extracts_typed_daemon_errors() {
        let error = serde_json::json!({"id":"x","result":{"error":{"code":"privacy_error","message":"m"}}});
        assert_eq!(write_error_code(&error).as_deref(), Some("privacy_error"));
        let success = serde_json::json!({"id":"x","result":{"success":{"governance_write":{"status":"promoted"}}}});
        assert_eq!(write_error_code(&success), None);
    }

    #[test]
    fn scaffold_provisions_privacy_key_before_daemon_start() {
        // A missing age key turns the first encrypted-tier classification into
        // a fatal privacy_error mid-run (found by the first full baseline run).
        let scaffold = crate::block_on(DaemonScaffold::fresh());
        assert!(
            scaffold.tree_dir().join(".memoryd/privacy/age-key.json").exists(),
            "scaffold must mint age key material like memoryd init does"
        );
    }

    #[test]
    fn gemini_scaffold_records_the_api_lane_triple_without_a_real_key() {
        let scaffold = crate::block_on(DaemonScaffold::fresh_gemini_api());
        let config = fs::read_to_string(scaffold.tree_dir().join("config.yaml")).expect("config");
        assert!(config.contains("api_embedding_consent: true"));
        assert!(config.contains("provider: gemini-api"));
        assert!(config.contains("model_ref: gemini-embedding-2"));
        assert!(config.contains("dimension: 768"));
    }

    #[test]
    fn sidecar_enrichment_is_forwarded_to_benchmark_writes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        let dataset = write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[{"sample_id":"x","conversation":{"session_1":[{"speaker":"A","dia_id":"D1:1","text":"hello"}]},"qa":[]}]"#,
        );
        let body = "A: hello";
        let sidecar = serde_json::json!({ crate::enrichment::item_key(body): {"abstraction":"A greeting", "cues":["A greeting"], "source":"structural"} });
        fs::write(crate::enrichment::sidecar_path(&dataset), serde_json::to_vec(&sidecar).expect("sidecar"))
            .expect("write sidecar");
        let mut config = fts_config(dataset_dir);
        config.locomo_conversation_limit = Some(1);
        let report = crate::block_on(run_baseline(&config, None)).expect("baseline");
        assert_eq!(report.enrichment.with_enrichment, 1);
        assert_eq!(report.enrichment.promoted, 1);
        assert!(report.ingestion.iter().any(|record| record.enriched));
    }

    #[test]
    fn v2_producer_consumer_keys_match_and_null_is_forwarded_as_no_abstraction() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dataset_dir = dir.path();
        let longmem = empty_longmemeval_fixture(dataset_dir);
        let dataset = write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[{"sample_id":"conversation-x","conversation":{"session_1":[{"speaker":"A","dia_id":"D1:1","text":"hello"}]},"qa":[]}]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.generation = Generation::V2;
        config.locomo_conversation_limit = Some(1);

        let context =
            sampled_corpus_contexts(&config).unwrap().into_iter().find(|context| context.dataset == dataset).unwrap();
        let key = enrichment_key(
            Generation::V2,
            (&context.corpus_instance_id, &context.session_id, context.target_ordinal, &context.body),
        );
        let enrichment = Enrichment { abstraction: None, cues: Vec::new(), source: "skipped_low_signal".to_owned() };
        let mut locomo_sidecar =
            crate::enrichment::EnrichmentSidecarV2::expected(&crate::enrichment::dataset_sha256(&dataset).unwrap());
        locomo_sidecar.entries.insert(key, enrichment.clone());
        crate::enrichment::save_v2_sidecar(&dataset, &locomo_sidecar).unwrap();
        let longmem_sidecar =
            crate::enrichment::EnrichmentSidecarV2::expected(&crate::enrichment::dataset_sha256(&longmem).unwrap());
        crate::enrichment::save_v2_sidecar(&longmem, &longmem_sidecar).unwrap();

        let mut meta = json!({});
        apply_enrichment_meta(&mut meta, Some(&enrichment), Generation::V2).unwrap();
        assert!(meta["abstraction"].is_null());
        assert_eq!(meta["cues"], json!([]));

        let report = crate::block_on(run_baseline(&config, None)).expect("v2 baseline");
        assert_eq!(report.split_config.generation, Generation::V2);
        assert_eq!(report.split_config.enrichment_prompt_sha256, Some(v2_prompt_sha256()));
        assert_eq!(report.split_config.enrichment_window_policy, Some(V2_WINDOW_POLICY));
        assert_eq!(report.enrichment.with_enrichment, 1);
        assert!(report.ingestion.iter().any(|record| record.enriched));
        assert_eq!(report.enrichment_provenance["locomo/locomo10.json"], locomo_sidecar.provenance());
    }

    #[test]
    fn v2_ingestion_refuses_null_abstraction_with_cues() {
        let mut meta = json!({});
        let corrupt = Enrichment {
            abstraction: None,
            cues: vec!["invalid cue".to_owned()],
            source: "skipped_low_signal".to_owned(),
        };
        let error = apply_enrichment_meta(&mut meta, Some(&corrupt), Generation::V2).unwrap_err();
        assert!(error.contains("null abstraction requires empty cues"), "{error}");
    }

    #[test]
    fn v2_ingests_date_metadata_at_one_past_last_ordinal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dataset_dir = dir.path();
        let longmem = empty_longmemeval_fixture(dataset_dir);
        let dataset = write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[{"sample_id":"conversation-x","conversation":{"session_1_date_time":"8 May 2023","session_1":[{"speaker":"A","dia_id":"D1:1","text":"hello"}]},"qa":[]}]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.generation = Generation::V2;
        config.locomo_conversation_limit = Some(1);
        let contexts = sampled_corpus_contexts(&config).unwrap();
        let date_context = contexts.iter().find(|context| context.date_metadata).expect("date context");
        assert_eq!(date_context.target_ordinal, date_context.session_turns.len());
        let date_key = enrichment_key(
            Generation::V2,
            (
                &date_context.corpus_instance_id,
                &date_context.session_id,
                date_context.target_ordinal,
                &date_context.body,
            ),
        );
        let mut locomo_sidecar =
            crate::enrichment::EnrichmentSidecarV2::expected(&crate::enrichment::dataset_sha256(&dataset).unwrap());
        locomo_sidecar.entries.insert(
            date_key.clone(),
            Enrichment { abstraction: None, cues: Vec::new(), source: "date_metadata".to_owned() },
        );
        let turn_context = contexts.iter().find(|context| !context.date_metadata).expect("turn context");
        let turn_key = enrichment_key(
            Generation::V2,
            (
                &turn_context.corpus_instance_id,
                &turn_context.session_id,
                turn_context.target_ordinal,
                &turn_context.body,
            ),
        );
        locomo_sidecar.entries.insert(
            turn_key,
            Enrichment { abstraction: None, cues: Vec::new(), source: "skipped_low_signal".to_owned() },
        );
        crate::enrichment::save_v2_sidecar(&dataset, &locomo_sidecar).unwrap();
        let longmem_sidecar =
            crate::enrichment::EnrichmentSidecarV2::expected(&crate::enrichment::dataset_sha256(&longmem).unwrap());
        crate::enrichment::save_v2_sidecar(&longmem, &longmem_sidecar).unwrap();

        let report = crate::block_on(run_baseline(&config, None)).expect("v2 baseline");
        assert_eq!(report.enrichment.with_enrichment, 2);
        assert_eq!(locomo_sidecar.entries[&date_key].source, "date_metadata");
    }

    #[test]
    fn v2_benchmark_refuses_incomplete_enrichment() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dataset_dir = dir.path();
        let longmem = empty_longmemeval_fixture(dataset_dir);
        let dataset = write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[{"sample_id":"conversation-x","conversation":{"session_1":[{"speaker":"A","dia_id":"D1:1","text":"hello"}]},"qa":[]}]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.generation = Generation::V2;
        config.locomo_conversation_limit = Some(1);
        let locomo_sidecar =
            crate::enrichment::EnrichmentSidecarV2::expected(&crate::enrichment::dataset_sha256(&dataset).unwrap());
        crate::enrichment::save_v2_sidecar(&dataset, &locomo_sidecar).unwrap();
        let longmem_sidecar =
            crate::enrichment::EnrichmentSidecarV2::expected(&crate::enrichment::dataset_sha256(&longmem).unwrap());
        crate::enrichment::save_v2_sidecar(&longmem, &longmem_sidecar).unwrap();

        let error = crate::block_on(run_baseline(&config, None)).unwrap_err();
        assert!(error.contains("v2 enrichment incomplete"), "{error}");
    }

    #[test]
    fn rank_metrics_hand_computed_rank_cutoff_and_partial_recall() {
        // 12 retrieved ids; relevant at ranks 2 and 11; gold size 3.
        let retrieved: Vec<String> = (1..=12).map(|i| format!("mem_{i:02}")).collect();
        let relevant: BTreeSet<String> = ["mem_02", "mem_11", "mem_99"].into_iter().map(str::to_owned).collect();
        let m = rank_metrics(&retrieved, &relevant);
        assert_eq!(m.hit_at_10, 1.0); // rank 2 is inside the top-10 cut
        assert_eq!(m.reciprocal_rank, 0.5); // first relevant at rank 2
                                            // rank-11 hit is outside the cut and mem_99 was never retrieved: 1 of 3.
        assert!((m.recall_at_10 - 1.0 / 3.0).abs() < 1e-12, "recall {}", m.recall_at_10);
        // Hand-computed binary nDCG@10: DCG = 1/log2(3) (single hit at rank 2);
        // IDCG = 1/log2(2) + 1/log2(3) + 1/log2(4) (3 gold ids, all within k).
        let dcg = 1.0 / 3.0f64.log2();
        let idcg = 1.0 + 1.0 / 3.0f64.log2() + 0.5;
        assert!((m.ndcg_at_10 - dcg / idcg).abs() < 1e-12, "ndcg {}", m.ndcg_at_10);

        // Relevant only past the cut: hit 0, but MRR scans the full list.
        let late: BTreeSet<String> = ["mem_11"].into_iter().map(str::to_owned).collect();
        let m = rank_metrics(&retrieved, &late);
        assert_eq!(m.hit_at_10, 0.0);
        assert!((m.reciprocal_rank - 1.0 / 11.0).abs() < 1e-12);
        assert_eq!(m.recall_at_10, 0.0);
        assert_eq!(m.ndcg_at_10, 0.0);

        // Empty gold: all zeros, no division by zero.
        let m = rank_metrics(&retrieved, &BTreeSet::new());
        assert_eq!((m.hit_at_10, m.reciprocal_rank, m.recall_at_10, m.ndcg_at_10), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn finish_metrics_hand_computed_means_exclude_item_errors() {
        fn item(hit: f64, rr: f64, recall: f64, error: Option<&str>) -> ItemOutcome {
            ItemOutcome {
                dataset: "locomo",
                split: Split::Dev,
                id: "fixture".to_owned(),
                category: "1".to_owned(),
                dispositions: DispositionCounts::default(),
                relevant_promoted_ids: Vec::new(),
                retrieved_ids: Vec::new(),
                startup_context_bytes: 0,
                startup_context_memory_ids: Vec::new(),
                startup_coverage: 0.0,
                search_hit_count: 0,
                search_empty: true,
                hit_at_10: hit,
                reciprocal_rank: rr,
                recall_at_10: recall,
                ndcg_at_10: 0.0,
                context_exact_match: false,
                context_contains: false,
                unmatched_evidence: Vec::new(),
                item_error: error.map(str::to_owned),
                judge: None,
                judge_error: None,
            }
        }

        let mut report = BaselineReport {
            schema_version: "test",
            report_name: "test",
            ranking_lanes: Vec::new(),
            vector_lane: "test",
            dataset_sha256s: BTreeMap::new(),
            enrichment_provenance: BTreeMap::new(),
            split_config: SplitConfig {
                generation: Generation::V1,
                enrichment_prompt_sha256: None,
                enrichment_window_policy: None,
                splits: vec![Split::Dev],
                locomo_conversation_limit: None,
                locomo_qa_per_conversation: None,
                longmemeval_per_split: 0,
                longmemeval_cleaned: false,
                embedding_lane: "test",
                fusion: "legacy",
                fusion_weights: None,
                excluded_keys: Vec::new(),
            },
            sampling: SamplingReport::default(),
            dispositions: DispositionCounts::default(),
            governance_drag: GovernanceDrag::default(),
            metrics: MetricReport::default(),
            ingestion: Vec::new(),
            enrichment: EnrichmentIngestionCounts::default(),
            items: vec![
                item(1.0, 0.5, 0.5, None),
                item(0.0, 0.0, 0.0, None),
                // Excluded item carries perfect scores that MUST NOT leak into means.
                item(1.0, 1.0, 1.0, Some("dangling session")),
            ],
            judge_inputs: Vec::new(),
            judge_identity: None,
        };
        finish_metrics(&mut report);
        // Hand-computed over the 2 scored items only.
        assert_eq!(report.metrics.scored_items, 2);
        assert_eq!(report.metrics.excluded_items, 1);
        assert_eq!(report.metrics.hit_at_10, 0.5);
        assert_eq!(report.metrics.mrr, 0.25);
        assert_eq!(report.metrics.recall_at_10, 0.25);
    }

    #[test]
    fn hit_at_ten_and_recall_at_ten_documented() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[
  {
    "sample_id": "locomo-fixture-0",
    "conversation": {
      "session_1_date_time": "8 May 2023",
      "session_1": [
        {"speaker": "Caroline", "dia_id": "D1:1", "text": "I attended the support group yesterday."}
      ]
    },
    "qa": [
      {"question": "When was the group?", "answer": "7 May 2023", "evidence": ["D1:1"], "category": 2}
    ]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.locomo_conversation_limit = Some(1);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let item = report.items.iter().find(|i| i.dataset == "locomo").expect("locomo item");
        assert!(item.retrieved_ids.len() == 1 && item.retrieved_ids[0].starts_with("mem_"));
        assert!(item.relevant_promoted_ids.len() == 1 && item.relevant_promoted_ids[0].starts_with("mem_"));
        assert!(item.startup_context_memory_ids.iter().all(|id| id.starts_with("mem_")));
        assert_eq!(item.search_hit_count, 1);
        assert!(!item.search_empty);
        assert_eq!(item.hit_at_10, 1.0);
        assert_eq!(item.recall_at_10, 1.0);
        assert_eq!(item.reciprocal_rank, 1.0);
        assert_eq!(item.ndcg_at_10, 1.0);
        assert_eq!(item.startup_coverage, 1.0);
        assert!(!item.context_exact_match);
        assert!(!item.context_contains);
        assert!(item.unmatched_evidence.is_empty());
        assert!(item.item_error.is_none());
        assert!(item.judge.as_ref().is_some_and(|v| v.score == 0.0));

        assert_eq!(report.metrics.scored_items, 1);
        assert_eq!(report.metrics.excluded_items, 0);
        assert_eq!(report.metrics.hit_at_10, 1.0);
        assert_eq!(report.metrics.recall_at_10, 1.0);
        assert_eq!(report.metrics.mrr, 1.0);
        assert_eq!(report.metrics.ndcg_at_10, 1.0);
        assert_eq!(report.metrics.startup_coverage, 1.0);
        assert_eq!(report.metrics.context_exact_match, 0.0);
        assert_eq!(report.metrics.context_contains, 0.0);
        assert_eq!(report.metrics.judge_mean, Some(0.0));

        // Metrics round-trip through JSON without field loss.
        let json = serde_json::to_string(&report.metrics).expect("serialize metrics");
        let round: MetricReport = serde_json::from_str(&json).expect("deserialize metrics");
        assert_eq!(round.scored_items, report.metrics.scored_items);
        assert_eq!(round.excluded_items, report.metrics.excluded_items);
        assert_eq!(round.hit_at_10, report.metrics.hit_at_10);
    }

    #[test]
    fn corpus_isolation_per_item() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_locomo_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "longmemeval/longmemeval_oracle.json",
            r#"[
  {
    "question_id": "item-2",
    "question_type": "test",
    "question": "Which system failed?",
    "answer": "GPS",
    "haystack_dates": ["2023/04/09"],
    "haystack_session_ids": ["answer-a"],
    "haystack_sessions": [[{"role": "user", "content": "The GPS failed.", "has_answer": true}]],
    "answer_session_ids": ["answer-a"]
  },
  {
    "question_id": "item-6",
    "question_type": "test",
    "question": "Which system failed?",
    "answer": "GPS",
    "haystack_dates": ["2023/04/10"],
    "haystack_session_ids": ["answer-b"],
    "haystack_sessions": [[{"role": "user", "content": "The GPS failed.", "has_answer": true}]],
    "answer_session_ids": ["answer-b"]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.longmemeval_per_split = 2;
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let longmem_items: Vec<_> = report.items.iter().filter(|item| item.dataset == "longmemeval").collect();
        assert_eq!(longmem_items.len(), 2);
        for (i, a) in longmem_items.iter().enumerate() {
            for (j, b) in longmem_items.iter().enumerate() {
                if i == j {
                    continue;
                }
                for relevant in &b.relevant_promoted_ids {
                    assert!(!a.retrieved_ids.contains(relevant));
                }
            }
        }
    }

    #[test]
    fn lane_truth_startup_coverage() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[
  {
    "sample_id": "locomo-fixture-0",
    "conversation": {
      "session_1_date_time": "8 May 2023",
      "session_1": [
        {"speaker": "Caroline", "dia_id": "D1:1", "text": "I attended the support group yesterday."}
      ]
    },
    "qa": [
      {"question": "When was the group?", "answer": "7 May 2023", "evidence": ["D1:1"], "category": 2}
    ]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.locomo_conversation_limit = Some(1);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let item = report.items.iter().find(|item| item.dataset == "locomo").expect("locomo item");
        assert!(item.startup_context_bytes > 0);
        assert!(!item.startup_context_memory_ids.is_empty());
        assert!(item.startup_coverage >= 0.0);
        assert!(item.search_hit_count <= TOP_K);
        // search_empty is a boolean field; it is present by compilation.
        let _ = item.search_empty;
    }

    #[test]
    fn longmemeval_gold_has_answer() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_locomo_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "longmemeval/longmemeval_oracle.json",
            r#"[
  {
    "question_id": "gold-3",
    "question_type": "test",
    "question": "Which system failed?",
    "answer": "GPS",
    "haystack_dates": ["2023/04/09"],
    "haystack_session_ids": ["answer-a"],
    "haystack_sessions": [[
      {"role": "user", "content": "The GPS failed.", "has_answer": true},
      {"role": "assistant", "content": "Sorry to hear that.", "has_answer": false}
    ]],
    "answer_session_ids": ["answer-a"]
  },
  {
    "question_id": "gold-5",
    "question_type": "test",
    "question": "Which system failed?",
    "answer": "GPS",
    "haystack_dates": ["2023/04/10"],
    "haystack_session_ids": ["answer-b"],
    "haystack_sessions": [[
      {"role": "user", "content": "The GPS failed.", "has_answer": false},
      {"role": "assistant", "content": "Sorry to hear that.", "has_answer": false}
    ]],
    "answer_session_ids": ["answer-b"]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.longmemeval_per_split = 2;
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let longmem_items: Vec<_> = report.items.iter().filter(|item| item.dataset == "longmemeval").collect();
        assert!(longmem_items.iter().any(|item| item.relevant_promoted_ids.len() == 1));
        assert!(longmem_items.iter().any(|item| item.relevant_promoted_ids.len() == 2));
    }

    #[test]
    fn sensitivity_expected_vs_observed() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_locomo_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "longmemeval/longmemeval_oracle.json",
            r#"[
  {
    "question_id": "sensitivity-a",
    "question_type": "test",
    "question": "What is the status?",
    "answer": "internal",
    "haystack_dates": ["2023/04/09"],
    "haystack_session_ids": ["answer-a"],
    "haystack_sessions": [[{"role": "user", "content": "The internal status is nominal.", "has_answer": true}]],
    "answer_session_ids": ["answer-a"]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.expected_sensitivity = "public".to_owned();
        config.longmemeval_per_split = 1;
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        assert!(
            report.governance_drag.expected_actual_mismatches > 0
                || report
                    .governance_drag
                    .by_source_kind
                    .values()
                    .map(|drag| drag.encrypted_not_retrievable)
                    .sum::<usize>()
                    > 0
        );
    }

    #[test]
    fn longmemeval_haystack_footprint() {
        let mut items = vec![
            LongMemItem {
                question_id: "a".to_owned(),
                question_type: "test".to_owned(),
                question: "q".to_owned(),
                answer: json!("answer a"),
                haystack_dates: vec!["d1".to_owned()],
                haystack_session_ids: vec!["s1".to_owned()],
                haystack_sessions: vec![vec![LongMemTurn {
                    role: "user".to_owned(),
                    content: "body a".to_owned(),
                    dia_id: None,
                    has_answer: true,
                }]],
                answer_session_ids: vec!["s1".to_owned()],
            },
            LongMemItem {
                question_id: "b".to_owned(),
                question_type: "test".to_owned(),
                question: "q".to_owned(),
                answer: json!("answer b"),
                haystack_dates: vec!["d2".to_owned()],
                haystack_session_ids: vec!["s2".to_owned()],
                haystack_sessions: vec![vec![LongMemTurn {
                    role: "user".to_owned(),
                    content: "body b".to_owned(),
                    dia_id: None,
                    has_answer: true,
                }]],
                answer_session_ids: vec!["s2".to_owned()],
            },
        ];
        let selected = balanced_longmem_items(&mut items, 1);
        assert_eq!(selected.len(), 1);
        let selected_ids: BTreeSet<String> = selected.iter().map(|s| s.question_id.clone()).collect();
        drop(selected);
        let non_selected = items.iter().find(|i| !selected_ids.contains(&i.question_id)).expect("non-selected exists");
        assert!(non_selected.haystack_sessions.is_empty());
        assert!(non_selected.haystack_session_ids.is_empty());
        assert!(non_selected.haystack_dates.is_empty());
    }

    #[test]
    fn search_hits_collapsed_to_memory_level() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[
  {
    "sample_id": "locomo-fixture-0",
    "conversation": {
      "session_1_date_time": "8 May 2023",
      "session_1": [
        {"speaker": "Caroline", "dia_id": "D1:1", "text": "I attended the support group yesterday."}
      ]
    },
    "qa": [
      {"question": "When was the group?", "answer": "7 May 2023", "evidence": ["D1:1"], "category": 2}
    ]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.locomo_conversation_limit = Some(1);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let item = report.items.iter().find(|item| item.dataset == "locomo").expect("locomo item");
        let unique: BTreeSet<_> = item.retrieved_ids.iter().collect();
        assert_eq!(unique.len(), item.retrieved_ids.len(), "retrieved_ids must be unique");
        assert!(item.search_hit_count <= TOP_K);
    }

    #[test]
    fn context_evidence_metrics_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[
  {
    "sample_id": "locomo-fixture-0",
    "conversation": {
      "session_1_date_time": "8 May 2023",
      "session_1": [
        {"speaker": "Caroline", "dia_id": "D1:1", "text": "I attended the support group yesterday."}
      ]
    },
    "qa": [
      {"question": "When was the group?", "answer": "7 May 2023", "evidence": ["D1:1"], "category": 2}
    ]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.locomo_conversation_limit = Some(1);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        assert!(report.metrics.context_exact_match >= 0.0 && report.metrics.context_exact_match <= 1.0);
        assert!(report.metrics.context_contains >= 0.0 && report.metrics.context_contains <= 1.0);
        let item = report.items.iter().find(|item| item.dataset == "locomo").expect("locomo item");
        let _ = item.context_exact_match;
        let _ = item.context_contains;
    }

    #[test]
    fn split_parity() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        write_fixture(
            dataset_dir,
            "locomo/locomo10.json",
            r#"[
  {
    "sample_id": "locomo-fixture-0",
    "conversation": {
      "session_1_date_time": "8 May 2023",
      "session_1": [
        {"speaker": "Caroline", "dia_id": "D1:1", "text": "I attended the support group yesterday."}
      ]
    },
    "qa": [
      {"question": "When was the group?", "answer": "7 May 2023", "evidence": ["D1:1"], "category": 2}
    ]
  },
  {
    "sample_id": "locomo-fixture-1",
    "conversation": {
      "session_1_date_time": "9 May 2023",
      "session_1": [
        {"speaker": "Alex", "dia_id": "D1:1", "text": "The code is blue."}
      ]
    },
    "qa": [
      {"question": "What color?", "answer": "blue", "evidence": ["D1:1"], "category": 1}
    ]
  }
]"#,
        );
        let mut config = fts_config(dataset_dir);
        config.splits = vec![Split::Dev, Split::Holdout];
        config.locomo_conversation_limit = Some(2);
        config.locomo_qa_per_conversation = Some(1);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        let locomo_items: Vec<_> = report.items.iter().filter(|item| item.dataset == "locomo").collect();
        assert!(locomo_items.iter().any(|item| item.split == Split::Dev));
        assert!(locomo_items.iter().any(|item| item.split == Split::Holdout));
    }

    #[test]
    fn bounded_auto_approve_retries_then_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("approve.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let responses = vec!["candidate", "candidate", "approved"];
        let handle = std::thread::spawn(move || {
            let status_iter = responses.into_iter();
            for status in status_iter {
                let response =
                    json!({"result": {"success": {"review_approve": {"id": "mem_1", "status": status, "summary": "x"}}}})
                        .to_string();
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut _line = String::new();
                let _ = reader.read_line(&mut _line);
                drop(reader);
                let _ = stream.write_all(format!("{response}\n").as_bytes());
            }
        });
        let project = dir.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let mut daemon = DaemonClient::new(&socket, &project);
        assert!(approve_until(&mut daemon, "mem_1", 3).unwrap());
        handle.join().unwrap();
    }

    #[test]
    fn bounded_auto_approve_returns_false_after_max_attempts() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("approve-fail.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..3 {
                let response =
                    json!({"result": {"success": {"review_approve": {"id": "mem_1", "status": "candidate", "summary": "x"}}}})
                        .to_string();
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut _line = String::new();
                let _ = reader.read_line(&mut _line);
                drop(reader);
                let _ = stream.write_all(format!("{response}\n").as_bytes());
            }
        });
        let project = dir.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let mut daemon = DaemonClient::new(&socket, &project);
        assert!(!approve_until(&mut daemon, "mem_1", 3).unwrap());
        handle.join().unwrap();
    }

    #[test]
    fn artifact_identity() {
        let dir = tempfile::tempdir().unwrap();
        let dataset_dir = dir.path();
        empty_longmemeval_fixture(dataset_dir);
        empty_locomo_fixture(dataset_dir);
        let config = fts_config(dataset_dir);
        let report = crate::block_on(run_baseline(&config, Some(&DeterministicMockJudge))).expect("baseline");
        assert_eq!(report.schema_version, "baseline_0.1");
        assert!(!report.dataset_sha256s.is_empty());
        assert_eq!(report.split_config.generation, Generation::V1);
        assert_eq!(report.split_config.enrichment_prompt_sha256, None);
        assert_eq!(report.split_config.enrichment_window_policy, None);
        assert_eq!(report.split_config.splits, vec![Split::Dev]);
        assert_eq!(report.judge_identity, Some("deterministic_mock".to_owned()));
    }
}
