//! Tests for model-lifecycle columns: auto `created_at`/`updated_at` timestamps
//! and optimistic-lock `version`. Run against in-memory SQLite.

use time::OffsetDateTime;
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "posts")]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    title: String,
    #[field(created_at)]
    created_at: OffsetDateTime,
    #[field(updated_at)]
    updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "docs")]
struct Doc {
    #[field(primary_key, auto)]
    id: i64,
    body: String,
    #[field(version)]
    version: i64,
}

async fn post_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE posts (\
            id INTEGER PRIMARY KEY, \
            title TEXT NOT NULL, \
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, \
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

async fn doc_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT NOT NULL, version BIGINT NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

fn new_post(title: &str) -> Post {
    Post {
        id: 0,
        title: title.into(),
        // Placeholders; both columns are database-filled on insert.
        created_at: OffsetDateTime::UNIX_EPOCH,
        updated_at: OffsetDateTime::UNIX_EPOCH,
    }
}

#[test]
fn lifecycle_metadata_is_recorded() {
    assert_eq!(<Post as Model>::UPDATED_AT, Some("updated_at"));
    assert_eq!(<Post as Model>::VERSION, None);
    assert_eq!(<Doc as Model>::VERSION, Some("version"));
    assert_eq!(<Doc as Model>::UPDATED_AT, None);
}

#[tokio::test]
async fn create_fills_timestamps_from_the_database() {
    let db = post_db().await;
    let post = Post::create(&db, &new_post("hello")).await.unwrap();
    // The database default filled both columns with the current time.
    assert!(post.created_at.year() >= 2026, "created_at not set: {:?}", post.created_at);
    // Both defaults evaluate to the same value within one statement.
    assert_eq!(post.created_at, post.updated_at);
}

#[tokio::test]
async fn save_touches_updated_at_but_not_created_at() {
    let db = post_db().await;
    // Seed a row whose timestamps are far in the past.
    db.execute(
        "INSERT INTO posts (title, created_at, updated_at) \
         VALUES ('old', '2020-01-01 00:00:00', '2020-01-01 00:00:00')"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    let mut post = Post::query().filter(Post::title.eq("old")).one(&db).await.unwrap();
    let original_created = post.created_at;
    let original_updated = post.updated_at;
    assert_eq!(original_updated.year(), 2020);

    post.title = "renamed".into();
    post.save(&db).await.unwrap();

    let reloaded = Post::find(&db, post.id).await.unwrap();
    assert_eq!(reloaded.title, "renamed");
    // updated_at moved to the database's current time.
    assert!(
        reloaded.updated_at > original_updated,
        "updated_at not touched: {:?}",
        reloaded.updated_at
    );
    // created_at is left untouched.
    assert_eq!(reloaded.created_at, original_created);
}

#[tokio::test]
async fn optimistic_lock_bumps_version_and_detects_conflict() {
    let db = doc_db().await;
    let doc = Doc::create(&db, &Doc { id: 0, body: "v1".into(), version: 1 }).await.unwrap();
    assert_eq!(doc.version, 1);

    // Two copies loaded "concurrently", both at version 1.
    let mut a = Doc::find(&db, doc.id).await.unwrap();
    let mut b = Doc::find(&db, doc.id).await.unwrap();

    // First writer wins and bumps the version (in the DB and in memory).
    a.body = "from-a".into();
    a.save(&db).await.unwrap();
    assert_eq!(a.version, 2);
    assert_eq!(Doc::find(&db, doc.id).await.unwrap().version, 2);

    // Second writer is stale (still version 1) -> conflict, DB unchanged.
    b.body = "from-b".into();
    let err = b.save(&db).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Conflict);
    let stored = Doc::find(&db, doc.id).await.unwrap();
    assert_eq!(stored.body, "from-a");
    assert_eq!(stored.version, 2);

    // The winner's in-memory version was bumped, so it can save again.
    a.body = "from-a-2".into();
    a.save(&db).await.unwrap();
    assert_eq!(a.version, 3);
    assert_eq!(Doc::find(&db, doc.id).await.unwrap().version, 3);
}
