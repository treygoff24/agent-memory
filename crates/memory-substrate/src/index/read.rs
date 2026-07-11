//! Index read helpers: dynamic filter builders, namespace-prefix parsing,
//! recall-index row marshalling, and the auxiliary tag/alias/entity hydration.

use std::collections::BTreeMap;

use rusqlite::{params_from_iter, Connection};

use crate::error::{SubstrateError, SubstrateResult};
use crate::model::{
    AuxScope, Entity, MemoryId, MemoryQuery, MemoryStatus, QueryResult, RecallIndexQuery, RecallIndexRow, RepoPath,
    Scope, Sensitivity, SourceKind,
};

use super::util::{invalid_column_value, parse_index_time};
use super::{bucketed_in_clause_width, pad_in_clause_bindings, sql_placeholders};
use crate::index::query::MERGE_NON_SERVABLE_SQL;

pub(super) fn append_memory_query_filters(
    query: &MemoryQuery,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) -> SubstrateResult<()> {
    if let Some(id) = query.id.as_ref() {
        filters.push("memories.id = ?".to_string());
        bindings.push(rusqlite::types::Value::Text(id.as_str().to_string()));
    }
    if !query.include_metadata_only {
        filters.push("memories.metadata_only = 0".to_string());
    }
    if let Some(status) = query.status {
        filters.push("memories.status = ?".to_string());
        bindings.push(rusqlite::types::Value::Text(status.as_db_str().to_string()));
    }
    append_namespace_filter(query.namespace_prefix.as_deref(), filters, bindings)?;
    if query.passive_recall_only {
        filters.push("memories.passive_recall = 1".to_string());
    }
    if let Some(updated_since) = query.updated_since.as_ref() {
        filters.push("memories.updated_at >= ?".to_string());
        bindings.push(rusqlite::types::Value::Text(updated_since.to_rfc3339()));
    }
    Ok(())
}

pub(super) fn append_recall_index_filters(
    query: &RecallIndexQuery,
    include_metadata_only: bool,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) -> SubstrateResult<()> {
    append_namespace_filter(query.namespace_prefix.as_deref(), filters, bindings)?;
    if !include_metadata_only {
        filters.push("memories.metadata_only = 0".to_string());
    }
    if !query.statuses.is_empty() {
        let placeholders = sql_placeholders(query.statuses.len());
        filters.push(format!("memories.status IN ({placeholders})"));
        for status in &query.statuses {
            bindings.push(rusqlite::types::Value::Text(status.as_db_str().to_string()));
        }
    }
    if query.passive_recall_only {
        filters.push("memories.passive_recall = 1".to_string());
    }
    if let Some(updated_since) = query.updated_since.as_ref() {
        filters.push("memories.updated_at >= ?".to_string());
        bindings.push(rusqlite::types::Value::Text(updated_since.to_rfc3339()));
    }
    if query.exclude_merge_non_servable {
        filters.push(MERGE_NON_SERVABLE_SQL.to_string());
    }
    Ok(())
}

