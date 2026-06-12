//! Tests for the QuerySet builder and its terminals, run against a real in-memory
//! SQLite database.

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

/// Creates the schema and seeds a fixed set of users.
async fn seed() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL, email TEXT NOT NULL, is_active INTEGER NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    for (username, email, active) in [
        ("alice", "alice@example.com", true),
        ("bob", "bob@example.com", false),
        ("carol", "carol@example.com", true),
        ("dave", "dave@example.com", true),
    ] {
        db.execute(
            "INSERT INTO users (username, email, is_active) VALUES (?, ?, ?)".into(),
            vec![
                Value::Text(username.into()),
                Value::Text(email.into()),
                Value::Bool(active),
            ],
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn all_returns_every_row() {
    let db = seed().await;
    let users = User::query().all(&db).await.unwrap();
    assert_eq!(users.len(), 4);
    assert_eq!(users[0].username, "alice");
}

#[tokio::test]
async fn filter_applies_an_and_predicate() {
    let db = seed().await;
    let active = User::query()
        .filter(User::is_active.eq(true))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 3);
    assert!(active.iter().all(|u| u.is_active));
}

#[tokio::test]
async fn filter_any_is_an_or_group() {
    let db = seed().await;
    let users = User::query()
        .filter_any([
            User::username.eq("alice"),
            User::username.eq("bob"),
        ])
        .all(&db)
        .await
        .unwrap();
    let mut names: Vec<&str> = users.iter().map(|u| u.username.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["alice", "bob"]);
}

#[tokio::test]
async fn combined_and_or_filters() {
    let db = seed().await;
    // is_active = true AND (username = alice OR username = bob)
    let users = User::query()
        .filter(User::is_active.eq(true))
        .filter_any([
            User::username.eq("alice"),
            User::username.eq("bob"),
        ])
        .all(&db)
        .await
        .unwrap();
    // Only alice is both active and named alice/bob (bob is inactive).
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}

#[tokio::test]
async fn filter_not_negates() {
    let db = seed().await;
    let users = User::query()
        .filter_not(User::is_active.eq(true))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "bob");
}

#[tokio::test]
async fn order_by_and_limit() {
    let db = seed().await;
    let users = User::query()
        .order_by(User::id.desc())
        .limit(2)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].username, "dave");
    assert_eq!(users[1].username, "carol");
}

#[tokio::test]
async fn offset_skips_rows() {
    let db = seed().await;
    let users = User::query()
        .order_by(User::id.asc())
        .limit(2)
        .offset(1)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users[0].username, "bob");
    assert_eq!(users[1].username, "carol");
}

#[tokio::test]
async fn first_returns_one_or_none() {
    let db = seed().await;
    let found = User::query()
        .filter(User::username.eq("carol"))
        .first(&db)
        .await
        .unwrap();
    assert_eq!(found.unwrap().username, "carol");

    let missing = User::query()
        .filter(User::username.eq("nobody"))
        .first(&db)
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn one_requires_exactly_one_row() {
    let db = seed().await;

    let single = User::query()
        .filter(User::username.eq("alice"))
        .one(&db)
        .await
        .unwrap();
    assert_eq!(single.username, "alice");

    let none = User::query()
        .filter(User::username.eq("nobody"))
        .one(&db)
        .await
        .unwrap_err();
    assert_eq!(none.kind(), ErrorKind::NotFound);

    let many = User::query()
        .filter(User::is_active.eq(true))
        .one(&db)
        .await
        .unwrap_err();
    assert_eq!(many.kind(), ErrorKind::MultipleFound);
}

#[tokio::test]
async fn count_and_exists() {
    let db = seed().await;

    let total = User::query().count(&db).await.unwrap();
    assert_eq!(total, 4);

    let active = User::query()
        .filter(User::is_active.eq(true))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(active, 3);

    assert!(User::query()
        .filter(User::username.eq("alice"))
        .exists(&db)
        .await
        .unwrap());
    assert!(!User::query()
        .filter(User::username.eq("nobody"))
        .exists(&db)
        .await
        .unwrap());
}

// ── LIKE / ILIKE ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn like_matches_prefix_pattern() {
    let db = seed().await;
    // Seeded: alice, bob, carol, dave — only "alice" matches "ali%".
    let users = User::query()
        .filter(User::username.like("ali%"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}

#[tokio::test]
async fn like_no_match_returns_empty() {
    let db = seed().await;
    let users = User::query()
        .filter(User::username.like("xyz%"))
        .all(&db)
        .await
        .unwrap();
    assert!(users.is_empty());
}

#[tokio::test]
async fn ilike_is_case_insensitive() {
    let db = seed().await;
    // "ALICE" upper-cased should still match the stored "alice".
    let users = User::query()
        .filter(User::username.ilike("ALICE"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "alice");
}

#[tokio::test]
async fn ilike_substring_match() {
    let db = seed().await;
    // "%OL%" should match "carol".
    let users = User::query()
        .filter(User::username.ilike("%OL%"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "carol");
}

// ── BETWEEN ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn between_returns_rows_in_range() {
    let db = seed().await;
    // Seeded ids: 1 (alice), 2 (bob), 3 (carol), 4 (dave).
    let users = User::query()
        .filter(User::id.between(2_i64, 3_i64))
        .order_by(User::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].username, "bob");
    assert_eq!(users[1].username, "carol");
}

#[tokio::test]
async fn between_is_inclusive_on_both_ends() {
    let db = seed().await;
    // Rows at both boundary values (id=1 and id=4) must be included.
    let users = User::query()
        .filter(User::id.between(1_i64, 4_i64))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(users, 4);
}

#[tokio::test]
async fn between_returns_empty_for_inverted_range() {
    let db = seed().await;
    // low > high — no rows satisfy the predicate.
    let users = User::query()
        .filter(User::id.between(10_i64, 1_i64))
        .all(&db)
        .await
        .unwrap();
    assert!(users.is_empty());
}

// ── NOT IN ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn not_in_excludes_listed_values() {
    let db = seed().await;
    let users = User::query()
        .filter(User::username.not_in(["alice", "bob"]))
        .order_by(User::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].username, "carol");
    assert_eq!(users[1].username, "dave");
}

