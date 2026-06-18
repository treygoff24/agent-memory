use chrono::{DateTime, Utc};
use memory_substrate::index::{open_index, Index};
use memory_substrate::*;

#[tokio::test]
async fn memory_query_filters_and_recall_index_use_stream_a_index_projections() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");

    let mut active = sample_memory("mem_20260430_a1b2c3d4e5f60718_000001", "2026-04-30T10:00:00Z");
    active.frontmatter.scope = Scope::Project;
    active.frontmatter.namespace = Some("project".to_string());
    active.frontmatter.canonical_namespace_id = Some("proj_alpha".to_string());
    active.frontmatter.tags = vec!["recall".to_string()];
    active.frontmatter.aliases = vec!["stream e".to_string()];
    active.frontmatter.entities = vec![Entity {
        id: "ent_stream_e".to_string(),
        label: "Stream E".to_string(),
        aliases: vec!["passive recall".to_string()],
    }];
    active.frontmatter.source.kind = SourceKind::AgentPrimary;
    active.frontmatter.confidence = 0.91;
    active.frontmatter.requires_user_confirmation = true;
    active.frontmatter.review_state = Some("pending".to_string());
    active.frontmatter.retrieval_policy.max_scope = Scope::Project;
    active.frontmatter.write_policy.human_review_required = true;

    let mut pinned = sample_memory("mem_20260430_a1b2c3d4e5f60718_000002", "2026-04-30T11:00:00Z");
    pinned.frontmatter.status = MemoryStatus::Pinned;
    pinned.frontmatter.scope = Scope::Project;
    pinned.frontmatter.namespace = Some("project".to_string());
    pinned.frontmatter.canonical_namespace_id = Some("proj_alpha".to_string());
    pinned.frontmatter.summary = "pinned recall fixture".to_string();
    pinned.frontmatter.tags = vec!["pinned-tag".to_string()];

    let mut disabled = sample_memory("mem_20260430_a1b2c3d4e5f60718_000003", "2026-04-30T12:00:00Z");
    disabled.frontmatter.scope = Scope::Project;
    disabled.frontmatter.namespace = Some("project".to_string());
    disabled.frontmatter.canonical_namespace_id = Some("proj_alpha".to_string());
    disabled.frontmatter.retrieval_policy.passive_recall = false;
    disabled.frontmatter.retrieval_policy.index_body = false;

    let mut me = sample_memory("mem_20260430_a1b2c3d4e5f60718_000004", "2026-04-30T13:00:00Z");
    me.frontmatter.scope = Scope::User;

    let agent = sample_memory("mem_20260430_a1b2c3d4e5f60718_000005", "2026-04-30T14:00:00Z");

    let mut org = sample_memory("mem_20260430_a1b2c3d4e5f60718_000006", "2026-04-30T15:00:00Z");
    org.frontmatter.scope = Scope::Org;
    org.frontmatter.namespace = Some("org".to_string());
    org.frontmatter.canonical_namespace_id = Some("org_alpha".to_string());

    for memory in [&active, &pinned, &disabled, &me, &agent, &org] {
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: memory.clone(),
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("write fixture");
    }

    let defaults = substrate.query_memory(MemoryQuery::default()).await.expect("default query");
    assert_eq!(
        ids(&defaults),
        vec![
            active.frontmatter.id.clone(),
            pinned.frontmatter.id.clone(),
            disabled.frontmatter.id.clone(),
            me.frontmatter.id.clone(),
            agent.frontmatter.id.clone(),
            org.frontmatter.id.clone()
        ]
    );

    let pinned_hits = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: None,
            include_metadata_only: false,
            status: Some(MemoryStatus::Pinned),
            namespace_prefix: None,
            passive_recall_only: false,
            updated_since: None,
        })
        .await
        .expect("pinned query");
    assert_eq!(ids(&pinned_hits), vec![pinned.frontmatter.id.clone()]);

    let recall_hits = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: None,
            include_metadata_only: false,
            status: None,
            namespace_prefix: Some("project:proj_alpha".to_string()),
            passive_recall_only: true,
            updated_since: Some(parse_time("2026-04-30T11:00:00Z")),
        })
        .await
        .expect("project recall query");
    assert_eq!(ids(&recall_hits), vec![pinned.frontmatter.id.clone()]);

    let me_hits = substrate
        .query_memory(MemoryQuery { namespace_prefix: Some("me".to_string()), ..MemoryQuery::default() })
        .await
        .expect("me namespace query");
    assert_eq!(ids(&me_hits), vec![me.frontmatter.id.clone()]);

    let agent_hits = substrate
        .query_memory(MemoryQuery { namespace_prefix: Some("agent".to_string()), ..MemoryQuery::default() })
        .await
        .expect("agent namespace query");
    assert_eq!(ids(&agent_hits), vec![agent.frontmatter.id.clone()]);

    let org_hits = substrate
        .query_memory(MemoryQuery { namespace_prefix: Some("org:org_alpha".to_string()), ..MemoryQuery::default() })
        .await
        .expect("org namespace query");
    assert_eq!(ids(&org_hits), vec![org.frontmatter.id.clone()]);

    let invalid = substrate
        .query_memory(MemoryQuery { namespace_prefix: Some("team:wrong".to_string()), ..MemoryQuery::default() })
        .await
        .expect_err("invalid namespace prefix");
    assert!(matches!(
        invalid,
        SubstrateError::InvalidQuery { ref field, ref value, ref message }
            if field == "namespace_prefix" && value == "team:wrong" && message.contains("invalid_query")
    ));

    let recall_index_rows = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: Some("project:proj_alpha".to_string()),
            statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned],
            passive_recall_only: true,
            updated_since: None,
            match_terms: vec![
                "ent_stream_e".to_string(),
                "passive recall".to_string(),
                "stream e".to_string(),
                "recall".to_string(),
            ],
            hydrate: AuxScope::All,
            source_identity: true,
        })
        .await
        .expect("recall index query");

    assert_eq!(recall_index_rows.len(), 1);
    let row = &recall_index_rows[0];
    assert_eq!(row.id, active.frontmatter.id);
    assert_eq!(row.path, active.path.expect("active path"));
    assert_eq!(row.summary, "sample".to_string());
    assert_eq!(row.status, MemoryStatus::Active);
    assert_eq!(row.scope, Scope::Project);
    assert_eq!(row.canonical_namespace_id.as_deref(), Some("proj_alpha"));
    assert_eq!(row.updated_at, parse_time("2026-04-30T10:00:00Z"));
    assert_eq!(row.confidence, 0.91);
    assert_eq!(row.source_kind, SourceKind::AgentPrimary);
    assert_eq!(row.sensitivity, Sensitivity::Internal);
    assert!(row.requires_user_confirmation);
    assert_eq!(row.review_state.as_deref(), Some("pending"));
    assert!(row.human_review_required);
    assert_eq!(row.max_scope, Scope::Project);
    assert!(row.passive_recall);
    assert!(row.index_body);
    assert_eq!(row.tags, vec!["recall".to_string()]);
    assert_eq!(row.aliases, vec!["stream e".to_string()]);
    assert_eq!(
        row.entities,
        vec![Entity {
            id: "ent_stream_e".to_string(),
            label: "Stream E".to_string(),
            aliases: vec!["passive recall".to_string()]
        }]
    );
}

