//! Tests for reading the live schema (table existence and created indexes).

#![cfg(feature = "migrations")]

use tork_orm_core::migration::introspect::{existing_indexes, table_exists};
use tork_orm_core::Database;

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
