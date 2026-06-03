use memory_substrate::index::open_index;

/// Spec §10.1 mandates `PRAGMA journal_mode = WAL` on every index database.
/// Pragmas that change the journal mode must run *outside* a transaction;
/// SQLite silently keeps the old mode otherwise. This smoke test fails fast
/// if migrations regress to setting `journal_mode` inside a `BEGIN` block.
#[test]
fn open_index_enables_wal_journal_mode() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0)).expect("query journal_mode");
    assert_eq!(mode.to_ascii_lowercase(), "wal", "expected WAL journal mode, got {mode}");
}

/// Spec §10.1 also pins `synchronous = NORMAL` for the WAL fast-path.
#[test]
fn open_index_sets_synchronous_normal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let synchronous: i64 = connection.query_row("PRAGMA synchronous", [], |row| row.get(0)).expect("query synchronous");
    // SQLite reports `NORMAL` as integer 1.
    assert_eq!(synchronous, 1, "expected synchronous=NORMAL (1), got {synchronous}");
}

/// A `busy_timeout` keeps writers waiting (rather than failing immediately with
/// SQLITE_BUSY) when they race the startup reconciler, merge driver, or a second
/// connection under WAL.
#[test]
fn open_index_sets_busy_timeout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let busy_timeout: i64 =
        connection.query_row("PRAGMA busy_timeout", [], |row| row.get(0)).expect("query busy_timeout");
    assert_eq!(busy_timeout, 5000, "expected busy_timeout=5000ms, got {busy_timeout}");
}

/// Schema-version gate must reject databases above the supported version.
#[test]
fn open_index_rejects_unsupported_schema_version() {
    use memory_substrate::OpenError;

    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("index.sqlite");
    {
        let connection = open_index(&path).expect("first open populates schema_migrations");
        connection
            .execute("INSERT INTO schema_migrations(version) VALUES (?1)", [9_999])
            .expect("seed future schema version");
    }

    let err = open_index(&path).expect_err("future version refused");
    let OpenError::IndexSchemaVersionUnsupported { found, supported } = err else {
        panic!("expected IndexSchemaVersionUnsupported, got {err:?}");
    };
    assert!(found > supported, "found ({found}) must exceed supported ({supported})");
}
