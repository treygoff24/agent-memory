use chrono::{DateTime, Utc};
use memory_substrate::{Entity, MemoryId, MemoryStatus, RecallIndexRow, RepoPath, Scope, Sensitivity, SourceKind};
use memoryd::recall::{
    collect_recall_candidates, rank_recall_candidates, resolve_entity_matches, select_ranked_candidates,
    EntityMatchKind, OmissionReason, RankingContext, RecallCandidate, RecallSectionName,
};

#[test]
fn ranking_formula_uses_spec_weights_from_recall_index_fields() {
    let candidate = candidate(
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Pinned)
            .with_scope(Scope::Project, Some("proj_agent_memory"))
            .with_updated_at("2026-04-29T12:00:00Z")
            .with_confidence(0.99)
            .with_source(SourceKind::User),
    )
    .with_entity_match(EntityMatchKind::ExactId);

    let ranked = rank_recall_candidates(vec![candidate], context());

    assert_eq!(ranked[0].score, 199);
    assert_eq!(ranked[0].id, "mem_20260430_0000000000000001_000001");
}

#[test]
fn ranking_is_computed_from_recall_index_rows_without_envelope_hydration() {
    let low = candidate(row("mem_20260430_0000000000000001_000001", MemoryStatus::Active).with_confidence(0.1));
    let high = candidate(row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_confidence(0.9));

    let ranked = rank_recall_candidates(vec![low, high], context());

    assert_eq!(
        ranked.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000002_000002", "mem_20260430_0000000000000001_000001",]
    );
}

#[test]
fn tie_breakers_are_score_status_recency_then_lexicographic_id() {
    let newer_active = candidate(
        row("mem_20260430_0000000000000004_000004", MemoryStatus::Active).with_updated_at("2026-04-30T11:00:00Z"),
    );
    let older_active_lex_b = candidate(
        row("mem_20260430_0000000000000003_000003", MemoryStatus::Active).with_updated_at("2026-04-30T10:00:00Z"),
    );
    let older_active_lex_a = candidate(
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_updated_at("2026-04-30T10:00:00Z"),
    );
    let pinned = candidate(row("mem_20260430_0000000000000001_000001", MemoryStatus::Pinned).with_confidence(0.0));

    let ranked = rank_recall_candidates(vec![older_active_lex_b, newer_active, pinned, older_active_lex_a], context());

    assert_eq!(
        ranked.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        vec![
            "mem_20260430_0000000000000001_000001",
            "mem_20260430_0000000000000004_000004",
            "mem_20260430_0000000000000002_000002",
            "mem_20260430_0000000000000003_000003",
        ]
    );
}

#[test]
fn pre_shuffled_candidates_produce_identical_ranking_output() {
    let rows = vec![
        row("mem_20260430_0000000000000003_000003", MemoryStatus::Active).with_confidence(0.3),
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Pinned).with_confidence(0.1),
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_confidence(0.9),
    ];
    let forward = collect_recall_candidates(RecallSectionName::RecentMemory, rows.clone()).facts;
    let shuffled = collect_recall_candidates(
        RecallSectionName::RecentMemory,
        vec![rows[2].clone(), rows[0].clone(), rows[1].clone()],
    )
    .facts;

    assert_eq!(
        rank_ids(rank_recall_candidates(forward, context())),
        rank_ids(rank_recall_candidates(shuffled, context()))
    );
}

#[test]
fn budget_exhaustion_produces_stable_omissions() {
    let candidates = collect_recall_candidates(
        RecallSectionName::RecentMemory,
        vec![
            row("mem_20260430_0000000000000003_000003", MemoryStatus::Active).with_summary("cccc"),
            row("mem_20260430_0000000000000001_000001", MemoryStatus::Pinned).with_summary("aaaa"),
            row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_summary("bbbb"),
        ],
    )
    .facts;

    let selected = select_ranked_candidates(RecallSectionName::RecentMemory, candidates, context(), 2);

    assert_eq!(
        selected.selected.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000001_000001", "mem_20260430_0000000000000002_000002",]
    );
    assert_eq!(selected.omitted.len(), 1);
    assert_eq!(selected.omitted[0].id.as_deref(), Some("mem_20260430_0000000000000003_000003"));
    assert_eq!(selected.omitted[0].reason, OmissionReason::BudgetExhausted);
}

#[test]
fn alias_collision_omission_is_one_per_section_alias_with_sorted_colliding_ids() {
    let rows = vec![
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Active).with_entity(
            "ent_alpha",
            "Alpha LLC",
            &["forge"],
        ),
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_entity(
            "ent_beta",
            "Beta LLC",
            &["FORGE"],
        ),
    ];
    let candidates = collect_recall_candidates(RecallSectionName::EntityRecall, rows).facts;

    let entity_section = resolve_entity_matches(RecallSectionName::EntityRecall, candidates.clone(), &["forge"]);
    let project_section = resolve_entity_matches(RecallSectionName::ProjectState, candidates, &["forge"]);

    assert_eq!(entity_section.candidates.len(), 0);
    assert_eq!(project_section.candidates.len(), 0);
    let omissions = [entity_section.omitted, project_section.omitted].concat();
    assert_eq!(omissions.len(), 2);
    assert!(omissions.iter().all(|omission| omission.id.is_none()));
    assert!(omissions.iter().all(|omission| omission.reason == OmissionReason::AmbiguousAlias));
    assert!(omissions.iter().all(|omission| omission.alias.as_deref() == Some("forge")));
    assert!(omissions.iter().all(|omission| omission.colliding_ids == vec!["ent_alpha", "ent_beta"]));
    assert_eq!(
        omissions.iter().map(|omission| omission.section).collect::<Vec<_>>(),
        vec![RecallSectionName::EntityRecall, RecallSectionName::ProjectState,]
    );
}

