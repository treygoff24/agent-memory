use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Duration, Utc};
use memory_substrate::{
    index::{open_index, Index},
    InitOptions, MemoryId, MemoryStatus, MemoryType, RecallIndexRow, RepoPath, Roots, Scope, Sensitivity, SourceKind,
    Substrate,
};
use memoryd::reality_check::{
    confidence_decay, days_since_observed_norm, score_memories_at, sensitivity_weight, ScoreWeights, ScoringConfig,
};
use tempfile::TempDir;

const PERFORMANCE_MEMORY_COUNT: usize = 10_000;
const PERFORMANCE_SAMPLE_COUNT: usize = 5;
const PERFORMANCE_P95_BUDGET: StdDuration = StdDuration::from_millis(500);
const PERFORMANCE_GATE_ENV: &str = "MEMORUM_SCORING_PERF_GATE";

#[tokio::test]
async fn test_score_formula_staleness_only() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let item = row(1).with_confidence(0.8).with_sensitivity(Sensitivity::Public);
    let corroborating = row(2);
    insert_memory(context.index(), &item, MemoryDbFields::new(now - Duration::days(90)).with_harness("codex"));
    insert_memory(context.index(), &corroborating, MemoryDbFields::new(now).with_harness("claude-code"));
    insert_supersession(context.index(), &item.id, &corroborating.id);
    insert_recall_hits(context.index(), &item.id, now, 3);

    let scored = score_memories_at(&[item], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_approx(scored[0].score, 0.35);
}

#[tokio::test]
async fn test_score_formula_all_components() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let target = row(1).with_confidence(0.5).with_sensitivity(Sensitivity::Confidential);
    let active_max = row(2).with_confidence(0.8);
    insert_memory(
        context.index(),
        &target,
        MemoryDbFields::new(now - Duration::days(45)).with_original_confidence(Some(0.7)).with_harness("codex"),
    );
    insert_memory(context.index(), &active_max, MemoryDbFields::new(now).with_harness("codex"));
    insert_recall_hits(context.index(), &target.id, now, 1);
    insert_recall_hits(context.index(), &active_max.id, now, 4);

    let scored =
        score_memories_at(&[target.clone(), active_max], &context.substrate, &ScoringConfig::with_top_n(2), now)
            .unwrap();
    let target_score = scored.iter().find(|item| item.memory_id == target.id).unwrap();

    assert_approx(target_score.score, 0.35 * 0.5 + 0.20 * 0.75 + 0.20 + 0.15 * 0.2 + 0.10 * 0.6);
}

#[test]
fn test_score_saturation_at_90_days() {
    let now = instant("2026-05-01T12:00:00Z");
    assert_approx(days_since_observed_norm(now - Duration::days(120), now), 1.0);
}

#[test]
fn test_score_below_90_days_proportional() {
    let now = instant("2026-05-01T12:00:00Z");
    assert_approx(days_since_observed_norm(now - Duration::days(45), now), 0.5);
}

#[tokio::test]
async fn test_corroboration_requires_two_distinct_harnesses() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let current = row(1);
    let previous = row(2);
    insert_memory(context.index(), &current, MemoryDbFields::new(now).with_harness("codex"));
    insert_memory(context.index(), &previous, MemoryDbFields::new(now).with_harness("codex"));
    insert_supersession(context.index(), &current.id, &previous.id);

    let scored = score_memories_at(&[current], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored[0].component_scores.cross_source_corroboration, 0.0);
}

#[tokio::test]
async fn test_corroboration_satisfied_by_two_harnesses() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let current = row(1);
    let previous = row(2);
    insert_memory(context.index(), &current, MemoryDbFields::new(now).with_harness("codex"));
    insert_memory(context.index(), &previous, MemoryDbFields::new(now).with_harness("claude-code"));
    insert_supersession(context.index(), &current.id, &previous.id);

    let scored = score_memories_at(&[current], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored[0].component_scores.cross_source_corroboration, 1.0);
}

