//! SQLite schema. Aligned with spec §10.1.

/// Current schema SQL.
///
/// Pragmas that must run outside transactions (`journal_mode`, `synchronous`)
/// are applied separately by [`crate::index::open_index`]. Only DDL belongs in
/// this batch.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations(
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS memories(
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
  metadata_only               INTEGER NOT NULL DEFAULT 0,
  passive_recall              INTEGER NOT NULL DEFAULT 1,
  index_body                  INTEGER NOT NULL DEFAULT 1,
  human_review_required       INTEGER NOT NULL DEFAULT 0,
  max_scope                   TEXT NOT NULL DEFAULT 'agent'
);

CREATE INDEX IF NOT EXISTS idx_memories_scope_canon_status_sens_updated
  ON memories(scope, canonical_namespace_id, status, sensitivity, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_type_status_updated
  ON memories(type, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_source_updated
  ON memories(source_kind, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_review
  ON memories(review_state, requires_user_confirmation);
CREATE INDEX IF NOT EXISTS idx_memories_path_nocase
  ON memories(path COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS memory_chunks(
  chunk_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id   TEXT NOT NULL,
  chunk_id    TEXT NOT NULL UNIQUE,
  body_hash   TEXT NOT NULL,
  text        TEXT NOT NULL,
  start_byte  INTEGER NOT NULL,
  end_byte    INTEGER NOT NULL,
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE VIRTUAL TABLE IF NOT EXISTS memory_chunks_fts USING fts5(text, content='memory_chunks', content_rowid='chunk_rowid');
CREATE TRIGGER IF NOT EXISTS memory_chunks_ai AFTER INSERT ON memory_chunks BEGIN
  INSERT INTO memory_chunks_fts(rowid, text) VALUES (new.chunk_rowid, new.text);
END;
CREATE TRIGGER IF NOT EXISTS memory_chunks_ad AFTER DELETE ON memory_chunks BEGIN
  INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, text) VALUES('delete', old.chunk_rowid, old.text);
END;
CREATE TRIGGER IF NOT EXISTS memory_chunks_au AFTER UPDATE ON memory_chunks BEGIN
  INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, text) VALUES('delete', old.chunk_rowid, old.text);
  INSERT INTO memory_chunks_fts(rowid, text) VALUES (new.chunk_rowid, new.text);
END;

CREATE TABLE IF NOT EXISTS pending_embedding_jobs(
  chunk_id      TEXT NOT NULL,
  provider      TEXT NOT NULL,
  model_ref     TEXT NOT NULL,
  dimension     INTEGER NOT NULL,
  content_hash  TEXT NOT NULL,
  enqueued_at   TEXT NOT NULL,
  attempts      INTEGER NOT NULL DEFAULT 0,
  last_error    TEXT,
  PRIMARY KEY(chunk_id, provider, model_ref, dimension)
);
CREATE INDEX IF NOT EXISTS idx_pending_embedding_jobs_enqueued
  ON pending_embedding_jobs(enqueued_at);

CREATE TABLE IF NOT EXISTS chunk_vectors(
  chunk_id    TEXT NOT NULL,
  provider    TEXT NOT NULL,
  model_ref   TEXT NOT NULL,
  dimension   INTEGER NOT NULL,
  vector_json TEXT NOT NULL,
  PRIMARY KEY(chunk_id, provider, model_ref, dimension)
);

CREATE TABLE IF NOT EXISTS chunk_embedding_meta(
  chunk_id      TEXT NOT NULL,
  provider      TEXT NOT NULL,
  model_ref     TEXT NOT NULL,
  dimension     INTEGER NOT NULL,
  vector_table  TEXT NOT NULL,
  embedded_at   TEXT NOT NULL,
  content_hash  TEXT NOT NULL,
  PRIMARY KEY(chunk_id, provider, model_ref, dimension),
  FOREIGN KEY(chunk_id) REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS dropped_embedding_triples(
  provider    TEXT NOT NULL,
  model_ref   TEXT NOT NULL,
  dimension   INTEGER NOT NULL,
  dropped_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY(provider, model_ref, dimension)
);

-- Priority auxiliary tables (spec §10.1): tags, aliases, entities, evidence.
-- All foreign-key on DELETE CASCADE so they clean up automatically with
-- memory row deletes.
-- Deferred: memory_supersession, memory_related, memory_regressions,
-- memory_regression_occurrences — add when query paths need them.

CREATE TABLE IF NOT EXISTS memory_tags(
  memory_id  TEXT NOT NULL,
  tag        TEXT NOT NULL,
  PRIMARY KEY(memory_id, tag),
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_memory_tags_tag ON memory_tags(tag);

CREATE TABLE IF NOT EXISTS memory_aliases(
  memory_id  TEXT NOT NULL,
  alias      TEXT NOT NULL COLLATE NOCASE,
  PRIMARY KEY(memory_id, alias),
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_memory_aliases_alias ON memory_aliases(alias COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS memory_entities(
  memory_id   TEXT NOT NULL,
  entity_id   TEXT NOT NULL,
  label       TEXT NOT NULL,
  PRIMARY KEY(memory_id, entity_id),
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_memory_entities_entity_id ON memory_entities(entity_id);

CREATE TABLE IF NOT EXISTS memory_entity_aliases(
  memory_id   TEXT NOT NULL,
  entity_id   TEXT NOT NULL,
  alias       TEXT NOT NULL COLLATE NOCASE,
  PRIMARY KEY(memory_id, entity_id, alias),
  FOREIGN KEY(memory_id, entity_id) REFERENCES memory_entities(memory_id, entity_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS memory_evidence(
  memory_id       TEXT NOT NULL,
  evidence_id     TEXT NOT NULL,
  quote           TEXT NOT NULL,
  quote_norm_hash TEXT,
  ref_text        TEXT NOT NULL,
  weight          REAL NOT NULL DEFAULT 1.0,
  observed_at     TEXT,
  PRIMARY KEY(memory_id, evidence_id),
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
"#;
