//! Tests for the write operations: create (with RETURNING), bulk_create, save,
//! and bulk update/delete, against in-memory SQLite.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    username: String,
    email: String,
    is_active: bool,
}

#[derive(Debug, Clone, Model)]
#[table(name = "counters")]
struct Counter {
    #[field(primary_key, auto)]
    id: i64,
    hits: i64,
}

async fn empty_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL UNIQUE, email TEXT NOT NULL, is_active INTEGER NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

async fn counter_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE counters (id INTEGER PRIMARY KEY, hits INTEGER NOT NULL DEFAULT 0)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

fn new_user(username: &str) -> User {
    User {
        id: 0,
        username: username.into(),
        email: format!("{username}@example.com"),
        is_active: true,
    }
}

#[tokio::test]
async fn create_returns_the_persisted_row_with_generated_id() {
    let db = empty_db().await;
    let stored = User::create(&db, &new_user("alice")).await.unwrap();
    // The database assigned the id even though the input had id 0.
    assert_eq!(stored.id, 1);
    assert_eq!(stored.username, "alice");

    let second = User::create(&db, &new_user("bob")).await.unwrap();
    assert_eq!(second.id, 2);
}

#[tokio::test]
async fn bulk_create_inserts_many() {
    let db = empty_db().await;
    let inserted = User::bulk_create(
        &db,
        &[new_user("alice"), new_user("bob"), new_user("carol")],
    )
    .await
    .unwrap();
    assert_eq!(inserted, 3);
    assert_eq!(User::query().count(&db).await.unwrap(), 3);
}

#[tokio::test]
async fn bulk_create_of_nothing_is_a_noop() {
    let db = empty_db().await;
    let inserted = User::bulk_create(&db, &[]).await.unwrap();
    assert_eq!(inserted, 0);
}

#[tokio::test]
async fn save_writes_back_current_field_values() {
    let db = empty_db().await;
    let mut stored = User::create(&db, &new_user("alice")).await.unwrap();

    stored.email = "alice@new.example.com".into();
    stored.is_active = false;
    let changed = stored.save(&db).await.unwrap();
    assert_eq!(changed, 1);

    let reloaded = User::query()
        .filter(User::id.eq(stored.id))
        .one(&db)
        .await
        .unwrap();
    assert_eq!(reloaded.email, "alice@new.example.com");
    assert!(!reloaded.is_active);
    // The auto primary key is untouched by save.
    assert_eq!(reloaded.id, stored.id);
}

#[tokio::test]
async fn query_update_sets_columns_on_matching_rows() {
    let db = empty_db().await;
    User::bulk_create(
        &db,
        &[new_user("alice"), new_user("bob"), new_user("carol")],
    )
    .await
    .unwrap();

    let changed = User::query()
        .filter(User::username.ne("bob"))
        .update(&db, [User::is_active.set(false)])
        .await
        .unwrap();
    assert_eq!(changed, 2);

    let active = User::query()
        .filter(User::is_active.eq(true))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].username, "bob");
}

#[tokio::test]
async fn query_delete_removes_matching_rows() {
    let db = empty_db().await;
    User::bulk_create(
        &db,
        &[new_user("alice"), new_user("bob"), new_user("carol")],
    )
    .await
    .unwrap();

    let removed = User::query()
        .filter(User::username.eq("bob"))
        .delete(&db)
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(User::query().count(&db).await.unwrap(), 2);

    let all_removed = User::query().delete(&db).await.unwrap();
    assert_eq!(all_removed, 2);
    assert_eq!(User::query().count(&db).await.unwrap(), 0);
}

