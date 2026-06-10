//! Regression: bulk reindex must not abort when a supersessor is indexed before
//! its `supersedes` target, and the supersession edge must survive a bulk pass.
//!
//! Root cause: `index::query::sync_supersession` inserted each
//! `memory_supersession` edge unguarded. The table's `supersedes_id` is a
//! `REFERENCES memories(id)` FK with `PRAGMA foreign_keys = ON`. During a bulk
//! reindex (`Substrate::open` → reconcile phase 6, or `Substrate::reindex`), the
//! tree is walked in unsorted `walkdir` order, so a supersessor can be upserted
//! before its target's `memories` row exists — tripping the FK and aborting the
//! whole reconcile with `OperatorRepairRequired`.
//!
//! Fix: the per-write insert is FK-guarded (parity with the v4 migration), and a
//! deferred [`Index::resync_supersession_edges`] pass re-derives every edge after
//! all `memories` rows of the bulk pass exist, so no edge is silently dropped.

use chrono::Utc;
use memory_substrate::index::{open_index, Index};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType,
    RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode,
    WritePolicy,
};
use rusqlite::Connection;

const TARGET_ID: &str = "mem_20260610_a1b2c3d4e5f60718_000001";
const SUPERSESSOR_ID: &str = "mem_20260610_a1b2c3d4e5f60718_000002";

/// Worst-case bulk order, forced deterministically at the `Index` layer: upsert
/// the supersessor (edge → target) *before* the target's `memories` row exists.
///
/// Before the fix this aborted with `FOREIGN KEY constraint failed`. With the
/// FK guard the supersessor upsert succeeds and the edge is *skipped* (target
/// absent); the deferred [`Index::resync_supersession_edges`] pass re-adds it
/// once the target lands. Both orderings are asserted: supersessor-first here,
/// and the reverse (target-first, edge inline) is the always-worked incremental
/// path that the projection test already covers — we re-assert it here too so
/// the guard cannot regress it.
#[test]
fn bulk_order_supersessor_before_target_does_not_abort_and_edge_survives_resync() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let mut index = Index::new(conn);

    // Supersessor first — target's `memories` row does not exist yet.
    index
        .upsert_memory(&sample_memory(SUPERSESSOR_ID, vec![MemoryId::new(TARGET_ID)]), false)
        .expect("supersessor-before-target upsert must not trip the FK");
    // Guard skipped the edge because the target is not indexed yet.
    assert!(
        supersedes_ids(index.connection(), SUPERSESSOR_ID).is_empty(),
        "edge to a not-yet-indexed target is deferred, not inserted"
    );

    // Target lands.
    index.upsert_memory(&sample_memory(TARGET_ID, Vec::new()), false).expect("target upsert");

    // Deferred pass re-derives the edge now that the target exists.
    let inserted = index.resync_supersession_edges().expect("resync supersession edges");
    assert_eq!(inserted, 1, "exactly the one deferred edge is backfilled");
    assert_eq!(
        supersedes_ids(index.connection(), SUPERSESSOR_ID),
        vec![TARGET_ID.to_string()],
        "edge present after deferred resync"
    );

    // Idempotent: a second pass inserts nothing and leaves the edge intact.
    let inserted_again = index.resync_supersession_edges().expect("resync again");
    assert_eq!(inserted_again, 0, "resync is idempotent");
    assert_eq!(supersedes_ids(index.connection(), SUPERSESSOR_ID), vec![TARGET_ID.to_string()]);
}

/// The reverse order — target indexed before the supersessor — inserts the edge
/// inline (the always-worked incremental path). The FK guard must not regress
/// it, and the deferred pass must be a no-op for an already-consistent table.
#[test]
fn target_before_supersessor_inserts_edge_inline_and_resync_is_noop() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let mut index = Index::new(conn);

    index.upsert_memory(&sample_memory(TARGET_ID, Vec::new()), false).expect("target first");
    index
        .upsert_memory(&sample_memory(SUPERSESSOR_ID, vec![MemoryId::new(TARGET_ID)]), false)
        .expect("supersessor after target");

    assert_eq!(
        supersedes_ids(index.connection(), SUPERSESSOR_ID),
        vec![TARGET_ID.to_string()],
        "edge inserted inline when target pre-exists"
    );

    let inserted = index.resync_supersession_edges().expect("resync");
    assert_eq!(inserted, 0, "nothing to backfill when every edge already exists");
    assert_eq!(supersedes_ids(index.connection(), SUPERSESSOR_ID), vec![TARGET_ID.to_string()]);
}

/// End-to-end through the real `Substrate::reindex` bulk path: a tree where a
/// supersessor references a target. Reconcile must succeed (no
/// `OperatorRepairRequired`) regardless of walkdir order, and the supersession
/// edge must exist afterward (the deferred pass guarantees it independent of
/// the order the bulk walk visited the files).
///
/// Many sibling files are seeded so neither walk order is contrived; the final
/// assertion holds for *either* order because the deferred pass re-derives the
/// edge once all `memories` rows are present.
#[tokio::test]
async fn full_reindex_with_cross_file_supersession_succeeds_and_keeps_edge() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    // Seed filler memories so the supersessor/target pair is buried in an
    // unsorted walk rather than trivially adjacent.
    for seq in 10..30 {
        let filler = sample_memory(&format!("mem_20260610_a1b2c3d4e5f60718_{seq:06}"), Vec::new());
        write_memory(&substrate, filler).await;
    }
    // Target and its supersessor: the supersessor carries a cross-file
    // `supersedes` edge to the target.
    write_memory(&substrate, sample_memory(TARGET_ID, Vec::new())).await;
    write_memory(&substrate, sample_memory(SUPERSESSOR_ID, vec![MemoryId::new(TARGET_ID)])).await;

    // The bulk rebuild. Before the fix this aborts with FOREIGN KEY when the
    // walk reaches the supersessor before the target.
    let reindexed = substrate.reindex().await.expect("full reindex must not abort on cross-file supersession");
    assert_eq!(reindexed, 22, "all seeded memories reindexed");

    // The edge must survive the bulk pass.
    let db = Connection::open(roots.runtime.join("index.sqlite")).expect("open index for assertion");
    assert_eq!(
        supersedes_ids(&db, SUPERSESSOR_ID),
        vec![TARGET_ID.to_string()],
        "supersession edge present after bulk reindex"
    );
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
    substrate
        .write_memory(memory_substrate::WriteRequest {
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
        .expect("write memory");
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

fn sample_memory(id: &str, supersedes: Vec<MemoryId>) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "supersession fk reindex fixture".to_string(),
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
