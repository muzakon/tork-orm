//! Tests for the [`QuerySet::chunk`] terminal method.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    username: String,
}

async fn seed() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    for name in ["alice", "bob", "carol", "dave", "eve", "frank", "grace"] {
        db.execute(
            "INSERT INTO users (username) VALUES (?)".into(),
            vec![Value::Text(name.into())],
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn chunk_returns_all_rows_in_batches() {
    let db = seed().await;
    let batches = User::query()
        .order_by(User::id.asc())
        .chunk(&db, 3)
        .await
        .unwrap();

    assert_eq!(batches.len(), 3); // 7 rows / 3 = 3 batches (3, 3, 1)
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
    assert_eq!(batches[2].len(), 1);

    let all: Vec<&str> = batches
        .iter()
        .flat_map(|b| b.iter().map(|u| u.username.as_str()))
        .collect();
    assert_eq!(all, ["alice", "bob", "carol", "dave", "eve", "frank", "grace"]);
}

#[tokio::test]
async fn chunk_with_size_one_returns_each_row_separately() {
    let db = seed().await;
    let batches = User::query()
        .order_by(User::id.asc())
        .chunk(&db, 1)
        .await
        .unwrap();

    assert_eq!(batches.len(), 7);
    for batch in &batches {
        assert_eq!(batch.len(), 1);
    }
}

#[tokio::test]
async fn chunk_with_filter_only_chunks_matches() {
    let db = seed().await;
    // Only names starting with "a" — just "alice"
    let batches = User::query()
        .order_by(User::id.asc())
        .filter_raw("username LIKE ?", ["a%"])
        .chunk(&db, 2)
        .await
        .unwrap();

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[0][0].username, "alice");
}

#[tokio::test]
async fn chunk_on_empty_query_returns_empty_vec() {
    let db = seed().await;
    let batches = User::query()
        .filter(User::username.eq("nonexistent"))
        .chunk(&db, 10)
        .await
        .unwrap();

    assert!(batches.is_empty());
}
