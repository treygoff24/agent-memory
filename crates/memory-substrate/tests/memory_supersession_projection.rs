use chrono::Utc;
use memory_substrate::index::{open_index, Index};
use memory_substrate::{
    Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope,
    Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
};
use rusqlite::Connection;

#[test]
fn sync_writes_and_replaces_memory_supersession_edges() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let mut index = Index::new(conn);

    let id_a = "mem_20260501_a1b2c3d4e5f60718_000101";
    let id_b = "mem_20260501_a1b2c3d4e5f60718_000102";
    let current = "mem_20260501_a1b2c3d4e5f60718_000103";
    index.upsert_memory(&sample_memory(id_a, Vec::new()), false).expect("insert a");
    index.upsert_memory(&sample_memory(id_b, Vec::new()), false).expect("insert b");
    index
        .upsert_memory(&sample_memory(current, vec![MemoryId::new(id_a)]), false)
        .expect("insert current superseding a");
    assert_eq!(supersedes_ids(index.connection(), current), vec![id_a.to_string()]);

    index
        .upsert_memory(&sample_memory(current, vec![MemoryId::new(id_b)]), false)
        .expect("replace current supersession edge");
    assert_eq!(supersedes_ids(index.connection(), current), vec![id_b.to_string()]);
}

#[test]
fn fresh_schema_has_memory_supersession_table_and_reverse_index() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = open_index(&temp.path().join("index.sqlite")).expect("open index");

    let columns = table_columns(&conn, "memory_supersession");
    assert_eq!(columns, vec!["memory_id", "supersedes_id"]);
    assert_eq!(index_columns(&conn, "idx_memory_supersession_supersedes_id"), vec!["supersedes_id"]);
}

#[test]
fn recursive_supersession_cte_is_bounded_across_cycles() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let mut index = Index::new(conn);

    let id_a = "mem_20260501_a1b2c3d4e5f60718_000111";
    let id_b = "mem_20260501_a1b2c3d4e5f60718_000112";
    let id_c = "mem_20260501_a1b2c3d4e5f60718_000113";
    index.upsert_memory(&sample_memory(id_a, Vec::new()), false).expect("insert a");
    index.upsert_memory(&sample_memory(id_b, Vec::new()), false).expect("insert b");
    index.upsert_memory(&sample_memory(id_c, Vec::new()), false).expect("insert c");
    index.upsert_memory(&sample_memory(id_a, vec![MemoryId::new(id_c)]), false).expect("update cyclic a");
    index.upsert_memory(&sample_memory(id_b, vec![MemoryId::new(id_a)]), false).expect("update cyclic b");
    index.upsert_memory(&sample_memory(id_c, vec![MemoryId::new(id_b)]), false).expect("update cyclic c");

    let chain = supersession_chain(index.connection(), id_a, 8);
    assert_eq!(chain, vec![id_c.to_string(), id_b.to_string()]);
}

fn supersedes_ids(conn: &Connection, memory_id: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT supersedes_id FROM memory_supersession WHERE memory_id = ?1 ORDER BY supersedes_id")
        .expect("prepare supersession query");
    stmt.query_map([memory_id], |row| row.get::<_, String>(0))
        .expect("query supersession rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect supersession rows")
}

fn supersession_chain(conn: &Connection, memory_id: &str, max_depth: u32) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            r#"
WITH RECURSIVE chain(memory_id, supersedes_id, depth, path) AS (
  SELECT memory_id, supersedes_id, 1, printf('|%s|%s|', memory_id, supersedes_id)
  FROM memory_supersession
  WHERE memory_id = ?1
  UNION ALL
  SELECT next.memory_id, next.supersedes_id, chain.depth + 1, chain.path || next.supersedes_id || '|'
  FROM memory_supersession AS next
  JOIN chain ON next.memory_id = chain.supersedes_id
  WHERE chain.depth < ?2
    AND instr(chain.path, printf('|%s|', next.supersedes_id)) = 0
)
SELECT supersedes_id FROM chain ORDER BY depth
"#,
        )
        .expect("prepare recursive supersession query");
    stmt.query_map((memory_id, max_depth), |row| row.get::<_, String>(0))
        .expect("query supersession chain")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect supersession chain")
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).expect("prepare table_info");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query table_info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns")
}

fn index_columns(conn: &Connection, index: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("PRAGMA index_info({index})")).expect("prepare index_info");
    stmt.query_map([], |row| row.get::<_, String>(2))
        .expect("query index_info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns")
}

fn sample_memory(id: &str, supersedes: Vec<MemoryId>) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "supersession fixture".to_string(),
            confidence: 0.8,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
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
            supersedes,
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: format!("body {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
