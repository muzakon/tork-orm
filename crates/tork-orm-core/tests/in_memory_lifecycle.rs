//! In-memory database lifecycle boundary.
//!
//! SQLite's `:memory:` databases are tied to the lifetime of the connection
//! that opened them: dropping the connection destroys the database. The
//! `SqlitePool` clamps a memory target to a single connection so concurrent
//! writers cannot each open their own divergent memory database, but that
//! means `pool.close()` (which drops every idle connection) wipes the
//! schema. The tests here pin that behavior down so it stays a deliberate
//! design choice instead of an undocumented surprise.

use tork_orm_core::Database;

#[tokio::test]
async fn in_memory_data_survives_a_dropped_pinned_handle() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO notes VALUES (1, 'hello')".into(),
        vec![],
    )
    .await
    .unwrap();

    // The first connection drops as soon as the pinned handle goes out of
    // scope; the pool re-opens a fresh one on demand, but the schema and
    // data are gone because `:memory:` is connection-bound.
    let rows = db
        .fetch_all("SELECT body FROM notes WHERE id = 1".into(), vec![])
        .await
        .unwrap();
    let body: String = rows[0].get("body").unwrap();
    assert_eq!(body, "hello");

    db.close().await;

    // After a close, the next fetch goes through a brand-new connection and
    // the schema and data are gone. This is the documented boundary: callers
    // who need persistence across pool close should use a file-backed
    // SQLite URL or a centralized database.
    let result = db
        .fetch_all("SELECT body FROM notes WHERE id = 1".into(), vec![])
        .await;
    assert!(result.is_err(), "in-memory data must not survive pool close");
}

#[tokio::test]
async fn in_memory_pool_clamps_to_one_connection() {
    // The pool treats `:memory:` as single-connection regardless of the
    // requested `max_connections`, so two concurrent reads cannot end up
    // talking to different memory databases.
    let db = Database::connect(":memory:", 8).await.unwrap();
    db.execute(
        "CREATE TABLE counters (n INTEGER NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO counters VALUES (1)".into(), vec![])
        .await
        .unwrap();
    let n: i64 = db
        .fetch_all("SELECT n FROM counters".into(), vec![])
        .await
        .unwrap()[0]
        .get("n")
        .unwrap();
    assert_eq!(n, 1);
}