#[test]
fn alias_collision_only_suppresses_candidates_depending_on_ambiguous_alias() {
    let rows = vec![
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Active).with_entity(
            "ent_alpha",
            "Alpha LLC",
            &["forge"],
        ),
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_entity(
            "ent_beta",
            "Beta LLC",
            &["FORGE"],
        ),
        row("mem_20260430_0000000000000003_000003", MemoryStatus::Pinned).with_entity(
            "ent_safe",
            "Safe Project",
            &["safe project"],
        ),
        row("mem_20260430_0000000000000004_000004", MemoryStatus::Active)
            .with_entity("ent_gamma", "Gamma LLC", &["forge"])
            .with_tags(&["safe-tag"]),
    ];
    let candidates = collect_recall_candidates(RecallSectionName::EntityRecall, rows).facts;

    let resolved =
        resolve_entity_matches(RecallSectionName::EntityRecall, candidates, &["forge", "ent_safe", "safe tag"]);

    assert_eq!(
        resolved.candidates.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>(),
        vec!["mem_20260430_0000000000000003_000003", "mem_20260430_0000000000000004_000004"]
    );
    assert_eq!(
        resolved.candidates.iter().map(|candidate| candidate.entity_match).collect::<Vec<_>>(),
        vec![EntityMatchKind::ExactId, EntityMatchKind::Tag]
    );
    assert_eq!(resolved.omitted.len(), 1);
    assert_eq!(resolved.omitted[0].reason, OmissionReason::AmbiguousAlias);
    assert_eq!(resolved.omitted[0].alias.as_deref(), Some("forge"));
    assert_eq!(resolved.omitted[0].colliding_ids, vec!["ent_alpha", "ent_beta", "ent_gamma"]);
}

#[test]
fn entity_resolution_matches_exact_ids_and_separator_equivalent_aliases() {
    let rows = vec![
        row("mem_20260430_0000000000000001_000001", MemoryStatus::Active)
            .with_entity("ent_alpha", "Alpha LLC", &["agent-memory"])
            .with_tags(&["rust/tools"]),
        row("mem_20260430_0000000000000002_000002", MemoryStatus::Active).with_entity(
            "ent_beta",
            "Beta LLC",
            &["unrelated"],
        ),
    ];
    let candidates = collect_recall_candidates(RecallSectionName::EntityRecall, rows).facts;

    let exact_id = resolve_entity_matches(RecallSectionName::EntityRecall, candidates.clone(), &["ent_alpha"]);
    let alias = resolve_entity_matches(RecallSectionName::EntityRecall, candidates.clone(), &["agent_memory"]);
    let tag = resolve_entity_matches(RecallSectionName::EntityRecall, candidates, &["rust tools"]);

    assert_eq!(exact_id.candidates[0].entity_match, EntityMatchKind::ExactId);
    assert_eq!(alias.candidates[0].entity_match, EntityMatchKind::ExactLabelOrAlias);
    assert_eq!(tag.candidates[0].entity_match, EntityMatchKind::Tag);
}

fn candidate(row: RecallIndexRow) -> RecallCandidate {
    RecallCandidate::from(row)
}

fn context() -> RankingContext {
    RankingContext {
        now: instant("2026-04-30T12:00:00Z"),
        exact_project_namespace: Some("proj_agent_memory".to_owned()),
    }
}

fn rank_ids(ranked: Vec<memoryd::recall::RankedRecallCandidate>) -> Vec<String> {
    ranked.into_iter().map(|item| item.id).collect()
}

fn row(id: &str, status: MemoryStatus) -> RecallIndexRow {
    RecallIndexRow {
        id: MemoryId::new(id),
        path: RepoPath::new(format!("me/{id}.md")),
        summary: format!("summary for {id}"),
        status,
        scope: Scope::User,
        canonical_namespace_id: None,
        updated_at: instant("2026-04-20T12:00:00Z"),
        indexed_at: instant("2026-04-20T12:00:00Z"),
        confidence: 0.0,
        source_kind: SourceKind::AgentPrimary,
        source_device: None,
        source_harness: None,
        source_session_id: None,
        author_harness: None,
        author_session_id: None,
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

trait RowFixture {
    fn with_scope(self, scope: Scope, canonical_namespace_id: Option<&str>) -> Self;
    fn with_updated_at(self, value: &str) -> Self;
    fn with_confidence(self, confidence: f64) -> Self;
    fn with_source(self, source_kind: SourceKind) -> Self;
    fn with_summary(self, summary: &str) -> Self;
    fn with_entity(self, id: &str, label: &str, aliases: &[&str]) -> Self;
    fn with_tags(self, tags: &[&str]) -> Self;
}

impl RowFixture for RecallIndexRow {
    fn with_scope(mut self, scope: Scope, canonical_namespace_id: Option<&str>) -> Self {
        self.scope = scope;
        self.max_scope = scope;
        self.canonical_namespace_id = canonical_namespace_id.map(str::to_owned);
        self
    }

    fn with_updated_at(mut self, value: &str) -> Self {
        self.updated_at = instant(value);
        self
    }

    fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence;
        self
    }

    fn with_source(mut self, source_kind: SourceKind) -> Self {
        self.source_kind = source_kind;
        self
    }

    fn with_summary(mut self, summary: &str) -> Self {
        self.summary = summary.to_owned();
        self
    }

    fn with_entity(mut self, id: &str, label: &str, aliases: &[&str]) -> Self {
        self.entities.push(Entity {
            id: id.to_owned(),
            label: label.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        });
        self
    }

    fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
        self
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
