//! Tests for QuerySet::paginate and QuerySet::paginate_as
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
    for (username, active) in [
        ("alice", true),
        ("bob", false),
        ("carol", true),
        ("dave", false),
        ("eve", true),
    ] {
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
async fn paginate_first_page() {
    let db = seed().await;
    let page = User::query()
        .order_by(User::id.asc())
        .paginate(&db, 1, 2)
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].username, "alice");
    assert_eq!(page.items[1].username, "bob");
    assert_eq!(page.total, 5);
    assert_eq!(page.page, 1);
    assert_eq!(page.page_size, 2);
    assert_eq!(page.pages, 3);
}

#[tokio::test]
async fn paginate_middle_page() {
    let db = seed().await;
    let page = User::query()
        .order_by(User::id.asc())
        .paginate(&db, 2, 2)
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].username, "carol");
    assert_eq!(page.items[1].username, "dave");
    assert_eq!(page.page, 2);
    assert_eq!(page.pages, 3);
}

#[tokio::test]
async fn paginate_last_page_partial() {
    let db = seed().await;
    let page = User::query()
        .order_by(User::id.asc())
        .paginate(&db, 3, 2)
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].username, "eve");
    assert_eq!(page.page, 3);
    assert_eq!(page.pages, 3);
}

#[tokio::test]
async fn paginate_page_beyond_end_clamps_to_last() {
    let db = seed().await;
    let page = User::query()
        .order_by(User::id.asc())
        .paginate(&db, 99, 2)
        .await
        .unwrap();

    // Clamped to page 3 (the last page).
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].username, "eve");
    assert_eq!(page.page, 3);
}

#[tokio::test]
async fn paginate_empty_result() {
    let db = seed().await;
    let page = User::query()
        .filter(User::username.eq("nobody"))
        .paginate(&db, 1, 10)
        .await
        .unwrap();

    assert!(page.items.is_empty());
    assert_eq!(page.total, 0);
    assert_eq!(page.page, 1);
    assert_eq!(page.pages, 1);
}

#[tokio::test]
async fn paginate_respects_filter() {
    let db = seed().await;
    // Only active users: alice, carol, eve (3 total).
    let page = User::query()
        .filter(User::is_active.eq(true))
        .order_by(User::id.asc())
        .paginate(&db, 1, 2)
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].username, "alice");
    assert_eq!(page.items[1].username, "carol");
    assert_eq!(page.total, 3);
    assert_eq!(page.pages, 2);
}

#[tokio::test]
async fn paginate_as_custom_dto() {
    let db = seed().await;

    #[derive(QueryResult)]
    struct IdOnly {
        id: i64,
    }

    let page = User::query()
        .select((User::id.as_("id"),))
        .order_by(User::id.asc())
        .paginate_as::<IdOnly, _>(&db, 1, 2)
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].id, 1);
    assert_eq!(page.items[1].id, 2);
    assert_eq!(page.total, 5);
    assert_eq!(page.pages, 3);
}

#[tokio::test]
async fn paginate_zero_or_one_page_size_defaults_to_one() {
    let db = seed().await;
    let page = User::query()
        .order_by(User::id.asc())
        .paginate(&db, 1, 0)
        .await
        .unwrap();

    // page_size of 0 is clamped to 1.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].username, "alice");
    assert_eq!(page.page_size, 1);
}