#[tokio::test]
async fn test_corroboration_walks_supersession_chain_depth_bounded() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let rows = (1..=10).map(row).collect::<Vec<_>>();
    for (index, item) in rows.iter().enumerate() {
        let harness = if index % 2 == 0 { "codex" } else { "claude-code" };
        insert_memory(context.index(), item, MemoryDbFields::new(now).with_harness(harness));
    }
    for pair in rows.windows(2) {
        insert_supersession(context.index(), &pair[0].id, &pair[1].id);
    }

    let scored = score_memories_at(&[rows[0].clone()], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored[0].component_scores.cross_source_corroboration, 1.0);
}

#[tokio::test]
async fn test_corroboration_recursive_cte_handles_cycle_via_depth_bound() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let a = row(1);
    let b = row(2);
    insert_memory(context.index(), &a, MemoryDbFields::new(now).with_harness("codex"));
    insert_memory(context.index(), &b, MemoryDbFields::new(now).with_harness("claude-code"));
    insert_supersession(context.index(), &a.id, &b.id);
    insert_supersession(context.index(), &b.id, &a.id);

    let scored = score_memories_at(&[a], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored[0].component_scores.cross_source_corroboration, 1.0);
    assert!(scored[0].score.is_finite());
}

#[tokio::test]
async fn test_corroboration_null_source_harness_does_not_count_as_distinct() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let current = row(1);
    let previous = row(2);
    insert_memory(context.index(), &current, MemoryDbFields::new(now).with_harness("codex"));
    insert_memory(context.index(), &previous, MemoryDbFields::new(now));
    insert_supersession(context.index(), &current.id, &previous.id);

    let scored = score_memories_at(&[current], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored[0].component_scores.cross_source_corroboration, 0.0);
}

#[tokio::test]
async fn test_corroboration_two_non_null_harnesses_with_one_null_in_chain_yields_corroboration() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let a = row(1);
    let b = row(2);
    let c = row(3);
    insert_memory(context.index(), &a, MemoryDbFields::new(now));
    insert_memory(context.index(), &b, MemoryDbFields::new(now).with_harness("claude-code"));
    insert_memory(context.index(), &c, MemoryDbFields::new(now).with_harness("codex"));
    insert_supersession(context.index(), &a.id, &b.id);
    insert_supersession(context.index(), &b.id, &c.id);

    let scored = score_memories_at(&[a], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored[0].component_scores.cross_source_corroboration, 1.0);
}

#[test]
fn test_sensitivity_weights_map_correctly() {
    assert_eq!(sensitivity_weight(Sensitivity::Public), 0.0);
    assert_eq!(sensitivity_weight(Sensitivity::Internal), 0.3);
    assert_eq!(sensitivity_weight(Sensitivity::Confidential), 0.6);
    assert_eq!(sensitivity_weight(Sensitivity::Personal), 1.0);
}

#[test]
fn test_confidence_decay_clamped_to_zero() {
    assert_eq!(confidence_decay(Some(0.4), 0.8), 0.0);
}

#[test]
fn test_confidence_decay_none_baseline_yields_zero() {
    assert_eq!(confidence_decay(None, 0.2), 0.0);
}

#[tokio::test]
async fn test_encrypted_memory_scored_from_index_only() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let encrypted = row(1).with_path("encrypted/mem_20260501_0000000000000001_000001.md");
    insert_memory(context.index(), &encrypted, MemoryDbFields::new(now).with_harness("codex").encrypted());

    let scored = score_memories_at(&[encrypted], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert!(scored[0].encrypted);
    assert!(scored[0].score.is_finite());
}

#[tokio::test]
async fn test_top_n_selection_respects_cap() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let rows = (1..=20).map(|n| row(n).with_updated_at(now - Duration::days(n as i64))).collect::<Vec<_>>();
    for item in &rows {
        insert_memory(context.index(), item, MemoryDbFields::new(item.updated_at).with_harness("codex"));
    }

    let scored = score_memories_at(&rows, &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored.len(), 12);
}

