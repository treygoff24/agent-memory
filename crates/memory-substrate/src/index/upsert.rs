//! Memory-row upsert: the `memories` table write, auxiliary-table sync (tags,
//! aliases, entities, evidence, supersession), chunk rebuild + embedding-job
//! enqueue, plus the file-consistency probe and supersession resync SQL.

use rusqlite::{named_params, params, Connection, Transaction};

use crate::index::chunking::chunk_memory;
use crate::markdown::hash_bytes;
use crate::model::{AuxRowKind, EmbeddingTriple, Memory, MemoryId, MemoryStatus, Sensitivity, Sha256};

use super::embedding::is_dropped_triple_rusqlite;

/// Upsert a memory into SQLite, populating all `memories` columns (spec §10.1).
///
/// `file_hash` is the exact on-disk hash when the caller already has it; the
/// body hash fallback preserves fixture call sites that do not touch disk.
/// `file_mtime_ns` is still 0 until the write path plumbs real metadata.
pub(super) struct MemoryUpsertOptions<'a> {
    pub(super) metadata_only: bool,
    pub(super) file_hash: Option<&'a Sha256>,
    pub(super) active_embedding: &'a EmbeddingTriple,
}

pub(super) fn upsert_memory_row_with_full_metadata(
    connection: &mut Connection,
    memory: &Memory,
    options: MemoryUpsertOptions<'_>,
) -> rusqlite::Result<()> {
    let active_embedding_dropped = is_dropped_triple_rusqlite(connection, options.active_embedding)?;
    let txn = connection.transaction()?;
    upsert_memory_row_in_txn(&txn, memory, options, active_embedding_dropped)?;
    txn.commit()
}