#[tokio::test]
async fn recall_index_match_terms_are_isolated_by_source() {
    let context = seeded_isolated_recall_match_substrate().await;

    assert_isolated_recall_match(&context.substrate, RecallMatchSource::Tag).await;
    assert_isolated_recall_match(&context.substrate, RecallMatchSource::MemoryAlias).await;
    assert_isolated_recall_match(&context.substrate, RecallMatchSource::EntityId).await;
    assert_isolated_recall_match(&context.substrate, RecallMatchSource::EntityLabel).await;
    assert_isolated_recall_match(&context.substrate, RecallMatchSource::EntityAlias).await;
}

#[tokio::test]
async fn recall_index_match_terms_use_union_semantics() {
    let context = seeded_isolated_recall_match_substrate().await;

    let rows = context
        .substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: Some(format!("project:{ISOLATED_PROJECT_NAMESPACE}")),
            statuses: vec![MemoryStatus::Active],
            passive_recall_only: true,
            updated_since: None,
            match_terms: vec!["source-tag-only".to_string(), "source-memory-alias-only".to_string()],
            hydrate: AuxScope::All,
            source_identity: true,
        })
        .await
        .expect("multi-term recall-index match query");

    assert_eq!(
        recall_ids(&rows),
        vec![
            RecallMatchSource::Tag.fixture().memory.frontmatter.id,
            RecallMatchSource::MemoryAlias.fixture().memory.frontmatter.id,
        ]
    );
}