#[tokio::test]
async fn test_pinned_memories_always_included() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let mut rows = (1..=12).map(|n| row(n).with_updated_at(now - Duration::days(90))).collect::<Vec<_>>();
    let pinned = row(13).with_status(MemoryStatus::Pinned).with_updated_at(now);
    rows.push(pinned.clone());
    for item in &rows {
        insert_memory(context.index(), item, MemoryDbFields::new(item.updated_at).with_harness("codex"));
    }

    let scored = score_memories_at(&rows, &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert_eq!(scored.len(), 12);
    assert!(scored.iter().any(|item| item.memory_id == pinned.id));
}

#[tokio::test]
async fn test_excluded_statuses_not_scored() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let rows = [
        MemoryStatus::Candidate,
        MemoryStatus::Quarantined,
        MemoryStatus::Tombstoned,
        MemoryStatus::Archived,
        MemoryStatus::Superseded,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, status)| row(index + 1).with_status(status))
    .collect::<Vec<_>>();
    for item in &rows {
        insert_memory(context.index(), item, MemoryDbFields::new(now).with_harness("codex"));
    }

    let scored = score_memories_at(&rows, &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert!(scored.is_empty());
}

#[tokio::test]
async fn test_passive_recall_false_excluded() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let item = row(1).with_passive_recall(false);
    insert_memory(context.index(), &item, MemoryDbFields::new(now).with_harness("codex"));

    let scored = score_memories_at(&[item], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert!(scored.is_empty());
}

#[tokio::test]
async fn test_score_bounded_zero_to_one() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let item = row(1).with_confidence(-10.0).with_sensitivity(Sensitivity::Personal);
    insert_memory(
        context.index(),
        &item,
        MemoryDbFields::new(now - Duration::days(365)).with_original_confidence(Some(10.0)).with_harness("codex"),
    );

    let scored = score_memories_at(&[item], &context.substrate, &ScoringConfig::default(), now).unwrap();

    assert!((0.0..=1.0).contains(&scored[0].score));
}

#[tokio::test]
async fn test_invalid_weight_config_falls_back_to_defaults() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let item = row(1).with_sensitivity(Sensitivity::Public);
    let corroborating = row(2);
    insert_memory(context.index(), &item, MemoryDbFields::new(now - Duration::days(90)).with_harness("codex"));
    insert_memory(context.index(), &corroborating, MemoryDbFields::new(now).with_harness("claude-code"));
    insert_supersession(context.index(), &item.id, &corroborating.id);
    insert_recall_hits(context.index(), &item.id, now, 1);
    let config = ScoringConfig {
        top_n: 12,
        weights: ScoreWeights {
            staleness: 1.1,
            recall_frequency: 0.0,
            cross_source_corroboration: 0.0,
            confidence_decay: 0.0,
            sensitivity: 0.0,
        },
    };

    let scored = score_memories_at(&[item], &context.substrate, &config, now).unwrap();

    assert_approx(scored[0].score, 0.35);
}

#[tokio::test]
async fn test_score_memories_at_10k_fixture_returns_all_finite_scores() {
    let context = TestContext::new().await;
    let now = instant("2026-05-01T12:00:00Z");
    let rows = (1..=PERFORMANCE_MEMORY_COUNT).map(performance_row).collect::<Vec<_>>();
    let index = context.index();
    insert_performance_fixture(&index, &rows, now);

    let config = ScoringConfig::with_top_n(PERFORMANCE_MEMORY_COUNT);
    let scored = score_memories_at(&rows, &context.substrate, &config, now).unwrap();

    assert_eq!(scored.len(), PERFORMANCE_MEMORY_COUNT);
    assert!(scored.iter().all(|item| item.score.is_finite()));

    if std::env::var_os(PERFORMANCE_GATE_ENV).is_some() {
        assert_score_memories_p95_under_budget(&rows, &context.substrate, &config, now);
    }
}