/// Upsert a single memory row (plus auxiliary tables, chunks, and embedding
/// jobs) inside a caller-supplied transaction.
///
/// Factored out of [`upsert_memory_row_with_full_metadata`] so a bulk reindex
/// can amortize one transaction across many rows and compute the
/// loop-invariant `active_embedding_dropped` flag once, instead of opening a
/// transaction and re-running the `dropped_embedding_triples` EXISTS probe per
/// memory. The single-row wrapper preserves the prior one-transaction-per-call
/// behavior. `active_embedding_dropped` MUST be
/// `is_dropped_triple_rusqlite(_, options.active_embedding)` for the same
/// triple, so batching is byte-for-byte equivalent to the per-row path.
pub(super) fn upsert_memory_row_in_txn(
    txn: &rusqlite::Transaction<'_>,
    memory: &Memory,
    options: MemoryUpsertOptions<'_>,
    active_embedding_dropped: bool,
) -> rusqlite::Result<()> {
    let previous_sensitivity = txn
        .query_row("SELECT sensitivity FROM memories WHERE id=?1", [memory.frontmatter.id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .and_then(|value| Sensitivity::from_db_str(&value));
    let sensitivity_upgraded = previous_sensitivity
        .is_some_and(|previous| previous.api_lane_eligible() && !memory.frontmatter.sensitivity.api_lane_eligible());
    let path = resolve_memory_path(memory);
    let sensitivity = memory.frontmatter.sensitivity.as_db_str();
    let memory_type = memory.frontmatter.memory_type.as_db_str();
    let scope = memory.frontmatter.scope.as_db_str();
    let trust_level = memory.frontmatter.trust_level.as_db_str();
    let status = memory.frontmatter.status.as_db_str();
    let author = memory.frontmatter.author.kind.as_db_str();
    let source_kind = memory.frontmatter.source.kind.as_db_str();
    let body_hash = hash_bytes(memory.body.as_bytes()).to_string();
    let frontmatter_json = serde_json::to_string(&memory.frontmatter).unwrap_or_else(|_| "{}".to_string());
    let file_hash = options.file_hash.map_or_else(|| body_hash.clone(), ToString::to_string);
    let file_mtime_ns: i64 = 0; // placeholder; deferred: plumb from fs::metadata
    let indexed_at = chrono::Utc::now().to_rfc3339();
    let created_at = memory.frontmatter.created_at.to_rfc3339();
    let updated_at = memory.frontmatter.updated_at.to_rfc3339();
    let observed_at = observed_at_for_index(memory).unwrap_or_else(|| created_at.clone());

    let passive_recall = memory.frontmatter.retrieval_policy.passive_recall as i64;
    let index_body = memory.frontmatter.retrieval_policy.index_body as i64;
    let human_review_required = memory.frontmatter.write_policy.human_review_required as i64;
    let max_scope = memory.frontmatter.retrieval_policy.max_scope.as_db_str();

    txn.execute(
        "INSERT INTO memories(
             id, path, schema_version, type, scope, namespace, canonical_namespace_id,
             summary, confidence, original_confidence, trust_level, sensitivity, status, review_state,
             requires_user_confirmation, created_at, updated_at,
             observed_at, valid_from, valid_until, ttl,
             author, source_kind, source_harness, source_device,
             body_hash, frontmatter_json, file_hash, file_mtime_ns, indexed_at, metadata_only,
             passive_recall, index_body, human_review_required, max_scope
         ) VALUES (
             :id, :path, :schema_version, :type, :scope, :namespace, :canonical_namespace_id,
             :summary, :confidence, :original_confidence, :trust_level, :sensitivity, :status, :review_state,
             :requires_user_confirmation, :created_at, :updated_at,
             :observed_at, :valid_from, :valid_until, :ttl,
             :author, :source_kind, :source_harness, :source_device,
             :body_hash, :frontmatter_json, :file_hash, :file_mtime_ns, :indexed_at, :metadata_only,
             :passive_recall, :index_body, :human_review_required, :max_scope
         )
         ON CONFLICT(id) DO UPDATE SET
             path=excluded.path, schema_version=excluded.schema_version,
             type=excluded.type, scope=excluded.scope,
             namespace=excluded.namespace, canonical_namespace_id=excluded.canonical_namespace_id,
             summary=excluded.summary, confidence=excluded.confidence,
             original_confidence=excluded.original_confidence,
             trust_level=excluded.trust_level, sensitivity=excluded.sensitivity,
             status=excluded.status, review_state=excluded.review_state,
             requires_user_confirmation=excluded.requires_user_confirmation,
             updated_at=excluded.updated_at, observed_at=excluded.observed_at,
             valid_from=excluded.valid_from, valid_until=excluded.valid_until,
             ttl=excluded.ttl, author=excluded.author,
             source_kind=excluded.source_kind, source_harness=excluded.source_harness,
             source_device=excluded.source_device, body_hash=excluded.body_hash,
             frontmatter_json=excluded.frontmatter_json,
             file_hash=excluded.file_hash, file_mtime_ns=excluded.file_mtime_ns,
             indexed_at=excluded.indexed_at, metadata_only=excluded.metadata_only,
             passive_recall=excluded.passive_recall, index_body=excluded.index_body,
             human_review_required=excluded.human_review_required,
             max_scope=excluded.max_scope",
        named_params! {
            ":id":                        memory.frontmatter.id.as_str(),
            ":path":                      &path,
            ":schema_version":            memory.frontmatter.schema_version as i64,
            ":type":                      memory_type,
            ":scope":                     scope,
            ":namespace":                 &memory.frontmatter.namespace,
            ":canonical_namespace_id":    &memory.frontmatter.canonical_namespace_id,
            ":summary":                   &memory.frontmatter.summary,
            ":confidence":                memory.frontmatter.confidence,
            ":original_confidence":       memory.frontmatter.original_confidence,
            ":trust_level":               trust_level,
            ":sensitivity":               sensitivity,
            ":status":                    status,
            ":review_state":              &memory.frontmatter.review_state,
            ":requires_user_confirmation": memory.frontmatter.requires_user_confirmation as i64,
            ":created_at":                &created_at,
            ":updated_at":                &updated_at,
            ":observed_at":               &observed_at,
            ":valid_from":                rusqlite::types::Null,
            ":valid_until":               rusqlite::types::Null,
            ":ttl":                       rusqlite::types::Null,
            ":author":                    author,
            ":source_kind":               source_kind,
            ":source_harness":            &memory.frontmatter.source.harness,
            ":source_device":             &memory.frontmatter.source.device,
            ":body_hash":                 &body_hash,
            ":frontmatter_json":          &frontmatter_json,
            ":file_hash":                 &file_hash,
            ":file_mtime_ns":             file_mtime_ns,
            ":indexed_at":                &indexed_at,
            ":metadata_only":             options.metadata_only as i64,
            ":passive_recall":            passive_recall,
            ":index_body":                index_body,
            ":human_review_required":     human_review_required,
            ":max_scope":                 max_scope,
        },
    )?;

    sync_auxiliary_tables(txn, memory)?;
    if sensitivity_upgraded {
        delete_chunk_vectors_and_meta(txn, memory.frontmatter.id.as_str())?;
    }
    sync_semantic_embedding_rows(
        txn,
        memory,
        &body_hash,
        options.active_embedding,
        active_embedding_dropped,
        sensitivity_upgraded,
    )?;

    // Rebuild chunks for this memory.
    txn.execute("DELETE FROM memory_chunks WHERE memory_id = ?1", [memory.frontmatter.id.as_str()])?;
    let may_index_body = !options.metadata_only
        && memory.frontmatter.retrieval_policy.index_body
        && !memory.frontmatter.is_merge_non_servable();
    if may_index_body {
        // Hoist the loop-invariant INSERTs out of the per-chunk loop: rusqlite's
        // `Transaction::execute` recompiles its SQL on every call, so a memory
        // with M chunks would re-parse+re-plan the same two statements M times.
        // `prepare_cached` compiles once and reuses across iterations (and across
        // memories within a bulk reindex sharing this connection).
        let enqueue_embeddings = memory.frontmatter.retrieval_policy.index_embeddings && !active_embedding_dropped;
        let mut chunk_stmt = txn.prepare_cached(
            "INSERT INTO memory_chunks(memory_id,chunk_id,body_hash,text,start_byte,end_byte)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )?;
        let mut pending_stmt = if enqueue_embeddings {
            Some(txn.prepare_cached(
                "INSERT OR IGNORE INTO pending_embedding_jobs(
                     chunk_id, provider, model_ref, dimension, content_hash, enqueued_at
                 ) VALUES (?1,?2,?3,?4,?5,?6)",
            )?)
        } else {
            None
        };
        for chunk in chunk_memory(memory) {
            chunk_stmt.execute(params![
                memory.frontmatter.id.as_str(),
                chunk.chunk_id.as_str(),
                chunk.body_hash.as_str(),
                chunk.text,
                chunk.start_byte as i64,
                chunk.end_byte as i64
            ])?;
            if let Some(pending_stmt) = pending_stmt.as_mut() {
                let enqueued_at = chrono::Utc::now().to_rfc3339();
                pending_stmt.execute(params![
                    chunk.chunk_id.as_str(),
                    options.active_embedding.provider.as_str(),
                    options.active_embedding.model_ref.as_str(),
                    i64::from(options.active_embedding.dimension),
                    chunk.body_hash.as_str(),
                    enqueued_at
                ])?;
            }
        }
    }

    Ok(())
}

fn delete_chunk_vectors_and_meta(txn: &Transaction<'_>, memory_id: &str) -> rusqlite::Result<()> {
    let rows = {
        let mut stmt = txn.prepare(
            "SELECT c.chunk_id,c.chunk_rowid,m.provider,m.model_ref,m.dimension
             FROM memory_chunks c JOIN chunk_embedding_meta m ON m.chunk_id=c.chunk_id
             WHERE c.memory_id=?1",
        )?;
        let rows = stmt.query_map([memory_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                EmbeddingTriple { provider: row.get(2)?, model_ref: row.get(3)?, dimension: row.get(4)? },
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (_, rowid, triple) in &rows {
        let table = crate::index::sqlite_vec::vector_table_name(triple);
        let exists: i64 = txn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table.as_ref()],
            |row| row.get(0),
        )?;
        if exists != 0 {
            txn.execute(&format!("DELETE FROM {table} WHERE rowid=?1"), [rowid])?;
        }
    }
    for (chunk_id, _, _) in rows {
        txn.execute("DELETE FROM chunk_vectors WHERE chunk_id=?1", [&chunk_id])?;
        txn.execute("DELETE FROM chunk_embedding_meta WHERE chunk_id=?1", [&chunk_id])?;
        txn.execute("DELETE FROM pending_embedding_jobs WHERE chunk_id=?1", [&chunk_id])?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sync_semantic_embedding_rows(
    txn: &Transaction<'_>,
    memory: &Memory,
    body_hash: &str,
    triple: &EmbeddingTriple,
    active_embedding_dropped: bool,
    sensitivity_upgraded: bool,
) -> rusqlite::Result<()> {
    let memory_id = memory.frontmatter.id.as_str();
    let servable = matches!(memory.frontmatter.status, MemoryStatus::Active | MemoryStatus::Pinned)
        && memory.frontmatter.retrieval_policy.index_embeddings;
    if !servable {
        delete_semantic_rows(txn, memory_id)?;
        return Ok(());
    }

    let enqueued_at = chrono::Utc::now().to_rfc3339();
    if sensitivity_upgraded {
        delete_semantic_vectors_and_meta(txn, memory_id)?;
    }
    if let Some(abstraction) = memory.frontmatter.abstraction.as_deref() {
        let abstraction_hash = hash_bytes(abstraction.as_bytes()).to_string();
        let previous = txn
            .query_row(
                "SELECT abstraction_hash, source_body_hash FROM memory_abstractions WHERE memory_id=?1",
                [memory_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok();
        let source_body_hash = previous
            .as_ref()
            .filter(|(old_hash, _)| old_hash == &abstraction_hash)
            .map_or(body_hash, |(_, source_hash)| source_hash.as_str());
        txn.execute(
            "INSERT INTO memory_abstractions(memory_id,abstraction,abstraction_hash,source_body_hash)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(memory_id) DO UPDATE SET abstraction=excluded.abstraction,
               abstraction_hash=excluded.abstraction_hash, source_body_hash=excluded.source_body_hash",
            params![memory_id, abstraction, abstraction_hash, source_body_hash],
        )?;
        replace_aux_job_if_stale(
            txn,
            "abstraction",
            memory_id,
            &abstraction_hash,
            triple,
            &enqueued_at,
            active_embedding_dropped,
        )?;
    } else {
        delete_aux_target(txn, "abstraction", memory_id)?;
        txn.execute("DELETE FROM memory_abstractions WHERE memory_id=?1", [memory_id])?;
    }

    for (ordinal, cue) in memory.frontmatter.cues.iter().enumerate() {
        let target_id = format!("{memory_id}:{ordinal}");
        let cue_hash = hash_bytes(cue.as_bytes()).to_string();
        txn.execute(
            "INSERT INTO memory_cues(memory_id,ordinal,cue_text,cue_hash) VALUES (?1,?2,?3,?4)
             ON CONFLICT(memory_id,ordinal) DO UPDATE SET cue_text=excluded.cue_text,cue_hash=excluded.cue_hash",
            params![memory_id, ordinal as i64, cue, cue_hash],
        )?;
        replace_aux_job_if_stale(txn, "cue", &target_id, &cue_hash, triple, &enqueued_at, active_embedding_dropped)?;
    }
    for ordinal in memory.frontmatter.cues.len()..3 {
        let target_id = format!("{memory_id}:{ordinal}");
        delete_aux_target(txn, "cue", &target_id)?;
        txn.execute("DELETE FROM memory_cues WHERE memory_id=?1 AND ordinal=?2", params![memory_id, ordinal as i64])?;
    }
    Ok(())
}

fn delete_semantic_vectors_and_meta(txn: &Transaction<'_>, memory_id: &str) -> rusqlite::Result<()> {
    delete_aux_vectors(txn, "abstraction", memory_id, None)?;
    for ordinal in 0..3 {
        delete_aux_vectors(txn, "cue", &format!("{memory_id}:{ordinal}"), None)?;
    }
    txn.execute(
        "DELETE FROM aux_embedding_meta WHERE target_id=?1 OR target_id LIKE ?2 ESCAPE '\\'",
        params![memory_id, format!("{}:%", memory_id.replace('%', "\\%").replace('_', "\\_"))],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn replace_aux_job_if_stale(
    txn: &Transaction<'_>,
    row_kind: &str,
    target_id: &str,
    content_hash: &str,
    triple: &EmbeddingTriple,
    enqueued_at: &str,
    active_embedding_dropped: bool,
) -> rusqlite::Result<()> {
    delete_aux_vectors(txn, row_kind, target_id, Some(content_hash))?;
    txn.execute(
        "DELETE FROM aux_embedding_meta WHERE row_kind=?1 AND target_id=?2 AND content_hash<>?3",
        params![row_kind, target_id, content_hash],
    )?;
    if active_embedding_dropped {
        return Ok(());
    }
    txn.execute(
        "INSERT INTO aux_pending_embedding_jobs(row_kind,target_id,content_hash,provider,model_ref,dimension,enqueued_at)
         SELECT ?1,?2,?3,?4,?5,?6,?7
         WHERE NOT EXISTS (
           SELECT 1 FROM aux_embedding_meta WHERE row_kind=?1 AND target_id=?2 AND content_hash=?3
             AND provider=?4 AND model_ref=?5 AND dimension=?6
         )
         ON CONFLICT(row_kind,target_id,provider,model_ref,dimension) DO UPDATE SET
           content_hash=excluded.content_hash,enqueued_at=excluded.enqueued_at,attempts=0,last_error=NULL",
        params![row_kind, target_id, content_hash, triple.provider, triple.model_ref, i64::from(triple.dimension), enqueued_at],
    )?;
    Ok(())
}

fn delete_aux_target(txn: &Transaction<'_>, row_kind: &str, target_id: &str) -> rusqlite::Result<()> {
    delete_aux_vectors(txn, row_kind, target_id, None)?;
    txn.execute(
        "DELETE FROM aux_pending_embedding_jobs WHERE row_kind=?1 AND target_id=?2",
        params![row_kind, target_id],
    )?;
    txn.execute("DELETE FROM aux_embedding_meta WHERE row_kind=?1 AND target_id=?2", params![row_kind, target_id])?;
    Ok(())
}

fn delete_aux_vectors(
    txn: &Transaction<'_>,
    row_kind: &str,
    target_id: &str,
    keep_hash: Option<&str>,
) -> rusqlite::Result<()> {
    let rowid = match row_kind {
        "abstraction" => txn
            .query_row("SELECT rowid FROM memory_abstractions WHERE memory_id=?1", [target_id], |row| {
                row.get::<_, i64>(0)
            })
            .ok(),
        "cue" => target_id.rsplit_once(':').and_then(|(memory_id, ordinal)| {
            txn.query_row(
                "SELECT rowid FROM memory_cues WHERE memory_id=?1 AND ordinal=?2",
                params![memory_id, ordinal.parse::<i64>().ok()?],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        }),
        _ => None,
    };
    let Some(rowid) = rowid else { return Ok(()) };
    let mut sql =
        String::from("SELECT provider,model_ref,dimension FROM aux_embedding_meta WHERE row_kind=?1 AND target_id=?2");
    if keep_hash.is_some() {
        sql.push_str(" AND content_hash<>?3");
    }
    let triples = {
        let mut stmt = txn.prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| {
            Ok(EmbeddingTriple { provider: row.get(0)?, model_ref: row.get(1)?, dimension: row.get(2)? })
        };
        if let Some(hash) = keep_hash {
            stmt.query_map(params![row_kind, target_id, hash], map)?.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![row_kind, target_id], map)?.collect::<rusqlite::Result<Vec<_>>>()?
        }
    };
    let kind = if row_kind == "abstraction" { AuxRowKind::Abstraction } else { AuxRowKind::Cue };
    for triple in triples {
        let table = crate::index::sqlite_vec::aux_vector_table_name(kind, &triple);
        let exists: i64 = txn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table.as_str()],
            |row| row.get(0),
        )?;
        if exists != 0 {
            txn.execute(&format!("DELETE FROM {table} WHERE rowid=?1"), [rowid])?;
        }
    }
    Ok(())
}

fn delete_semantic_rows(txn: &Transaction<'_>, memory_id: &str) -> rusqlite::Result<()> {
    delete_aux_target(txn, "abstraction", memory_id)?;
    for ordinal in 0..3 {
        delete_aux_target(txn, "cue", &format!("{memory_id}:{ordinal}"))?;
    }
    txn.execute("DELETE FROM memory_abstractions WHERE memory_id=?1", [memory_id])?;
    txn.execute("DELETE FROM memory_cues WHERE memory_id=?1", [memory_id])?;
    txn.execute(
        "DELETE FROM aux_pending_embedding_jobs WHERE target_id=?1 OR target_id LIKE ?2 ESCAPE '\\'",
        params![memory_id, format!("{}:%", memory_id.replace('%', "\\%").replace('_', "\\_"))],
    )?;
    txn.execute(
        "DELETE FROM aux_embedding_meta WHERE target_id=?1 OR target_id LIKE ?2 ESCAPE '\\'",
        params![memory_id, format!("{}:%", memory_id.replace('%', "\\%").replace('_', "\\_"))],
    )?;
    Ok(())
}

/// Sync priority auxiliary tables: tags, aliases, entities, evidence, supersession.
///
/// Each table is replaced wholesale for this memory_id — safe because these
/// are derived projections; canonical data lives in the Markdown file.
///
/// Deferred: memory_related, memory_regressions tables.
fn sync_auxiliary_tables(txn: &Transaction<'_>, memory: &Memory) -> rusqlite::Result<()> {
    let id = memory.frontmatter.id.as_str();
    sync_tags(txn, id, &memory.frontmatter.tags)?;
    sync_aliases(txn, id, &memory.frontmatter.aliases)?;
    sync_entities(txn, id, &memory.frontmatter.entities)?;
    sync_evidence(txn, id, &memory.frontmatter.evidence)?;
    sync_supersession(txn, id, &memory.frontmatter.supersedes)?;
    Ok(())
}

fn sync_tags(txn: &Transaction<'_>, memory_id: &str, tags: &[String]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_tags WHERE memory_id = ?1", [memory_id])?;
    let mut stmt = txn.prepare_cached("INSERT OR IGNORE INTO memory_tags(memory_id, tag) VALUES (?1, ?2)")?;
    for tag in tags {
        stmt.execute(params![memory_id, tag])?;
    }
    Ok(())
}

fn sync_aliases(txn: &Transaction<'_>, memory_id: &str, aliases: &[String]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_aliases WHERE memory_id = ?1", [memory_id])?;
    let mut stmt = txn.prepare_cached("INSERT OR IGNORE INTO memory_aliases(memory_id, alias) VALUES (?1, ?2)")?;
    for alias in aliases {
        stmt.execute(params![memory_id, alias])?;
    }
    Ok(())
}

fn sync_entities(txn: &Transaction<'_>, memory_id: &str, entities: &[crate::model::Entity]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_entity_aliases WHERE memory_id = ?1", [memory_id])?;
    txn.execute("DELETE FROM memory_entities WHERE memory_id = ?1", [memory_id])?;
    let mut entity_stmt =
        txn.prepare_cached("INSERT OR IGNORE INTO memory_entities(memory_id, entity_id, label) VALUES (?1, ?2, ?3)")?;
    let mut alias_stmt = txn.prepare_cached(
        "INSERT OR IGNORE INTO memory_entity_aliases(memory_id, entity_id, alias) VALUES (?1, ?2, ?3)",
    )?;
    for entity in entities {
        entity_stmt.execute(params![memory_id, entity.id, entity.label])?;
        for alias in &entity.aliases {
            alias_stmt.execute(params![memory_id, entity.id, alias])?;
        }
    }
    Ok(())
}

fn sync_evidence(txn: &Transaction<'_>, memory_id: &str, evidence: &[crate::model::Evidence]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_evidence WHERE memory_id = ?1", [memory_id])?;
    let mut stmt = txn.prepare_cached(
        "INSERT OR IGNORE INTO memory_evidence(
             memory_id, evidence_id, quote, quote_norm_hash, ref_text, weight, observed_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
    )?;
    for ev in evidence {
        let observed_at = ev.observed_at.as_ref().map(|t| t.to_rfc3339());
        stmt.execute(params![memory_id, ev.id, ev.quote, ev.quote_norm_hash, ev.reference, ev.weight, observed_at])?;
    }
    Ok(())
}

fn sync_supersession(txn: &Transaction<'_>, memory_id: &str, supersedes: &[MemoryId]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_supersession WHERE memory_id = ?1", [memory_id])?;
    // FK guard, parity with the v4 migration's supersession bootstrap
    // (`migrations.rs`): the edge is inserted only when its target's
    // `memories` row already exists. `memory_supersession.supersedes_id`
    // is a `REFERENCES memories(id)` FK with `PRAGMA foreign_keys = ON`,
    // so an unguarded insert against a not-yet-indexed target trips the
    // constraint and — during a *bulk* reindex that walks files in
    // unsorted order — aborts the whole reconcile. Bulk callers keep this
    // skip-then-resync behavior. Runtime write callers audit the skipped
    // target set after upsert and enqueue durable §8.3 repair state instead
    // of silently waiting for a restart.
    let mut stmt = txn.prepare_cached(
        "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id)
         SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM memories WHERE id = ?2)",
    )?;
    for supersedes_id in supersedes {
        stmt.execute(params![memory_id, supersedes_id.as_str()])?;
    }
    Ok(())
}

pub(super) fn file_consistency_state_in_connection(
    connection: &Connection,
    path: &crate::model::RepoPath,
) -> Option<(Sha256, bool)> {
    match connection.query_row(
        "SELECT file_hash, status, trust_level FROM memories WHERE path = ?1",
        [path.as_str()],
        |row| {
            let hash: String = row.get(0)?;
            let status: String = row.get(1)?;
            let trust_level: String = row.get(2)?;
            Ok((hash, status, trust_level))
        },
    ) {
        Ok((hash, status, trust_level)) => {
            let quarantined = status == "quarantined" || trust_level == "quarantined";
            Some((Sha256::new(hash), quarantined))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => {
            tracing::warn!(path = path.as_str(), %error, "index consistency lookup failed; forcing safe reindex");
            None
        }
    }
}

pub(super) fn resync_supersession_edges_sql() -> &'static str {
    "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id)
     SELECT memories.id, superseded.value
     FROM memories, json_each(memories.frontmatter_json, '$.supersedes') AS superseded
     WHERE superseded.value IS NOT NULL
       AND EXISTS (SELECT 1 FROM memories AS target WHERE target.id = superseded.value)"
}

fn observed_at_for_index(memory: &Memory) -> Option<String> {
    memory.frontmatter.observed_at.as_ref().map(chrono::DateTime::to_rfc3339)
}

fn resolve_memory_path(memory: &Memory) -> String {
    memory
        .path
        .as_ref()
        .map_or_else(|| format!("agent/patterns/{}.md", memory.frontmatter.id.as_str()), |p| p.as_str().to_string())
}