fn append_namespace_filter(
    namespace_prefix: Option<&str>,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) -> SubstrateResult<()> {
    match namespace_prefix.map(parse_namespace_prefix).transpose()? {
        Some(NamespaceFilter::Scope(scope)) => {
            filters.push("memories.scope = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(scope.to_string()));
        }
        Some(NamespaceFilter::ScopeAndCanonicalId { scope, canonical_id }) => {
            filters.push("memories.scope = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(scope.to_string()));
            filters.push("memories.canonical_namespace_id = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(canonical_id));
        }
        None => {}
    }
    Ok(())
}

pub(super) fn append_match_term_filters(
    query: &RecallIndexQuery,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) {
    // Recall match terms intentionally use union semantics. A passive recall request should surface
    // candidates matching any observed tag, alias, entity id, or entity alias from the current turn.
    let terms = query.match_terms.iter().filter(|term| !term.trim().is_empty()).collect::<Vec<_>>();
    if terms.is_empty() {
        return;
    }

    let mut clauses = Vec::new();
    for term in terms {
        clauses.push(
            "(EXISTS (SELECT 1 FROM memory_tags WHERE memory_tags.memory_id = memories.id AND memory_tags.tag = ? COLLATE NOCASE)
              OR EXISTS (SELECT 1 FROM memory_aliases WHERE memory_aliases.memory_id = memories.id AND memory_aliases.alias = ? COLLATE NOCASE)
              OR EXISTS (SELECT 1 FROM memory_entities WHERE memory_entities.memory_id = memories.id AND (memory_entities.entity_id = ? OR memory_entities.label = ? COLLATE NOCASE))
              OR EXISTS (SELECT 1 FROM memory_entity_aliases WHERE memory_entity_aliases.memory_id = memories.id AND memory_entity_aliases.alias = ? COLLATE NOCASE))"
                .to_string(),
        );
        for _ in 0..5 {
            bindings.push(rusqlite::types::Value::Text(term.to_string()));
        }
    }
    filters.push(format!("({})", clauses.join(" OR ")));
}

pub(super) fn append_filters_and_order(sql: &mut String, filters: Vec<String>, order_by: &str) {
    if !filters.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&filters.join(" AND "));
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(order_by);
}

enum NamespaceFilter {
    Scope(&'static str),
    ScopeAndCanonicalId { scope: &'static str, canonical_id: String },
}

fn parse_namespace_prefix(value: &str) -> SubstrateResult<NamespaceFilter> {
    match value {
        "me" => Ok(NamespaceFilter::Scope("user")),
        "agent" => Ok(NamespaceFilter::Scope("agent")),
        _ if value.starts_with("project:") => parse_scoped_namespace(value, "project:", "project"),
        _ if value.starts_with("org:") => parse_scoped_namespace(value, "org:", "org"),
        _ => Err(invalid_namespace_prefix(value)),
    }
}

fn parse_scoped_namespace(value: &str, prefix: &str, scope: &'static str) -> SubstrateResult<NamespaceFilter> {
    let canonical_id = value.strip_prefix(prefix).unwrap_or_default();
    if canonical_id.is_empty() || canonical_id.contains(':') {
        return Err(invalid_namespace_prefix(value));
    }
    Ok(NamespaceFilter::ScopeAndCanonicalId { scope, canonical_id: canonical_id.to_string() })
}

fn invalid_namespace_prefix(value: &str) -> SubstrateError {
    SubstrateError::InvalidQuery {
        field: "namespace_prefix".to_string(),
        value: value.to_string(),
        message: "invalid_query: expected one of me, agent, project:<canonical_id>, org:<canonical_id>".to_string(),
    }
}

pub(super) fn collect_query_results(
    conn: &Connection,
    sql: &str,
    bindings: Vec<rusqlite::types::Value>,
) -> rusqlite::Result<Vec<QueryResult>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(row_to_result(row)?);
    }
    Ok(results)
}

/// Marshal a recall-index row.
///
/// `source_identity` mirrors [`RecallIndexQuery::source_identity`]: when set,
/// the SELECT projected the four extra identity/merge-diagnostics columns
/// (indices 19-22) and they are read here; when unset they are absent from the
/// row and left `None`. `source_harness` (index 18) is always present.
pub(super) fn row_to_recall_index_row(
    row: &rusqlite::Row<'_>,
    source_identity: bool,
) -> rusqlite::Result<RecallIndexRow> {
    let (source_session_id, author_harness, author_session_id, merge_diagnostics_json) = if source_identity {
        (row.get(19)?, row.get(20)?, row.get(21)?, row.get(22)?)
    } else {
        (None, None, None, None)
    };
    let status_text: String = row.get(3)?;
    let scope_text: String = row.get(4)?;
    let source_kind_text: String = row.get(9)?;
    let sensitivity_text: String = row.get(11)?;
    let max_scope_text: String = row.get(17)?;
    Ok(RecallIndexRow {
        id: MemoryId::new(row.get::<_, String>(0)?),
        // `from_unchecked`: path was validated at index-write time; hydrating from DB row.
        path: RepoPath::from_unchecked(row.get::<_, String>(1)?),
        summary: row.get(2)?,
        status: MemoryStatus::from_db_str(&status_text).ok_or_else(|| invalid_column_value("status", &status_text))?,
        scope: Scope::from_db_str(&scope_text).ok_or_else(|| invalid_column_value("scope", &scope_text))?,
        canonical_namespace_id: row.get(5)?,
        updated_at: parse_index_time(row.get::<_, String>(6)?.as_str())?,
        indexed_at: parse_index_time(row.get::<_, String>(7)?.as_str())?,
        confidence: row.get(8)?,
        source_kind: SourceKind::from_db_str(&source_kind_text)
            .ok_or_else(|| invalid_column_value("source_kind", &source_kind_text))?,
        source_device: row.get(10)?,
        sensitivity: Sensitivity::from_db_str(&sensitivity_text)
            .ok_or_else(|| invalid_column_value("sensitivity", &sensitivity_text))?,
        passive_recall: row.get::<_, i64>(12)? != 0,
        index_body: row.get::<_, i64>(13)? != 0,
        requires_user_confirmation: row.get::<_, i64>(14)? != 0,
        review_state: row.get(15)?,
        human_review_required: row.get::<_, i64>(16)? != 0,
        max_scope: Scope::from_db_str(&max_scope_text).ok_or_else(|| invalid_column_value("scope", &max_scope_text))?,
        source_harness: row.get(18)?,
        source_session_id,
        author_harness,
        author_session_id,
        merge_diagnostics_json,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: Vec::new(),
    })
}

pub(super) fn hydrate_recall_index_auxiliary(
    conn: &Connection,
    rows: &mut [RecallIndexRow],
    scope: AuxScope,
) -> rusqlite::Result<()> {
    if rows.is_empty() || scope == AuxScope::None {
        return Ok(());
    }

    let ids = rows.iter().map(|row| row.id.as_str().to_owned()).collect::<Vec<_>>();

    // Tags are needed by `All` and `Tags`; aliases/entities only by `All`/`Entities`.
    let want_tags = matches!(scope, AuxScope::All | AuxScope::Tags);
    let want_aliases = scope == AuxScope::All;
    let want_entities = matches!(scope, AuxScope::All | AuxScope::Entities);

    let mut tags_by_memory = if want_tags {
        read_strings_by_memory(
            conn,
            AuxiliaryStringTable {
                table: "memory_tags",
                column: "tag",
                order_by: "ORDER BY memory_id, tag COLLATE NOCASE, tag",
            },
            &ids,
        )?
    } else {
        BTreeMap::new()
    };
    let mut aliases_by_memory = if want_aliases {
        read_strings_by_memory(
            conn,
            AuxiliaryStringTable {
                table: "memory_aliases",
                column: "alias",
                order_by: "ORDER BY memory_id, alias COLLATE NOCASE, alias",
            },
            &ids,
        )?
    } else {
        BTreeMap::new()
    };
    let mut entities_by_memory = if want_entities { read_entities_by_memory(conn, &ids)? } else { BTreeMap::new() };

    for row in rows {
        if want_tags {
            row.tags = tags_by_memory.remove(row.id.as_str()).unwrap_or_default();
        }
        if want_aliases {
            row.aliases = aliases_by_memory.remove(row.id.as_str()).unwrap_or_default();
        }
        if want_entities {
            row.entities = entities_by_memory.remove(row.id.as_str()).unwrap_or_default();
        }
    }
    Ok(())
}

struct AuxiliaryStringTable {
    table: &'static str,
    column: &'static str,
    order_by: &'static str,
}

fn read_strings_by_memory(
    conn: &Connection,
    table: AuxiliaryStringTable,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<String, Vec<String>>> {
    let width = bucketed_in_clause_width(ids.len());
    let placeholders = sql_placeholders(width);
    let sql = format!(
        "SELECT memory_id,{} FROM {} WHERE memory_id IN ({placeholders}) {}",
        table.column, table.table, table.order_by
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(ids, width)))?;
    let mut values = BTreeMap::<String, Vec<String>>::new();
    while let Some(row) = rows.next()? {
        values.entry(row.get::<_, String>(0)?).or_default().push(row.get(1)?);
    }
    Ok(values)
}

pub(super) fn read_entities_by_memory(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<String, Vec<Entity>>> {
    let width = bucketed_in_clause_width(ids.len());
    let placeholders = sql_placeholders(width);
    let sql = format!(
        "SELECT memory_id,entity_id,label FROM memory_entities
         WHERE memory_id IN ({placeholders})
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(ids, width)))?;
    let mut aliases_by_entity = read_entity_aliases_by_memory(conn, ids)?;
    let mut entities = BTreeMap::<String, Vec<Entity>>::new();
    while let Some(row) = rows.next()? {
        let memory_id = row.get::<_, String>(0)?;
        let entity_id = row.get::<_, String>(1)?;
        let label = row.get::<_, String>(2)?;
        // `(memory_id, entity_id)` is unique per row, so `remove` is safe and
        // hands us the owned alias Vec instead of cloning a throwaway copy.
        let aliases = aliases_by_entity.remove(&(memory_id.clone(), entity_id.clone())).unwrap_or_default();
        entities.entry(memory_id).or_default().push(Entity { id: entity_id, label, aliases });
    }
    Ok(entities)
}

/// Read every indexed entity (with aliases) as ordered `(memory_id, Entity)`
/// pairs. Unfiltered sibling of [`read_entities_by_memory`]; reads only the two
/// entity tables.
pub(super) fn read_all_entity_rows(conn: &Connection) -> rusqlite::Result<Vec<(MemoryId, Entity)>> {
    let aliases_by_entity = read_all_entity_aliases(conn)?;
    let mut stmt = conn.prepare_cached(
        "SELECT memory_id,entity_id,label FROM memory_entities
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id",
    )?;
    let mut rows = stmt.query([])?;
    let mut entities = Vec::new();
    while let Some(row) = rows.next()? {
        let memory_id = row.get::<_, String>(0)?;
        let entity_id = row.get::<_, String>(1)?;
        let label = row.get::<_, String>(2)?;
        let aliases = aliases_by_entity.get(&(memory_id.clone(), entity_id.clone())).cloned().unwrap_or_default();
        entities.push((MemoryId::new(memory_id), Entity { id: entity_id, label, aliases }));
    }
    Ok(entities)
}

fn read_all_entity_aliases(conn: &Connection) -> rusqlite::Result<BTreeMap<(String, String), Vec<String>>> {
    let mut stmt = conn.prepare_cached(
        "SELECT memory_id,entity_id,alias FROM memory_entity_aliases
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id, alias COLLATE NOCASE, alias",
    )?;
    let mut rows = stmt.query([])?;
    let mut aliases = BTreeMap::<(String, String), Vec<String>>::new();
    while let Some(row) = rows.next()? {
        aliases.entry((row.get(0)?, row.get(1)?)).or_default().push(row.get(2)?);
    }
    Ok(aliases)
}

fn read_entity_aliases_by_memory(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<(String, String), Vec<String>>> {
    let width = bucketed_in_clause_width(ids.len());
    let placeholders = sql_placeholders(width);
    let sql = format!(
        "SELECT memory_id,entity_id,alias FROM memory_entity_aliases
         WHERE memory_id IN ({placeholders})
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id, alias COLLATE NOCASE, alias"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(ids, width)))?;
    let mut aliases = BTreeMap::<(String, String), Vec<String>>::new();
    while let Some(row) = rows.next()? {
        aliases.entry((row.get(0)?, row.get(1)?)).or_default().push(row.get(2)?);
    }
    Ok(aliases)
}

fn row_to_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueryResult> {
    Ok(QueryResult {
        id: MemoryId::new(row.get::<_, String>(0)?),
        // `from_unchecked`: path was validated at index-write time; hydrating from DB row.
        path: RepoPath::from_unchecked(row.get::<_, String>(1)?),
        summary: row.get(2)?,
    })
}