#[tokio::test]
async fn recall_index_statuses_and_updated_since_filters_are_independent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");

    let active_old =
        project_memory("mem_20260430_a1b2c3d4e5f60718_000013", "2026-04-30T10:00:00Z", ISOLATED_PROJECT_NAMESPACE);
    let mut pinned_at_threshold =
        project_memory("mem_20260430_a1b2c3d4e5f60718_000014", "2026-04-30T11:00:00Z", ISOLATED_PROJECT_NAMESPACE);
    pinned_at_threshold.frontmatter.status = MemoryStatus::Pinned;
    let active_new =
        project_memory("mem_20260430_a1b2c3d4e5f60718_000015", "2026-04-30T12:00:00Z", ISOLATED_PROJECT_NAMESPACE);

    for memory in [&active_old, &pinned_at_threshold, &active_new] {
        write_fixture(&substrate, memory.clone()).await;
    }

    let pinned_only = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: Some(format!("project:{ISOLATED_PROJECT_NAMESPACE}")),
            statuses: vec![MemoryStatus::Pinned],
            passive_recall_only: false,
            updated_since: None,
            match_terms: Vec::new(),
            hydrate: AuxScope::All,
            source_identity: true,
        })
        .await
        .expect("status-only recall index query");
    assert_eq!(recall_ids(&pinned_only), vec![pinned_at_threshold.frontmatter.id.clone()]);

    let updated_since_threshold = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: Some(format!("project:{ISOLATED_PROJECT_NAMESPACE}")),
            statuses: Vec::new(),
            passive_recall_only: false,
            updated_since: Some(parse_time("2026-04-30T11:00:00Z")),
            match_terms: Vec::new(),
            hydrate: AuxScope::All,
            source_identity: true,
        })
        .await
        .expect("updated-since-only recall index query");
    assert_eq!(
        recall_ids(&updated_since_threshold),
        vec![pinned_at_threshold.frontmatter.id.clone(), active_new.frontmatter.id.clone()]
    );

    let active_or_pinned_updated_since_threshold = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: Some(format!("project:{ISOLATED_PROJECT_NAMESPACE}")),
            statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned],
            passive_recall_only: false,
            updated_since: Some(parse_time("2026-04-30T11:00:00Z")),
            match_terms: Vec::new(),
            hydrate: AuxScope::All,
            source_identity: true,
        })
        .await
        .expect("combined status and updated-since recall index query");
    assert_eq!(
        recall_ids(&active_or_pinned_updated_since_threshold),
        vec![pinned_at_threshold.frontmatter.id.clone(), active_new.frontmatter.id.clone()]
    );
}

#[test]
fn recall_index_excludes_metadata_only_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("fresh index init");
    let mut index = Index::new(connection);

    let visible = sample_memory("mem_20260430_a1b2c3d4e5f60718_000016", "2026-04-30T13:00:00Z");
    let metadata_only = sample_memory("mem_20260430_a1b2c3d4e5f60718_000017", "2026-04-30T14:00:00Z");

    index.upsert_memory(&visible, false).expect("upsert visible row");
    index.upsert_memory(&metadata_only, true).expect("upsert metadata-only row");

    let rows = index.query_recall_index(&RecallIndexQuery::default()).expect("query recall index");

    assert_eq!(recall_ids(&rows), vec![visible.frontmatter.id]);
}

#[test]
fn fresh_index_init_creates_recall_filter_indexes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("fresh index init");

    assert_recall_governance_columns_exist(&connection);
    assert_recall_filter_indexes_exist(&connection);
}

