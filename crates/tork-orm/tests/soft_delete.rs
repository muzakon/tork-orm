//! Tests for soft-delete: the `#[field(deleted_at)]` column, the default
//! `deleted_at IS NULL` query scope, `with_deleted`/`only_deleted`, and the
//! soft/`force`/`restore` delete operations. Run against in-memory SQLite.

use time::OffsetDateTime;
use tork_orm::dialect::{render_select, SqliteDialect};
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "notes")]
struct Note {
    #[field(primary_key, auto)]
    id: i64,
    body: String,
    #[field(deleted_at)]
    deleted_at: Option<OffsetDateTime>,
}

async fn note_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL, deleted_at TEXT)".into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

fn note(body: &str) -> Note {
    Note { id: 0, body: body.into(), deleted_at: None }
}

#[test]
fn metadata_and_default_scope_render() {
    assert_eq!(<Note as Model>::DELETED_AT, Some("deleted_at"));

    // The default scope is baked into the statement.
    let (active, _) = render_select(&SqliteDialect::new(), &Note::query().into_statement());
    assert!(
        active.contains("WHERE \"notes\".\"deleted_at\" IS NULL"),
        "default scope missing: {active}"
    );

    // with_deleted drops it (no WHERE clause at all).
    let (all, _) = render_select(&SqliteDialect::new(), &Note::query().with_deleted().into_statement());
    assert!(!all.contains("WHERE"), "with_deleted should drop the scope: {all}");

    // only_deleted flips it.
    let (deleted, _) =
        render_select(&SqliteDialect::new(), &Note::query().only_deleted().into_statement());
    assert!(
        deleted.contains("WHERE \"notes\".\"deleted_at\" IS NOT NULL"),
        "only_deleted scope wrong: {deleted}"
    );
}

#[tokio::test]
async fn default_scope_hides_soft_deleted_rows() {
    let db = note_db().await;
    let a = Note::create(&db, &note("a")).await.unwrap();
    assert!(a.deleted_at.is_none());
    Note::create(&db, &note("b")).await.unwrap();
    Note::create(&db, &note("c")).await.unwrap();

    // Soft-delete one row.
    a.delete(&db).await.unwrap();

    // The default scope hides it; with_deleted/only_deleted reveal it.
    assert_eq!(Note::query().count(&db).await.unwrap(), 2);
    assert_eq!(Note::query().with_deleted().count(&db).await.unwrap(), 3);
    assert_eq!(Note::query().only_deleted().count(&db).await.unwrap(), 1);

    // find() respects the default scope.
    assert_eq!(Note::find(&db, a.id).await.unwrap_err().kind(), ErrorKind::NotFound);

    // The soft-deleted row carries a deleted_at timestamp.
    let revealed = Note::query().with_deleted().filter(Note::id.eq(a.id)).one(&db).await.unwrap();
    assert!(revealed.deleted_at.is_some());
}

#[tokio::test]
async fn instance_restore_and_force_delete() {
    let db = note_db().await;
    let a = Note::create(&db, &note("a")).await.unwrap();

    // Soft delete, then restore via an instance loaded from only_deleted().
    a.delete(&db).await.unwrap();
    assert_eq!(Note::query().count(&db).await.unwrap(), 0);
    let deleted = Note::query().only_deleted().one(&db).await.unwrap();
    deleted.restore(&db).await.unwrap();
    assert_eq!(Note::query().count(&db).await.unwrap(), 1);

    // force_delete removes the row for good.
    let active = Note::query().one(&db).await.unwrap();
    active.force_delete(&db).await.unwrap();
    assert_eq!(Note::query().with_deleted().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn bulk_soft_delete_hard_delete_and_restore() {
    let db = note_db().await;
    for body in ["a", "b", "c"] {
        Note::create(&db, &note(body)).await.unwrap();
    }

    // Bulk delete soft-deletes the active rows.
    let soft = Note::query().delete(&db).await.unwrap();
    assert_eq!(soft, 3);
    assert_eq!(Note::query().count(&db).await.unwrap(), 0);
    assert_eq!(Note::query().with_deleted().count(&db).await.unwrap(), 3);

    // Bulk restore brings them back.
    let restored = Note::query().only_deleted().restore(&db).await.unwrap();
    assert_eq!(restored, 3);
    assert_eq!(Note::query().count(&db).await.unwrap(), 3);

    // Bulk hard_delete removes them physically (bypassing soft-delete).
    let removed = Note::query().with_deleted().hard_delete(&db).await.unwrap();
    assert_eq!(removed, 3);
    assert_eq!(Note::query().with_deleted().count(&db).await.unwrap(), 0);
}
