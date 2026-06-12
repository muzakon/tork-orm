//! Tests for Model::find, Model::get_or_none, and QuerySet::one_or_none
//! against in-memory SQLite.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50), unique)]
    username: String,
    is_active: bool,
}

async fn seed() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL UNIQUE, is_active INTEGER NOT NULL)"
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

// ── Model::find ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn find_returns_row_when_pk_exists() {
    let db = seed().await;
    let user = User::find(&db, 1_i64).await.unwrap();
    assert_eq!(user.id, 1);
    assert_eq!(user.username, "alice");
}

#[tokio::test]
async fn find_returns_not_found_for_missing_pk() {
    let db = seed().await;
    let err = User::find(&db, 999_i64).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn find_accepts_different_value_types() {
    let db = seed().await;
    let user = User::find(&db, 2_i32).await.unwrap();
    assert_eq!(user.username, "bob");
}

// ── Model::get_or_none ───────────────────────────────────────────────────────

#[tokio::test]
async fn get_or_none_returns_some_when_pk_exists() {
    let db = seed().await;
    let user = User::get_or_none(&db, 1_i64).await.unwrap();
    assert!(user.is_some());
    assert_eq!(user.unwrap().username, "alice");
}

#[tokio::test]
async fn get_or_none_returns_none_for_missing_pk() {
    let db = seed().await;
    let user = User::get_or_none(&db, 999_i64).await.unwrap();
    assert!(user.is_none());
}

#[tokio::test]
async fn get_or_none_is_idempotent() {
    let db = seed().await;
    let user = User::get_or_none(&db, 1_i64).await.unwrap();
    assert!(user.is_some());

    // Fetching the same key again gives the same row.
    let again = User::get_or_none(&db, 1_i64).await.unwrap();
    assert_eq!(user, again);
}

// ── QuerySet::one_or_none ────────────────────────────────────────────────────

#[tokio::test]
async fn one_or_none_returns_row_for_unique_filter() {
    let db = seed().await;
    let user = User::query()
        .filter(User::username.eq("alice"))
        .one_or_none(&db)
        .await
        .unwrap();
    assert_eq!(user.unwrap().username, "alice");
}

#[tokio::test]
async fn one_or_none_returns_none_for_no_match() {
    let db = seed().await;
    let user = User::query()
        .filter(User::username.eq("nobody"))
        .one_or_none(&db)
        .await
        .unwrap();
    assert!(user.is_none());
}

#[tokio::test]
async fn one_or_none_errors_on_multiple_matches() {
    let db = seed().await;
    // Both alice and bob are active — filtering by is_active alone returns many.
    let err = User::query()
        .filter(User::is_active.eq(true))
        .one_or_none(&db)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MultipleFound);
}

#[tokio::test]
async fn one_or_none_with_pk_filter_returns_single_row() {
    let db = seed().await;
    // Filtering by the unique PK guarantees at most one row.
    let user = User::query()
        .filter(User::id.eq(2_i64))
        .one_or_none(&db)
        .await
        .unwrap()
        .expect("user 2 should exist");
    assert_eq!(user.username, "bob");
}
