use memory_substrate::index::{open_index, INDEX_SUPPORTED_SCHEMA_VERSION};

#[test]
fn migration_v6_is_additive_and_file_copy_restores_v5() {
    let temp = tempfile::tempdir().expect("tempdir");
    let live = temp.path().join("index.sqlite");
    let backup = temp.path().join("index-v5.backup.sqlite");
    let connection = open_index(&live).expect("create realistic current index");
    connection
        .execute_batch(
            "DROP TABLE aux_pending_embedding_jobs;
             DROP TABLE aux_embedding_meta;
             DROP TABLE memory_cues;
             DROP TABLE memory_abstractions;
             DELETE FROM schema_migrations WHERE version=6;",
        )
        .expect("downgrade copied fixture to schema 5");
    drop(connection);
    std::fs::copy(&live, &backup).expect("pre-migration copy");

    let migrated = open_index(&live).expect("migrate");
    assert_eq!(INDEX_SUPPORTED_SCHEMA_VERSION, 6);
    let version: i64 =
        migrated.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get(0)).expect("version");
    assert_eq!(version, 6);
    for table in ["memory_abstractions", "memory_cues", "aux_embedding_meta", "aux_pending_embedding_jobs"] {
        let exists: i64 = migrated
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| {
                row.get(0)
            })
            .expect("table probe");
        assert_eq!(exists, 1, "{table}");
    }
    drop(migrated);

    std::fs::copy(&backup, &live).expect("restore backup");
    let restored = rusqlite::Connection::open(&live).expect("open restored");
    let version: i64 = restored
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get(0))
        .expect("restored version");
    assert_eq!(version, 5);
    let aux_exists: i64 = restored
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_abstractions')",
            [],
            |row| row.get(0),
        )
        .expect("restored table probe");
    assert_eq!(aux_exists, 0);
}
