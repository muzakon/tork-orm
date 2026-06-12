//! Tests for QuerySet::union / union_all, run against in-memory SQLite.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    username: String,
    is_active: bool,
}

async fn seed() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL, is_active INTEGER NOT NULL)".into(),
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
async fn union_deduplicates_rows() {
    let db = seed().await;
    // Both branches return alice; UNION should deduplicate.
    let rows = User::query()
        .filter(User::username.eq("alice"))
        .union(User::query().filter(User::username.eq("alice")))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].username, "alice");
}

#[tokio::test]
async fn union_all_preserves_duplicates() {
    let db = seed().await;
    // Same alice row from both branches — union_all keeps both copies.
    let rows = User::query()
        .filter(User::username.eq("alice"))
        .union_all(User::query().filter(User::username.eq("alice")))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|u| u.username == "alice"));
}

#[tokio::test]
async fn union_combines_disjoint_sets() {
    let db = seed().await;
    // First branch: alice. Second branch: bob. No overlap — either union mode gives 2 rows.
    let rows = User::query()
        .filter(User::username.eq("alice"))
        .union(User::query().filter(User::username.eq("bob")))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let mut names: Vec<&str> = rows.iter().map(|u| u.username.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["alice", "bob"]);
}

#[tokio::test]
async fn union_with_order_by_and_limit() {
    let db = seed().await;
    // Three branches (one per user) combined, ordered by id DESC, limited to 2.
    let rows = User::query()
        .filter(User::username.eq("alice"))
        .union(User::query().filter(User::username.eq("bob")))
        .union(User::query().filter(User::username.eq("carol")))
        .order_by(User::id.desc())
        .limit(2)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].username, "carol");
    assert_eq!(rows[1].username, "bob");
}

#[tokio::test]
async fn union_count_wraps_combined_result() {
    let db = seed().await;
    // Active users + inactive users = all 3, deduplicated = 3.
    let total = User::query()
        .filter(User::is_active.eq(true))
        .union(User::query().filter(User::is_active.eq(false)))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(total, 3);
}

#[tokio::test]
async fn union_first_returns_one_row() {
    let db = seed().await;
    let row = User::query()
        .filter(User::username.eq("alice"))
        .union(User::query().filter(User::username.eq("bob")))
        .order_by(User::id.asc())
        .first(&db)
        .await
        .unwrap();
    assert_eq!(row.unwrap().username, "alice");
}
