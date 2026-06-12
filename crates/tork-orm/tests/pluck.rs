//! Tests for QuerySet::pluck — single-column value extraction
//! against in-memory SQLite.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    username: String,
    is_active: bool,
}

async fn seed() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL, is_active INTEGER NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    for (username, active) in [("alice", true), ("bob", false), ("carol", true)] {
        db.execute(
            "INSERT INTO users (username, is_active) VALUES (?, ?)".into(),
            vec![Value::Text(username.into()), Value::Bool(active)],
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn pluck_string_column() {
    let db = seed().await;
    let usernames = User::query()
        .order_by(User::id.asc())
        .pluck(&db, User::username)
        .await
        .unwrap();
    assert_eq!(usernames, vec!["alice", "bob", "carol"]);
}

#[tokio::test]
async fn pluck_int_column() {
    let db = seed().await;
    let ids = User::query()
        .order_by(User::id.asc())
        .pluck(&db, User::id)
        .await
        .unwrap();
    assert_eq!(ids, vec![1_i64, 2, 3]);
}

#[tokio::test]
async fn pluck_with_filter() {
    let db = seed().await;
    let active_names = User::query()
        .filter(User::is_active.eq(true))
        .order_by(User::id.asc())
        .pluck(&db, User::username)
        .await
        .unwrap();
    assert_eq!(active_names, vec!["alice", "carol"]);
}

#[tokio::test]
async fn pluck_distinct() {
    let db = seed().await;
    let statuses = User::query()
        .distinct()
        .pluck(&db, User::is_active)
        .await
        .unwrap();
    // There are 2 distinct values (true and false) across 3 rows.
    let mut sorted: Vec<bool> = statuses;
    sorted.sort_unstable();
    assert_eq!(sorted, vec![false, true]);
}

#[tokio::test]
async fn pluck_empty_result_returns_empty_vec() {
    let db = seed().await;
    let names: Vec<String> = User::query()
        .filter(User::username.eq("nobody"))
        .pluck(&db, User::username)
        .await
        .unwrap();
    assert!(names.is_empty());
}

#[tokio::test]
async fn pluck_respects_limit_and_offset() {
    let db = seed().await;
    let names = User::query()
        .order_by(User::id.asc())
        .limit(2)
        .offset(1)
        .pluck(&db, User::username)
        .await
        .unwrap();
    // With offset 1 and limit 2, we get bob and carol (skipping alice).
    assert_eq!(names, vec!["bob", "carol"]);
}
