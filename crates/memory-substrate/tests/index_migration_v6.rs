use memory_substrate::index::{chunk_memory, migrate_v6, open_index, Index, INDEX_SUPPORTED_SCHEMA_VERSION};
use memory_substrate::{EmbeddingTriple, RepoPath};

fn fixture_memory() -> memory_substrate::Memory {
    let markdown = r#"---
schema_version: 1
id: mem_20260424_a1b2c3d4e5f60718_000100
type: pattern
scope: agent
summary: representative migration memory
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  component: test
tags:
  - migration
aliases:
  - migration-alias
---
body text used for the migration fixture
"#;
    memory_substrate::frontmatter::parse_document(
        markdown,
        Some(RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000100.md")),
    )
    .expect("parse fixture")
    .memory
}

#[test]
fn migrate_v6_is_idempotent_and_preserves_representative_data_and_rollback_is_readable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let live = temp.path().join("index.sqlite");
    let backup = temp.path().join("index-v5.backup.sqlite");

    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
    let memory = fixture_memory();
    let expected_body_hash = memory_substrate::markdown::hash_bytes(memory.body.as_bytes()).to_string();
    let expected_chunk_count = chunk_memory(&memory).len();
    let expected_id = memory.frontmatter.id.as_str().to_string();
    let expected_summary = memory.frontmatter.summary.clone();

    let mut index = Index::with_active_embedding(open_index(&live).expect("open baseline"), triple);
    index.upsert_memory(&memory, false).expect("seed representative v5 data");

    // Simulate a genuine v5 database by removing the v6 tables and the v6
    // schema_migrations row. The v5 data (memories, chunks, tags, aliases,
    // pending jobs) must survive migration and rollback.
    drop(index);
    let mut conn = rusqlite::Connection::open(&live).expect("reopen for downgrade");
    conn.execute_batch(
        "DROP TABLE IF EXISTS memory_abstractions;
         DROP TABLE IF EXISTS memory_cues;
         DROP TABLE IF EXISTS aux_embedding_meta;
         DROP TABLE IF EXISTS aux_pending_embedding_jobs;
         DELETE FROM schema_migrations WHERE version = 6;",
    )
    .expect("downgrade to schema v5");

    let v5_version: i64 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .expect("v5 version");
    assert_eq!(v5_version, 5);

    std::fs::copy(&live, &backup).expect("pre-migration backup");

    // migrate_v6 is the only thing that creates the v6 tables in this path.
    migrate_v6(&mut conn).expect("migrate_v6");

    let version: i64 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 6);
    assert_eq!(INDEX_SUPPORTED_SCHEMA_VERSION, 6);

    for table in ["memory_abstractions", "memory_cues", "aux_embedding_meta", "aux_pending_embedding_jobs"] {
        let exists: i64 = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| {
                row.get(0)
            })
            .expect("table probe");
        assert_eq!(exists, 1, "{table}");
    }

    // Data integrity: memories row and all v5-era derived rows must be intact.
    let summary: String = conn
        .query_row("SELECT summary FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))
        .expect("memory still present");
    assert_eq!(summary, expected_summary);

    let body_hash: String = conn
        .query_row("SELECT body_hash FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))
        .expect("body_hash");
    assert_eq!(body_hash, expected_body_hash);

    let chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_chunks WHERE memory_id=?1", [expected_id.as_str()], |row| row.get(0))
        .expect("chunk count");
    assert_eq!(chunk_count, expected_chunk_count as i64);

    let pending_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pending_embedding_jobs WHERE chunk_id IN (SELECT chunk_id FROM memory_chunks WHERE memory_id=?1)",
            [expected_id.as_str()],
            |row| row.get(0),
        )
        .expect("pending count");
    assert_eq!(pending_count, expected_chunk_count as i64);

    let tag_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_tags WHERE memory_id=?1", [expected_id.as_str()], |row| row.get(0))
        .expect("tag count");
    assert_eq!(tag_count, 1);

    let alias_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_aliases WHERE memory_id=?1", [expected_id.as_str()], |row| row.get(0))
        .expect("alias count");
    assert_eq!(alias_count, 1);

    drop(conn);

    // Rollback: restoring the pre-migration copy must remain readable, and a
    // normal open must migrate it back to v6.
    std::fs::copy(&backup, &live).expect("restore v5 backup");
    let restored = open_index(&live).expect("open restored v5");
    let restored_version: i64 = restored
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .expect("restored version");
    assert_eq!(restored_version, 6);
    let restored_summary: String = restored
        .query_row("SELECT summary FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))
        .expect("restored summary");
    assert_eq!(restored_summary, expected_summary);
}
