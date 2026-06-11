//! Tests for reading the live schema (table existence, columns, and indexes).

#![cfg(feature = "migrations")]

use tork_orm_core::migration::introspect::{existing_columns, existing_indexes, table_exists};
use tork_orm_core::Database;

#[tokio::test]
async fn existing_columns_returns_columns_in_order() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL, score REAL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    let cols = existing_columns(&db, "items").await.unwrap();
    assert_eq!(cols.len(), 3);

    assert_eq!(cols[0].name, "id");
    assert!(cols[0].is_pk);

    assert_eq!(cols[1].name, "label");
    assert_eq!(cols[1].declared_type.to_uppercase(), "TEXT");
    assert!(cols[1].not_null);
    assert!(!cols[1].is_pk);

    assert_eq!(cols[2].name, "score");
    assert_eq!(cols[2].declared_type.to_uppercase(), "REAL");
    assert!(!cols[2].not_null);
    assert!(!cols[2].is_pk);
}

#[tokio::test]
async fn existing_columns_empty_for_nonexistent_table() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    let cols = existing_columns(&db, "does_not_exist").await.unwrap();
    assert!(cols.is_empty());
}

#[tokio::test]
async fn existing_columns_captures_varchar_type() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE t (name VARCHAR(100) NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();

    let cols = existing_columns(&db, "t").await.unwrap();
    assert_eq!(cols.len(), 1);
    assert_eq!(cols[0].declared_type.to_uppercase(), "VARCHAR(100)");
    assert!(cols[0].not_null);
}

#[tokio::test]
async fn reads_tables_and_created_indexes() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, \
         slug TEXT NOT NULL UNIQUE)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "CREATE INDEX \"posts_user_id_idx\" ON \"posts\" (\"user_id\")".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "CREATE UNIQUE INDEX \"posts_slug_user_key\" ON \"posts\" (\"slug\", \"user_id\")".into(),
        vec![],
    )
    .await
    .unwrap();

    assert!(table_exists(&db, "posts").await.unwrap());
    assert!(!table_exists(&db, "comments").await.unwrap());

    let mut indexes = existing_indexes(&db, "posts").await.unwrap();
    indexes.sort_by(|a, b| a.name.cmp(&b.name));
    // Only the two CREATE INDEX statements; the UNIQUE-constraint auto-index on
    // slug is excluded (origin != 'c').
    assert_eq!(indexes.len(), 2);
    assert_eq!(indexes[0].name, "posts_slug_user_key");
    assert!(indexes[0].unique);
    assert_eq!(indexes[1].name, "posts_user_id_idx");
    assert!(!indexes[1].unique);
}