fn assert_score_memories_p95_under_budget(
    rows: &[RecallIndexRow],
    substrate: &Substrate,
    config: &ScoringConfig,
    now: DateTime<Utc>,
) {
    let mut durations = Vec::with_capacity(PERFORMANCE_SAMPLE_COUNT);
    let mut scored_count = 0usize;
    for _ in 0..PERFORMANCE_SAMPLE_COUNT {
        let started = Instant::now();
        let scored = score_memories_at(rows, substrate, config, now).unwrap();
        durations.push(started.elapsed());
        scored_count = scored.len();
        assert!(scored.iter().all(|item| item.score.is_finite()));
    }

    let p95 = percentile_p95(durations);
    assert_eq!(scored_count, PERFORMANCE_MEMORY_COUNT);
    assert!(p95 <= PERFORMANCE_P95_BUDGET, "score_memories_at 10k p95 {:?} exceeded {:?}", p95, PERFORMANCE_P95_BUDGET);
}

struct TestContext {
    _repo: TempDir,
    runtime: TempDir,
    substrate: Substrate,
}

impl TestContext {
    async fn new() -> Self {
        let repo = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let substrate = Substrate::init(
            Roots::new(repo.path(), runtime.path()),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_scoring".to_string()) },
        )
        .await
        .unwrap();
        Self { _repo: repo, runtime, substrate }
    }

    fn index(&self) -> Index {
        Index::new(open_index(&self.runtime.path().join("index.sqlite")).unwrap())
    }
}

#[derive(Clone, Debug)]
struct MemoryDbFields {
    observed_at: DateTime<Utc>,
    original_confidence: Option<f64>,
    source_harness: Option<String>,
    encrypted: bool,
}

impl MemoryDbFields {
    fn new(observed_at: DateTime<Utc>) -> Self {
        Self { observed_at, original_confidence: None, source_harness: None, encrypted: false }
    }

    fn with_original_confidence(mut self, original_confidence: Option<f64>) -> Self {
        self.original_confidence = original_confidence;
        self
    }

    fn with_harness(mut self, source_harness: &str) -> Self {
        self.source_harness = Some(source_harness.to_string());
        self
    }

    fn encrypted(mut self) -> Self {
        self.encrypted = true;
        self
    }
}

trait RowFixture {
    fn with_confidence(self, confidence: f64) -> Self;
    fn with_sensitivity(self, sensitivity: Sensitivity) -> Self;
    fn with_status(self, status: MemoryStatus) -> Self;
    fn with_updated_at(self, updated_at: DateTime<Utc>) -> Self;
    fn with_path(self, path: &str) -> Self;
    fn with_passive_recall(self, passive_recall: bool) -> Self;
}

impl RowFixture for RecallIndexRow {
    fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence;
        self
    }

    fn with_sensitivity(mut self, sensitivity: Sensitivity) -> Self {
        self.sensitivity = sensitivity;
        self
    }

    fn with_status(mut self, status: MemoryStatus) -> Self {
        self.status = status;
        self
    }

    fn with_updated_at(mut self, updated_at: DateTime<Utc>) -> Self {
        self.updated_at = updated_at;
        self
    }

    fn with_path(mut self, path: &str) -> Self {
        self.path = RepoPath::new(path);
        self
    }

    fn with_passive_recall(mut self, passive_recall: bool) -> Self {
        self.passive_recall = passive_recall;
        self
    }
}

fn row(n: usize) -> RecallIndexRow {
    let id = id(n);
    RecallIndexRow {
        id: MemoryId::new(id.clone()),
        path: RepoPath::new(format!("me/{id}.md")),
        summary: format!("summary {n}"),
        memory_type: MemoryType::Pattern,
        status: MemoryStatus::Active,
        scope: Scope::User,
        canonical_namespace_id: None,
        updated_at: instant("2026-05-01T12:00:00Z"),
        indexed_at: instant("2026-05-01T12:00:00Z"),
        confidence: 0.8,
        source_kind: SourceKind::AgentPrimary,
        source_device: None,
        sensitivity: Sensitivity::Internal,
        passive_recall: true,
        index_body: true,
        requires_user_confirmation: false,
        review_state: None,
        human_review_required: false,
        max_scope: Scope::User,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: Vec::new(),
    }
}