#[test]
fn v1_index_migration_backfills_recall_projection_columns_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("index.sqlite");
    let connection = rusqlite::Connection::open(&db_path).expect("open v1 fixture");
    create_v1_index_fixture(&connection);
    let mut memory = sample_memory("mem_20260430_a1b2c3d4e5f60718_000007", "2026-04-30T12:00:00Z");
    memory.frontmatter.retrieval_policy.passive_recall = false;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.max_scope = Scope::Org;
    memory.frontmatter.write_policy.human_review_required = true;
    insert_v1_memory_row(&connection, &memory);
    drop(connection);

    let first_open = open_index(&db_path).expect("first migration");
    assert_eq!(
        first_open
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get::<_, i64>(0))
            .expect("MAX(version) from schema_migrations"),
        5
    );
    assert_recall_governance_columns_exist(&first_open);
    assert_recall_filter_indexes_exist(&first_open);

    let index = Index::new(first_open);
    let passive_rows = index
        .query_recall_index(&RecallIndexQuery { passive_recall_only: true, ..RecallIndexQuery::default() })
        .expect("query passive only");
    assert!(passive_rows.is_empty());
    let rows = index.query_recall_index(&RecallIndexQuery::default()).expect("query all");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].passive_recall);
    assert!(!rows[0].index_body);
    assert!(rows[0].human_review_required);
    assert_eq!(rows[0].max_scope, Scope::Org);
    drop(index);

    let second_open = open_index(&db_path).expect("second open is idempotent");
    assert_eq!(
        second_open
            .query_row("SELECT COUNT(*) FROM schema_migrations WHERE version = 3", [], |row| row.get::<_, i64>(0))
            .expect("version 3 migration count"),
        1
    );
    assert_eq!(
        second_open
            .query_row(
                "SELECT passive_recall,index_body,human_review_required,max_scope FROM memories WHERE id = ?1",
                [memory.frontmatter.id.as_str()],
                |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?))
                }
            )
            .expect("backfilled recall governance columns"),
        (0, 0, 1, "org".to_string())
    );
}

fn ids(rows: &[QueryResult]) -> Vec<MemoryId> {
    rows.iter().map(|row| row.id.clone()).collect()
}

fn recall_ids(rows: &[RecallIndexRow]) -> Vec<MemoryId> {
    rows.iter().map(|row| row.id.clone()).collect()
}

const ISOLATED_PROJECT_NAMESPACE: &str = "proj_isolated_recall";

struct TestSubstrate {
    _temp: tempfile::TempDir,
    substrate: Substrate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecallMatchSource {
    Tag,
    MemoryAlias,
    EntityId,
    EntityLabel,
    EntityAlias,
}

impl RecallMatchSource {
    const ALL: [Self; 5] = [Self::Tag, Self::MemoryAlias, Self::EntityId, Self::EntityLabel, Self::EntityAlias];