// ── instance delete ───────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_instance_removes_exactly_one_row() {
    let db = empty_db().await;
    User::bulk_create(
        &db,
        &[new_user("alice"), new_user("bob"), new_user("carol")],
    )
    .await
    .unwrap();

    let bob = User::query()
        .filter(User::username.eq("bob"))
        .one(&db)
        .await
        .unwrap();

    let removed = bob.delete(&db).await.unwrap();
    assert_eq!(removed, 1);
    assert_eq!(User::query().count(&db).await.unwrap(), 2);
    assert!(!User::query()
        .filter(User::username.eq("bob"))
        .exists(&db)
        .await
        .unwrap());
}

#[tokio::test]
async fn delete_nonexistent_instance_returns_zero() {
    let db = empty_db().await;
    let ghost = User { id: 999, username: "ghost".into(), email: "g@example.com".into(), is_active: false };
    let removed = ghost.delete(&db).await.unwrap();
    assert_eq!(removed, 0);
}

// ── expression UPDATE ─────────────────────────────────────────────────────────

#[tokio::test]
async fn update_set_with_expr_increments_atomically() {
    let db = counter_db().await;
    let c = Counter::create(&db, &Counter { id: 0, hits: 10 }).await.unwrap();

    let changed = Counter::query()
        .filter(Counter::id.eq(c.id))
        .update(&db, [Counter::hits.set(Counter::hits.add(5_i64))])
        .await
        .unwrap();
    assert_eq!(changed, 1);

    let reloaded = Counter::query().filter(Counter::id.eq(c.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.hits, 15);
}

#[tokio::test]
async fn update_set_with_literal_still_binds_param() {
    let db = counter_db().await;
    let c = Counter::create(&db, &Counter { id: 0, hits: 0 }).await.unwrap();

    let changed = Counter::query()
        .filter(Counter::id.eq(c.id))
        .update(&db, [Counter::hits.set(42_i64)])
        .await
        .unwrap();
    assert_eq!(changed, 1);

    let reloaded = Counter::query().filter(Counter::id.eq(c.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.hits, 42);
}

// ── upsert ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn upsert_inserts_when_no_conflict() {
    let db = empty_db().await;
    // alice does not exist; upsert should insert and return the new row.
    let stored = User::upsert(&db, &new_user("alice")).await.unwrap();
    assert_eq!(stored.username, "alice");
    assert!(stored.id > 0);
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn upsert_replaces_conflicting_row_on_unique_key() {
    let db = empty_db().await;
    // Insert alice first (auto-id assigned).
    let _ = User::create(&db, &new_user("alice")).await.unwrap();

    // Upsert the same username (UNIQUE constraint) with a different email.
    // INSERT OR REPLACE deletes the conflicting row and inserts the new one.
    let replaced = User::upsert(
        &db,
        &User { id: 0, username: "alice".into(), email: "alice2@example.com".into(), is_active: false },
    )
    .await
    .unwrap();
    assert_eq!(replaced.username, "alice");
    assert_eq!(replaced.email, "alice2@example.com");
    assert!(!replaced.is_active);

    // Still exactly one alice row in the table.
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

// ── update_returning ──────────────────────────────────────────────────────────

#[tokio::test]
async fn update_returning_gives_back_changed_rows() {
    let db = empty_db().await;
    User::bulk_create(&db, &[new_user("alice"), new_user("bob")]).await.unwrap();

    let updated = User::query()
        .filter(User::username.eq("alice"))
        .update_returning(&db, [User::is_active.set(false)])
        .await
        .unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].username, "alice");
    assert!(!updated[0].is_active);
}

#[tokio::test]
async fn update_returning_empty_when_no_rows_match() {
    let db = empty_db().await;
    let updated = User::query()
        .filter(User::username.eq("nobody"))
        .update_returning(&db, [User::is_active.set(false)])
        .await
        .unwrap();
    assert!(updated.is_empty());
}

// ── delete_returning ──────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_returning_gives_back_removed_rows() {
    let db = empty_db().await;
    User::bulk_create(&db, &[new_user("alice"), new_user("bob"), new_user("carol")])
        .await
        .unwrap();

    let removed = User::query()
        .filter(User::username.eq("bob"))
        .delete_returning(&db)
        .await
        .unwrap();

    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].username, "bob");
    assert_eq!(User::query().count(&db).await.unwrap(), 2);
}