fn insert_memory(index: Index, row: &RecallIndexRow, fields: MemoryDbFields) {
    index
        .connection()
        .execute(
            "INSERT OR REPLACE INTO memories(
                id, path, schema_version, type, scope, namespace, canonical_namespace_id,
                summary, confidence, original_confidence, trust_level, sensitivity, status, review_state,
                requires_user_confirmation, created_at, updated_at,
                observed_at, valid_from, valid_until, ttl,
                author, source_kind, source_harness, source_device,
                body_hash, frontmatter_json, file_hash, file_mtime_ns, indexed_at, metadata_only,
                passive_recall, index_body, human_review_required, max_scope
             ) VALUES (
                ?1, ?2, 1, 'pattern', 'user', NULL, NULL,
                ?3, ?4, ?5, 'trusted', ?6, ?7, NULL,
                0, ?8, ?9,
                ?10, NULL, NULL, NULL,
                'agent', 'agent-primary', ?11, NULL,
                'hash', '{}', 'hash', 0, ?12, ?13,
                ?14, ?15, 0, 'user'
             )",
            (
                row.id.as_str(),
                row.path.as_str(),
                row.summary.as_str(),
                row.confidence,
                fields.original_confidence,
                sensitivity_str(row.sensitivity),
                status_str(row.status),
                row.updated_at.to_rfc3339(),
                row.updated_at.to_rfc3339(),
                fields.observed_at.to_rfc3339(),
                fields.source_harness,
                row.indexed_at.to_rfc3339(),
                fields.encrypted as i64,
                row.passive_recall as i64,
                row.index_body as i64,
            ),
        )
        .unwrap();
}

fn insert_supersession(index: Index, memory_id: &MemoryId, supersedes_id: &MemoryId) {
    index
        .connection()
        .execute(
            "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id) VALUES (?1, ?2)",
            (memory_id.as_str(), supersedes_id.as_str()),
        )
        .unwrap();
}

fn insert_recall_hits(index: Index, memory_id: &MemoryId, now: DateTime<Utc>, count: usize) {
    for seq in 0..count {
        let event_id = format!("evt_{}_{}", memory_id.as_str(), seq);
        index
            .connection()
            .execute(
                "INSERT OR REPLACE INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
                 VALUES (?1, 'dev_scoring', ?2, 'recall_hit', ?3, ?4, '{}')",
                (event_id, seq as i64, memory_id.as_str(), (now - Duration::days(seq as i64)).to_rfc3339()),
            )
            .unwrap();
    }
}

fn performance_row(n: usize) -> RecallIndexRow {
    row(n)
        .with_updated_at(instant("2026-05-01T12:00:00Z") - Duration::hours((n % 240) as i64))
        .with_confidence(0.45 + f64::from((n % 50) as u32) / 100.0)
        .with_sensitivity(match n % 4 {
            0 => Sensitivity::Public,
            1 => Sensitivity::Internal,
            2 => Sensitivity::Confidential,
            _ => Sensitivity::Personal,
        })
        .with_status(if n.is_multiple_of(199) { MemoryStatus::Pinned } else { MemoryStatus::Active })
}

fn insert_performance_fixture(index: &Index, rows: &[RecallIndexRow], now: DateTime<Utc>) {
    let connection = index.connection();
    connection.execute_batch("BEGIN IMMEDIATE TRANSACTION").unwrap();

    match insert_performance_fixture_rows(connection, rows, now) {
        Ok(()) => connection.execute_batch("COMMIT").unwrap(),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            panic!("failed to insert performance fixture: {error}");
        }
    }
}