    fn fixture(self) -> IsolatedRecallFixture {
        match self {
            Self::Tag => {
                let mut memory = project_memory(
                    "mem_20260430_a1b2c3d4e5f60718_000008",
                    "2026-04-30T16:00:00Z",
                    ISOLATED_PROJECT_NAMESPACE,
                );
                memory.frontmatter.tags = vec!["source-tag-only".to_string()];
                IsolatedRecallFixture { source: self, match_term: "source-tag-only", memory }
            }
            Self::MemoryAlias => {
                let mut memory = project_memory(
                    "mem_20260430_a1b2c3d4e5f60718_000009",
                    "2026-04-30T17:00:00Z",
                    ISOLATED_PROJECT_NAMESPACE,
                );
                memory.frontmatter.aliases = vec!["source-memory-alias-only".to_string()];
                IsolatedRecallFixture { source: self, match_term: "source-memory-alias-only", memory }
            }
            Self::EntityId => {
                let mut memory = project_memory(
                    "mem_20260430_a1b2c3d4e5f60718_000010",
                    "2026-04-30T18:00:00Z",
                    ISOLATED_PROJECT_NAMESPACE,
                );
                memory.frontmatter.entities = vec![Entity {
                    id: "source_entity_id_only".to_string(),
                    label: "Entity Id Fixture".to_string(),
                    aliases: Vec::new(),
                }];
                IsolatedRecallFixture { source: self, match_term: "source_entity_id_only", memory }
            }
            Self::EntityLabel => {
                let mut memory = project_memory(
                    "mem_20260430_a1b2c3d4e5f60718_000011",
                    "2026-04-30T19:00:00Z",
                    ISOLATED_PROJECT_NAMESPACE,
                );
                memory.frontmatter.entities = vec![Entity {
                    id: "entity_label_fixture".to_string(),
                    label: "Source Entity Label Only".to_string(),
                    aliases: Vec::new(),
                }];
                IsolatedRecallFixture { source: self, match_term: "Source Entity Label Only", memory }
            }
            Self::EntityAlias => {
                let mut memory = project_memory(
                    "mem_20260430_a1b2c3d4e5f60718_000012",
                    "2026-04-30T20:00:00Z",
                    ISOLATED_PROJECT_NAMESPACE,
                );
                memory.frontmatter.entities = vec![Entity {
                    id: "entity_alias_fixture".to_string(),
                    label: "Entity Alias Fixture".to_string(),
                    aliases: vec!["source-entity-alias-only".to_string()],
                }];
                IsolatedRecallFixture { source: self, match_term: "source-entity-alias-only", memory }
            }
        }
    }
}

struct IsolatedRecallFixture {
    source: RecallMatchSource,
    match_term: &'static str,
    memory: Memory,
}

async fn seeded_isolated_recall_match_substrate() -> TestSubstrate {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");

    for source in RecallMatchSource::ALL {
        let fixture = source.fixture();
        assert_eq!(matching_sources(&fixture.memory, fixture.match_term), vec![fixture.source]);
        write_fixture(&substrate, fixture.memory).await;
    }

    TestSubstrate { _temp: temp, substrate }
}

async fn assert_isolated_recall_match(substrate: &Substrate, source: RecallMatchSource) {
    let fixture = source.fixture();
    let rows = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: Some(format!("project:{ISOLATED_PROJECT_NAMESPACE}")),
            statuses: vec![MemoryStatus::Active],
            passive_recall_only: true,
            updated_since: None,
            match_terms: vec![fixture.match_term.to_string()],
            hydrate: AuxScope::All,
            source_identity: true,
        })
        .await
        .expect("isolated recall-index match query");

    assert_eq!(
        recall_ids(&rows),
        vec![fixture.memory.frontmatter.id.clone()],
        "{source:?} should be the only match source for `{}`",
        fixture.match_term
    );
}

async fn write_fixture(substrate: &Substrate, memory: Memory) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write fixture");
}

fn project_memory(id: &str, updated_at: &str, canonical_namespace_id: &str) -> Memory {
    let mut memory = sample_memory(id, updated_at);
    memory.frontmatter.scope = Scope::Project;
    memory.frontmatter.namespace = Some("project".to_string());
    memory.frontmatter.canonical_namespace_id = Some(canonical_namespace_id.to_string());
    memory
}

fn matching_sources(memory: &Memory, term: &str) -> Vec<RecallMatchSource> {
    let mut sources = Vec::new();
    if memory.frontmatter.tags.iter().any(|value| matches_term(value, term)) {
        sources.push(RecallMatchSource::Tag);
    }
    if memory.frontmatter.aliases.iter().any(|value| matches_term(value, term)) {
        sources.push(RecallMatchSource::MemoryAlias);
    }
    if memory.frontmatter.entities.iter().any(|entity| matches_term(&entity.id, term)) {
        sources.push(RecallMatchSource::EntityId);
    }
    if memory.frontmatter.entities.iter().any(|entity| matches_term(&entity.label, term)) {
        sources.push(RecallMatchSource::EntityLabel);
    }
    if memory
        .frontmatter
        .entities
        .iter()
        .flat_map(|entity| entity.aliases.iter())
        .any(|value| matches_term(value, term))
    {
        sources.push(RecallMatchSource::EntityAlias);
    }
    sources
}

fn matches_term(value: &str, term: &str) -> bool {
    value.eq_ignore_ascii_case(term)
}

fn sample_memory(id: &str, updated_at: &str) -> Memory {
    let created_at = parse_time("2026-04-30T09:00:00Z");
    let updated_at = parse_time(updated_at);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "sample".to_string(),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at,
            updated_at,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: std::collections::BTreeMap::new(),
        },
        body: format!("body for {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn parse_time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("rfc3339 fixture").with_timezone(&Utc)
}

