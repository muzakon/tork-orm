//! Tests for the SQL-file migrator: a revision chain of `.sql` files applied and
//! reverted against in-memory SQLite, with bookkeeping and checksums.

use std::path::Path;

use tork_orm_core::migration::{FileMigrator, OnMismatch};
use tork_orm_core::{Database, Value};

/// Writes a migration file into `dir`.
fn write_migration(dir: &Path, revision: &str, down: &str, name: &str, up: &str, down_sql: &str) {
    let down_line = if down.is_empty() {
        "-- down_revision:".to_string()
    } else {
        format!("-- down_revision: {down}")
    };
    let content = format!(
        "-- revision: {revision}\n{down_line}\n-- migrate:up\n{up}\n-- migrate:down\n{down_sql}\n"
    );
    std::fs::write(dir.join(format!("{revision}_{name}.sql")), content).unwrap();
}

async fn table_exists(db: &Database, name: &str) -> bool {
    let rows = db
        .fetch_all(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?".into(),
            vec![Value::Text(name.into())],
        )
        .await
        .unwrap();
    !rows.is_empty()
}

/// A standard two-migration chain: users (base) then posts.
fn seed_chain(dir: &Path) {
    write_migration(
        dir,
        "aaaa11112222",
        "",
        "create_users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        "DROP TABLE users;",
    );
    write_migration(
        dir,
        "bbbb33334444",
        "aaaa11112222",
        "create_posts",
        "CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);",
        "DROP TABLE posts;",
    );
}

#[tokio::test]
async fn up_applies_the_chain_and_down_reverts_it() {
    let dir = tempfile::tempdir().unwrap();
    seed_chain(dir.path());
    let db = Database::connect(":memory:", 1).await.unwrap();
    let migrator = FileMigrator::new(db.clone(), dir.path());

    let applied = migrator.up().await.unwrap();
    assert_eq!(applied.len(), 2);
    // Chain order: users before posts.
    assert_eq!(applied[0].name, "create_users");
    assert_eq!(applied[1].name, "create_posts");
    assert!(table_exists(&db, "users").await);
    assert!(table_exists(&db, "posts").await);

    // A second up is a no-op.
    assert_eq!(migrator.up().await.unwrap().len(), 0);

    // down 1 reverts the head (posts), keeping users.
    let reverted = migrator.down(1).await.unwrap();
    assert_eq!(reverted.len(), 1);
    assert_eq!(reverted[0].name, "create_posts");
    assert!(table_exists(&db, "users").await);
    assert!(!table_exists(&db, "posts").await);

    // down_all reverts the rest.
    assert_eq!(migrator.down_all().await.unwrap().len(), 1);
    assert!(!table_exists(&db, "users").await);
}

#[tokio::test]
async fn status_reports_applied_and_pending() {
    let dir = tempfile::tempdir().unwrap();
    seed_chain(dir.path());
    let db = Database::connect(":memory:", 1).await.unwrap();
    let migrator = FileMigrator::new(db, dir.path());

    let before = migrator.status().await.unwrap();
    assert_eq!(before.len(), 2);
    assert!(!before[0].applied);
    assert!(!before[1].applied);

    migrator.up_to("aaaa").await.unwrap(); // apply only the base (by prefix)
    let after = migrator.status().await.unwrap();
    assert!(after[0].applied);
    assert_eq!(after[0].checksum_matches, Some(true));
    assert!(!after[1].applied);
}

#[tokio::test]
async fn a_failed_migration_rolls_back_and_keeps_earlier_ones() {
    let dir = tempfile::tempdir().unwrap();
    write_migration(
        dir.path(),
        "aaaa11112222",
        "",
        "create_users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);",
        "DROP TABLE users;",
    );
    write_migration(
        dir.path(),
        "bbbb33334444",
        "aaaa11112222",
        "broken",
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY); THIS IS NOT SQL;",
        "DROP TABLE widgets;",
    );
    let db = Database::connect(":memory:", 1).await.unwrap();
    let migrator = FileMigrator::new(db.clone(), dir.path());

    let error = migrator.up().await.unwrap_err();
    assert_eq!(error.kind(), tork_orm_core::ErrorKind::Query);

    // The base committed and stays; the broken one left no partial table and no row.
    assert!(table_exists(&db, "users").await);
    assert!(!table_exists(&db, "widgets").await);
    let rows = db
        .fetch_all("SELECT revision FROM _tork_migrations".into(), vec![])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String>("revision").unwrap(), "aaaa11112222");
}

#[tokio::test]
async fn a_changed_file_is_reported_and_can_error() {
    let dir = tempfile::tempdir().unwrap();
    write_migration(
        dir.path(),
        "aaaa11112222",
        "",
        "create_users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);",
        "DROP TABLE users;",
    );
    let db = Database::connect(":memory:", 1).await.unwrap();
    FileMigrator::new(db.clone(), dir.path()).up().await.unwrap();

    // Change the up SQL of the already-applied migration.
    write_migration(
        dir.path(),
        "aaaa11112222",
        "",
        "create_users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, extra TEXT);",
        "DROP TABLE users;",
    );

    let status = FileMigrator::new(db.clone(), dir.path())
        .status()
        .await
        .unwrap();
    assert_eq!(status[0].checksum_matches, Some(false));

    let error = FileMigrator::new(db, dir.path())
        .on_checksum_mismatch(OnMismatch::Error)
        .up()
        .await
        .unwrap_err();
    assert_eq!(error.kind(), tork_orm_core::ErrorKind::Configuration);
}