fn insert_performance_fixture_rows(
    connection: &rusqlite::Connection,
    rows: &[RecallIndexRow],
    now: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let mut memory_statement = connection.prepare(
        "INSERT OR REPLACE INTO memories(
            id, path, schema_version, type, scope, namespace, canonical_namespace_id,
            summary, confidence, original_confidence, trust_level, sensitivity, status, review_state,
            requires_user_confirmation, created_at, updated_at,
            observed_at, valid_from, valid_until, ttl,
            author, source_kind, source_harness, source_device,
            body_hash, frontmatter_json, file_hash, file_mtime_ns, indexed_at, metadata_only,
            passive_recall, index_body, human_review_required, max_scope
         ) VALUES (
            ?1, ?2, 1, 'pattern', 'user', NULL, NULL,
            ?3, ?4, ?5, 'trusted', ?6, ?7, NULL,
            0, ?8, ?9,
            ?10, NULL, NULL, NULL,
            'agent', 'agent-primary', ?11, NULL,
            ?12, '{}', ?13, 0, ?14, ?15,
            ?16, ?17, 0, 'user'
         )",
    )?;
    let mut event_statement = connection.prepare(
        "INSERT OR REPLACE INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
         VALUES (?1, 'dev_scoring', ?2, 'recall_hit', ?3, ?4, '{}')",
    )?;
    let mut supersession_statement =
        connection.prepare("INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id) VALUES (?1, ?2)")?;

    let mut event_sequence = 0i64;
    for (index, item) in rows.iter().enumerate() {
        let observed_at = now - Duration::days((index % 120) as i64);
        memory_statement.execute(rusqlite::params![
            item.id.as_str(),
            item.path.as_str(),
            item.summary.as_str(),
            item.confidence,
            original_confidence(index),
            sensitivity_str(item.sensitivity),
            status_str(item.status),
            item.updated_at.to_rfc3339(),
            item.updated_at.to_rfc3339(),
            observed_at.to_rfc3339(),
            source_harness(index),
            format!("body_hash_{index:08}"),
            format!("file_hash_{index:08}"),
            item.indexed_at.to_rfc3339(),
            metadata_only(index) as i64,
            item.passive_recall as i64,
            item.index_body as i64,
        ])?;

        for hit_index in 0..(index % 7) {
            event_statement.execute((
                format!("evt_scoring_perf_{index:05}_{hit_index:02}"),
                event_sequence,
                item.id.as_str(),
                (now - Duration::days(hit_index as i64)).to_rfc3339(),
            ))?;
            event_sequence += 1;
        }

        if index % 4 == 0 && index > 0 {
            supersession_statement.execute((item.id.as_str(), rows[index - 1].id.as_str()))?;
        }
    }
    Ok(())
}

fn source_harness(index: usize) -> &'static str {
    match index % 4 {
        0 => "codex",
        1 => "claude-code",
        2 => "ci",
        _ => "operator",
    }
}

fn original_confidence(index: usize) -> Option<f64> {
    (!index.is_multiple_of(3)).then(|| 0.55 + f64::from((index % 40) as u32) / 100.0)
}

fn metadata_only(index: usize) -> bool {
    index.is_multiple_of(17)
}

fn percentile_p95(mut durations: Vec<StdDuration>) -> StdDuration {
    durations.sort_unstable();
    let index = ((durations.len().saturating_sub(1)) as f64 * 0.95).ceil() as usize;
    durations[index.min(durations.len().saturating_sub(1))]
}

fn id(n: usize) -> String {
    format!("mem_20260501_{n:016x}_{n:06}")
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).unwrap().with_timezone(&Utc)
}

fn sensitivity_str(value: Sensitivity) -> &'static str {
    match value {
        Sensitivity::Public => "public",
        Sensitivity::Internal => "internal",
        Sensitivity::Confidential => "confidential",
        Sensitivity::Personal => "personal",
    }
}

fn status_str(value: MemoryStatus) -> &'static str {
    match value {
        MemoryStatus::Candidate => "candidate",
        MemoryStatus::Active => "active",
        MemoryStatus::Pinned => "pinned",
        MemoryStatus::Superseded => "superseded",
        MemoryStatus::Archived => "archived",
        MemoryStatus::Tombstoned => "tombstoned",
        MemoryStatus::Quarantined => "quarantined",
    }
}

fn assert_approx(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 0.0001, "expected {expected}, got {actual}");
}