#[tokio::test]
async fn delete_returning_all_rows_clears_table() {
    let db = empty_db().await;
    User::bulk_create(&db, &[new_user("alice"), new_user("bob")]).await.unwrap();

    let removed = User::query().delete_returning(&db).await.unwrap();
    assert_eq!(removed.len(), 2);
    let mut names: Vec<&str> = removed.iter().map(|u| u.username.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["alice", "bob"]);
    assert_eq!(User::query().count(&db).await.unwrap(), 0);
}

// ── get_or_create ────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_or_create_creates_when_not_found() {
    let db = empty_db().await;
    let (user, created) = User::get_or_create(
        &db,
        |q| q.filter(User::username.eq("alice")),
        &new_user("alice"),
    )
    .await
    .unwrap();

    assert!(created);
    assert_eq!(user.username, "alice");
    assert!(user.is_active);
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn get_or_create_returns_existing() {
    let db = empty_db().await;
    let _ = User::create(&db, &new_user("alice")).await.unwrap();

    let (user, created) = User::get_or_create(
        &db,
        |q| q.filter(User::username.eq("alice")),
        &new_user("alice"),
    )
    .await
    .unwrap();

    assert!(!created);
    assert_eq!(user.username, "alice");
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn get_or_create_errors_on_multiple_matches() {
    let db = empty_db().await;
    User::bulk_create(&db, &[new_user("alice"), User {
        id: 0,
        username: "alice2".into(),
        email: "alice2@example.com".into(),
        is_active: true,
    }])
    .await
    .unwrap();

    let err = User::get_or_create(
        &db,
        |q| q.filter_raw("username LIKE ?", ["alice%"]),
        &new_user("bob"),
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MultipleFound);
}

// ── update_or_create ─────────────────────────────────────────────────────────

#[tokio::test]
async fn update_or_create_creates_when_not_found() {
    let db = empty_db().await;
    let (user, created) = User::update_or_create(
        &db,
        |q| q.filter(User::username.eq("alice")),
        &new_user("alice"),
    )
    .await
    .unwrap();

    assert!(created);
    assert_eq!(user.username, "alice");
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn update_or_create_updates_existing() {
    let db = empty_db().await;
    let _ = User::create(&db, &new_user("alice")).await.unwrap();

    let updated_value = User {
        id: 0,
        username: "alice".into(),
        email: "alice-new@example.com".into(),
        is_active: false,
    };
    let (user, created) = User::update_or_create(
        &db,
        |q| q.filter(User::username.eq("alice")),
        &updated_value,
    )
    .await
    .unwrap();

    assert!(!created);
    assert_eq!(user.email, "alice-new@example.com");
    assert!(!user.is_active);
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

// ── first_or_create ──────────────────────────────────────────────────────────

#[tokio::test]
async fn first_or_create_creates_when_not_found() {
    let db = empty_db().await;
    let user = User::first_or_create(
        &db,
        |q| q.filter(User::username.eq("alice")),
        &new_user("alice"),
    )
    .await
    .unwrap();

    assert_eq!(user.username, "alice");
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn first_or_create_returns_first_if_multiple_match() {
    let db = empty_db().await;
    User::bulk_create(
        &db,
        &[new_user("alice"), User {
            id: 0,
            username: "alice2".into(),
            email: "alice2@example.com".into(),
            is_active: true,
        }],
    )
    .await
    .unwrap();

    let user = User::first_or_create(
        &db,
        |q| q.filter_raw("username LIKE ?", ["alice%"]),
        &new_user("bob"),
    )
    .await
    .unwrap();
    assert_eq!(user.username, "alice");
    assert_eq!(User::query().count(&db).await.unwrap(), 2);
}
