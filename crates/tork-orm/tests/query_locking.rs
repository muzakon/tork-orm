//! Tests for the Phase 1 query additions: `DISTINCT ON`, row-locking modifiers
//! (`FOR UPDATE`/`FOR SHARE`/`SKIP LOCKED`/`NOWAIT`/`OF`), and keyset (seek)
//! pagination. SQL is asserted by rendering the built statement; behaviour and
//! dialect gating are checked against in-memory SQLite.

use tork_orm::dialect::{render_select, Dialect, MySqlDialect, PostgresDialect, SqliteDialect};
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "items")]
struct Item {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    category: String,
    price: i64,
}

/// Renders a query set's statement to SQL under a dialect.
fn sql_of(query: QuerySet<Item>, dialect: &dyn Dialect) -> String {
    render_select(dialect, &query.into_statement()).0
}

// ── DISTINCT ON ──────────────────────────────────────────────────────────────

#[test]
fn distinct_on_renders_on_postgres() {
    let sql = sql_of(
        Item::query()
            .distinct_on((Item::category,))
            .order_by(Item::category.asc())
            .order_by(Item::price.desc()),
        &PostgresDialect::new(),
    );
    assert!(
        sql.starts_with("SELECT DISTINCT ON (\"items\".\"category\") "),
        "unexpected DISTINCT ON SQL: {sql}"
    );
}

// ── Row locking ──────────────────────────────────────────────────────────────

#[test]
fn for_update_renders_everywhere() {
    assert!(sql_of(Item::query().for_update(), &SqliteDialect::new()).ends_with(" FOR UPDATE"));
    assert!(sql_of(Item::query().for_update(), &PostgresDialect::new()).ends_with(" FOR UPDATE"));
    assert!(sql_of(Item::query().for_update(), &MySqlDialect::new()).ends_with(" FOR UPDATE"));
}

#[test]
fn lock_modifiers_render_on_postgres() {
    let pg = PostgresDialect::new();
    assert!(sql_of(Item::query().for_share(), &pg).ends_with(" FOR SHARE"));
    assert!(sql_of(Item::query().for_update().skip_locked(), &pg).ends_with(" FOR UPDATE SKIP LOCKED"));
    assert!(sql_of(Item::query().for_update().nowait(), &pg).ends_with(" FOR UPDATE NOWAIT"));
    assert!(
        sql_of(Item::query().for_update().lock_of(&[Item::TABLE]), &pg)
            .ends_with(" FOR UPDATE OF \"items\"")
    );
    // A modifier on its own implies FOR UPDATE.
    assert!(sql_of(Item::query().skip_locked(), &pg).ends_with(" FOR UPDATE SKIP LOCKED"));
}

#[test]
fn lock_modifiers_render_on_mysql() {
    let my = MySqlDialect::new();
    assert!(sql_of(Item::query().for_update().skip_locked(), &my).ends_with(" FOR UPDATE SKIP LOCKED"));
    assert!(sql_of(Item::query().for_share(), &my).ends_with(" FOR SHARE"));
}

// ── Keyset (seek) pagination ─────────────────────────────────────────────────

#[test]
fn keyset_after_single_key_renders_simple_comparison() {
    let sql = sql_of(
        Item::query()
            .order_by(Item::id.asc())
            .keyset_after(vec![Value::Int(5)]),
        &SqliteDialect::new(),
    );
    assert!(sql.contains("WHERE \"items\".\"id\" > ?"), "unexpected keyset SQL: {sql}");
}

#[test]
fn keyset_after_composite_key_renders_or_chain() {
    let sql = sql_of(
        Item::query()
            .order_by(Item::category.asc())
            .order_by(Item::id.asc())
            .keyset_after(vec![Value::Text("books".into()), Value::Int(5)]),
        &SqliteDialect::new(),
    );
    assert!(
        sql.contains(
            "WHERE (\"items\".\"category\" > ? OR \
             (\"items\".\"category\" = ? AND \"items\".\"id\" > ?))"
        ),
        "unexpected composite keyset SQL: {sql}"
    );
}

#[test]
fn keyset_before_flips_the_comparison() {
    let sql = sql_of(
        Item::query()
            .order_by(Item::id.asc())
            .keyset_before(vec![Value::Int(5)]),
        &SqliteDialect::new(),
    );
    assert!(sql.contains("WHERE \"items\".\"id\" < ?"), "unexpected keyset SQL: {sql}");
}

#[test]
fn keyset_descending_key_uses_less_than_for_after() {
    let sql = sql_of(
        Item::query()
            .order_by(Item::id.desc())
            .keyset_after(vec![Value::Int(5)]),
        &SqliteDialect::new(),
    );
    assert!(sql.contains("WHERE \"items\".\"id\" < ?"), "unexpected keyset SQL: {sql}");
}

#[test]
#[should_panic(expected = "match the number of `order_by` terms")]
fn keyset_cursor_length_must_match_order_terms() {
    let _ = Item::query()
        .order_by(Item::id.asc())
        .keyset_after(vec![Value::Int(1), Value::Int(2)]);
}

// ── Live behaviour + dialect gating against SQLite ───────────────────────────

async fn item_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, category TEXT NOT NULL, price INTEGER NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    for i in 1..=5 {
        db.execute(
            "INSERT INTO items (category, price) VALUES ('a', ?)".into(),
            vec![Value::Int(i * 10)],
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn keyset_pagination_walks_pages_in_order() {
    let db = item_db().await;

    let page1 = Item::query().order_by(Item::id.asc()).limit(2).all(&db).await.unwrap();
    assert_eq!(page1.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1, 2]);

    let cursor = vec![page1.last().unwrap().id.to_value()];
    let page2 = Item::query()
        .order_by(Item::id.asc())
        .keyset_after(cursor)
        .limit(2)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(page2.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 4]);

    let cursor = vec![page2.last().unwrap().id.to_value()];
    let page3 = Item::query()
        .order_by(Item::id.asc())
        .keyset_after(cursor)
        .limit(2)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(page3.iter().map(|i| i.id).collect::<Vec<_>>(), vec![5]);
}

#[tokio::test]
async fn distinct_on_is_rejected_on_sqlite() {
    let db = item_db().await;
    let result = Item::query().distinct_on((Item::category,)).all(&db).await;
    assert!(result.is_err(), "DISTINCT ON should be rejected on SQLite");
}

#[tokio::test]
async fn lock_modifiers_are_rejected_on_sqlite() {
    let db = item_db().await;
    let result = Item::query().for_update().skip_locked().all(&db).await;
    assert!(result.is_err(), "SKIP LOCKED should be rejected on SQLite");
}