#[tokio::test]
async fn not_in_empty_list_matches_all_rows() {
    let db = seed().await;
    let empty: [&str; 0] = [];
    let users = User::query()
        .filter(User::username.not_in(empty))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(users, 4);
}

// ── RAW SQL ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn filter_raw_returns_matching_rows() {
    let db = seed().await;
    // Seeded: alice (1), bob (2), carol (3), dave (4).
    // SQLite LENGTH() on username — should return users whose username is longer than 4 chars.
    // "alice" = 5, "carol" = 5, "dave" = 4, "bob" = 3.
    let users = User::query()
        .filter_raw("LENGTH(username) > ?", [4_i64])
        .order_by(User::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].username, "alice");
    assert_eq!(users[1].username, "carol");
}

#[tokio::test]
async fn filter_raw_no_params_works() {
    let db = seed().await;
    // Constant predicate that is always true (1 = 1) — all rows returned.
    let users = User::query()
        .filter_raw("1 = 1", [] as [i64; 0])
        .count(&db)
        .await
        .unwrap();
    assert_eq!(users, 4);
}

// ── EXISTS / NOT EXISTS ───────────────────────────────────────────────────────

#[tokio::test]
async fn exists_subquery_returns_all_when_match_found() {
    let db = seed().await;
    // EXISTS (SELECT ... WHERE is_active = true) — the seed has active users,
    // so the EXISTS is true and all outer rows are returned.
    let users = User::query()
        .filter(Expr::exists(
            User::query().filter(User::is_active.eq(true)),
        ))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(users, 4);
}

#[tokio::test]
async fn not_exists_returns_empty_when_subquery_has_rows() {
    let db = seed().await;
    // NOT EXISTS (...) where the subquery matches — predicate is false for every
    // outer row, so the result set is empty.
    let users = User::query()
        .filter(Expr::not_exists(
            User::query().filter(User::is_active.eq(true)),
        ))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

// ── SUBQUERIES ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn in_subquery_filters_rows_at_runtime() {
    let db = seed().await;
    // Seeded: alice (active), bob (inactive), carol (active), dave (active).
    // The subquery selects the ids of all active users; the outer query fetches
    // users whose id appears in that set — so bob should be excluded.
    let users = User::query()
        .filter(User::id.in_subquery(
            User::query()
                .filter(User::is_active.eq(true))
                .select((User::id,)),
        ))
        .order_by(User::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 3);
    assert!(users.iter().all(|u| u.is_active));
    assert!(users.iter().all(|u| u.username != "bob"));
}

#[tokio::test]
async fn not_in_subquery_filters_rows_at_runtime() {
    let db = seed().await;
    // Excludes rows whose id is in the inactive-user subquery — so only bob should be excluded.
    let users = User::query()
        .filter(User::id.not_in_subquery(
            User::query()
                .filter(User::is_active.eq(false))
                .select((User::id,)),
        ))
        .order_by(User::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(users.len(), 3);
    assert!(users.iter().all(|u| u.username != "bob"));
}

// ── NONE ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn none_returns_zero_rows() {
    let db = seed().await;
    let users = User::query().none().all(&db).await.unwrap();
    assert!(users.is_empty());
}

#[tokio::test]
async fn none_works_with_filters() {
    let db = seed().await;
    // Adding a filter after none() should still yield zero rows because 0=1
    // is AND-ed with the filter.
    let users = User::query()
        .none()
        .filter(User::is_active.eq(true))
        .all(&db)
        .await
        .unwrap();
    assert!(users.is_empty());
}

// ── FOR UPDATE ────────────────────────────────────────────────────────────────

#[test]
fn for_update_appears_in_rendered_sql() {
    let stmt = User::query()
        .filter(User::is_active.eq(true))
        .for_update()
        .into_statement();
    assert!(stmt.lock.is_some());

    let dialect = tork_orm_core::dialect::SqliteDialect::new();
    let (sql, _) = tork_orm_core::dialect::render_select(&dialect, &stmt);
    assert!(
        sql.ends_with(" FOR UPDATE"),
        "expected SQL to end with ' FOR UPDATE', got: {sql}"
    );
}

#[test]
fn for_update_after_limit_offset() {
    let stmt = User::query()
        .for_update()
        .order_by(User::id.asc())
        .limit(2)
        .offset(1)
        .into_statement();

    let dialect = tork_orm_core::dialect::SqliteDialect::new();
    let (sql, _) = tork_orm_core::dialect::render_select(&dialect, &stmt);
    assert!(
        sql.contains("LIMIT 2 OFFSET 1 FOR UPDATE"),
        "expected 'LIMIT 2 OFFSET 1 FOR UPDATE', got: {sql}"
    );
}
