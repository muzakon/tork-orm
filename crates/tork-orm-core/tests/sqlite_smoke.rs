//! Smoke test for the SQLite driver: it proves the async bridge, bound-parameter
//! path, and row mapping work end to end before any higher-level query layer
//! exists.

use tork_orm_core::{Database, Value};

/// Creating a table, inserting with bound parameters, and selecting back the row
/// round-trips through the pool and the [`Row`](tork_orm_core::Row) mapping.
#[tokio::test]
async fn create_insert_select_roundtrip() {
    let db = Database::connect(":memory:", 4).await.unwrap();

    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL, is_active INTEGER NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    let inserted = db
        .execute(
            "INSERT INTO users (username, is_active) VALUES (?, ?)".into(),
            vec![Value::Text("alice".into()), Value::Bool(true)],
        )
        .await
        .unwrap();
    assert_eq!(inserted.rows_affected, 1);
    assert_eq!(inserted.last_insert_rowid, 1);

    let rows = db
        .fetch_all(
            "SELECT id, username, is_active FROM users".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.get::<i64>("id").unwrap(), 1);
    assert_eq!(row.get::<String>("username").unwrap(), "alice");
    assert!(row.get::<bool>("is_active").unwrap());
}

/// A value containing SQL metacharacters is bound, not interpolated, so it cannot
/// alter the statement and round-trips verbatim.
#[tokio::test]
async fn bound_parameters_are_not_interpolated() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();

    let hostile = "alice'); DROP TABLE notes; --";
    db.execute(
        "INSERT INTO notes (body) VALUES (?)".into(),
        vec![Value::Text(hostile.into())],
    )
    .await
    .unwrap();

    let rows = db
        .fetch_all(
            "SELECT body FROM notes WHERE body = ?".into(),
            vec![Value::Text(hostile.into())],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String>("body").unwrap(), hostile);
}

/// Reading a `NULL` column into an `Option` yields `None`, and a non-null into
/// `Some`.
#[tokio::test]
async fn nullable_columns_map_to_option() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE people (id INTEGER PRIMARY KEY, nickname TEXT)".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO people (nickname) VALUES (?), (?)".into(),
        vec![Value::Null, Value::Text("ace".into())],
    )
    .await
    .unwrap();

    let rows = db
        .fetch_all("SELECT nickname FROM people ORDER BY id".into(), vec![])
        .await
        .unwrap();
    assert_eq!(rows[0].get::<Option<String>>("nickname").unwrap(), None);
    assert_eq!(
        rows[1].get::<Option<String>>("nickname").unwrap(),
        Some("ace".to_string())
    );
}

/// A batch runs several semicolon-separated statements in one call.
#[tokio::test]
async fn execute_batch_runs_multiple_statements() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute_batch(
        "CREATE TABLE a (id INTEGER PRIMARY KEY); \
         CREATE TABLE b (id INTEGER PRIMARY KEY); \
         INSERT INTO a (id) VALUES (1), (2);"
            .into(),
    )
    .await
    .unwrap();

    let count = db
        .fetch_all("SELECT COUNT(*) FROM a".into(), vec![])
        .await
        .unwrap();
    assert_eq!(count[0].get_index::<i64>(0).unwrap(), 2);

    let table_b = db
        .fetch_all(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'b'".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(table_b.len(), 1);
}
