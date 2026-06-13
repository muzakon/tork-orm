//! Relations and performance: preloading is N+1-free (one extra query per
//! relation), and indexed lookups use their index (verified with
//! `EXPLAIN QUERY PLAN`).

use ecommerce::models::*;
use ecommerce::testkit::*;
use tork_orm::prelude::*;
use tork_orm::Value;

async fn db() -> Database {
    migrated(":memory:", 1).await.unwrap()
}

#[tokio::test]
async fn preload_is_n_plus_one_free() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();
    for i in 0..6 {
        Order::create(&db, &order(s.user_id, &format!("O-{i}"), 100)).await.unwrap();
    }

    // Count only the statements the preload itself runs: one for the parents, one
    // for the related orders, regardless of how many orders there are.
    let before = db.statement_count();
    let users = User::query().preload(User::orders()).all(&db).await.unwrap();
    let ran = db.statement_count() - before;
    assert_eq!(ran, 2, "preload should add exactly one query per relation, not one per row");

    let total_orders: usize = users.iter().map(|u| u.get::<Order>().len()).sum();
    assert_eq!(total_orders, 6);
}

#[tokio::test]
async fn indexed_lookup_uses_its_index() {
    let db = db().await;
    seed(&db, 100).await.unwrap();

    // users.email carries a unique index; SQLite should plan a SEARCH USING INDEX
    // rather than a full table SCAN.
    let rows = db
        .fetch_all(
            "EXPLAIN QUERY PLAN SELECT id FROM users WHERE email = ?".into(),
            vec![Value::Text("buyer@x.com".into())],
        )
        .await
        .unwrap();
    let plan = rows
        .iter()
        .filter_map(|r| r.get::<String>("detail").ok())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        plan.contains("USING") && plan.contains("INDEX"),
        "expected an index search, got plan: {plan}"
    );
}

#[tokio::test]
async fn foreign_key_lookup_uses_its_index() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();
    let o = Order::create(&db, &order(s.user_id, "O-1", 100)).await.unwrap();
    OrderItem::create(&db, &order_item(o.id, s.vendor_id, s.product_id, s.variant_id, 1, 100))
        .await
        .unwrap();

    // order_items.order_id is auto-indexed from its foreign key.
    let rows = db
        .fetch_all(
            "EXPLAIN QUERY PLAN SELECT id FROM order_items WHERE order_id = ?".into(),
            vec![Value::Int(o.id)],
        )
        .await
        .unwrap();
    let plan = rows
        .iter()
        .filter_map(|r| r.get::<String>("detail").ok())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(plan.contains("USING") && plan.contains("INDEX"), "expected an index search, got plan: {plan}");
}