fn create_v1_index_fixture(connection: &rusqlite::Connection) {
    connection
        .execute_batch(
            r#"
CREATE TABLE schema_migrations(
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO schema_migrations(version) VALUES (1);

CREATE TABLE memories(
  id                          TEXT PRIMARY KEY,
  path                        TEXT NOT NULL UNIQUE,
  schema_version              INTEGER NOT NULL,
  type                        TEXT NOT NULL,
  scope                       TEXT NOT NULL,
  namespace                   TEXT,
  canonical_namespace_id      TEXT,
  summary                     TEXT NOT NULL,
  confidence                  REAL NOT NULL,
  trust_level                 TEXT NOT NULL,
  sensitivity                 TEXT NOT NULL,
  status                      TEXT NOT NULL,
  review_state                TEXT,
  requires_user_confirmation  INTEGER NOT NULL,
  created_at                  TEXT NOT NULL,
  updated_at                  TEXT NOT NULL,
  observed_at                 TEXT,
  valid_from                  TEXT,
  valid_until                 TEXT,
  ttl                         TEXT,
  author                      TEXT NOT NULL,
  source_kind                 TEXT NOT NULL,
  source_harness              TEXT,
  source_device               TEXT,
  body_hash                   TEXT NOT NULL,
  frontmatter_json            TEXT NOT NULL CHECK (json_valid(frontmatter_json)),
  file_hash                   TEXT NOT NULL,
  file_mtime_ns               INTEGER NOT NULL,
  indexed_at                  TEXT NOT NULL,
  metadata_only               INTEGER NOT NULL DEFAULT 0
);
"#,
        )
        .expect("create v1 fixture");
}

fn insert_v1_memory_row(connection: &rusqlite::Connection, memory: &Memory) {
    let frontmatter_json = serde_json::to_string(&memory.frontmatter).expect("frontmatter json");
    let path = memory.path.as_ref().expect("path").as_str().to_string();
    connection
        .execute(
            "INSERT INTO memories(
               id,path,schema_version,type,scope,namespace,canonical_namespace_id,summary,confidence,
               trust_level,sensitivity,status,review_state,requires_user_confirmation,created_at,updated_at,
               observed_at,valid_from,valid_until,ttl,author,source_kind,source_harness,source_device,
               body_hash,frontmatter_json,file_hash,file_mtime_ns,indexed_at,metadata_only
             ) VALUES (
               ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,
               NULL,NULL,NULL,NULL,?17,?18,NULL,NULL,?19,?20,?21,0,?22,0
             )",
            rusqlite::params![
                memory.frontmatter.id.as_str(),
                path,
                memory.frontmatter.schema_version as i64,
                "pattern",
                "agent",
                Option::<String>::None,
                Option::<String>::None,
                memory.frontmatter.summary,
                memory.frontmatter.confidence,
                "trusted",
                "internal",
                "active",
                Option::<String>::None,
                0_i64,
                memory.frontmatter.created_at.to_rfc3339(),
                memory.frontmatter.updated_at.to_rfc3339(),
                "system",
                "import",
                "sha256:body",
                frontmatter_json,
                "sha256:file",
                Utc::now().to_rfc3339(),
            ],
        )
        .expect("insert v1 memory row");
}

fn memory_column_exists(connection: &rusqlite::Connection, column: &str) -> bool {
    let mut stmt = connection.prepare("PRAGMA table_info(memories)").expect("table info");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("column names");
    rows.iter().any(|name| name == column)
}

fn assert_recall_filter_indexes_exist(connection: &rusqlite::Connection) {
    assert!(index_exists(connection, "idx_memories_status_passive_updated"));
    assert!(index_exists(connection, "idx_memories_scope_canon_status_passive_updated"));
}

fn assert_recall_governance_columns_exist(connection: &rusqlite::Connection) {
    assert!(memory_column_exists(connection, "passive_recall"));
    assert!(memory_column_exists(connection, "index_body"));
    assert!(memory_column_exists(connection, "human_review_required"));
    assert!(memory_column_exists(connection, "max_scope"));
}

fn index_exists(connection: &rusqlite::Connection, name: &str) -> bool {
    connection
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1", [name], |row| {
            row.get::<_, i64>(0)
        })
        .expect("index lookup")
        == 1
}
