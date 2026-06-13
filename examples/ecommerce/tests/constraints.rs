//! Constraint enforcement against in-memory SQLite. The driver enables
//! `PRAGMA foreign_keys = ON`, so foreign keys, unique indexes, and CHECK
//! constraints all reject bad data at the database.

use ecommerce::models::*;
use ecommerce::testkit::*;
use tork_orm::prelude::*;
use tork_orm::Value;

async fn db() -> Database {
    migrated(":memory:", 1).await.unwrap()
}

#[tokio::test]
async fn foreign_key_violation_is_rejected() {
    let db = db().await;
    // No user with id 999999 exists; the FK on addresses.user_id must reject it.
    let result = Address::create(&db, &address(999_999)).await;
    assert!(result.is_err(), "FK to a missing user should be rejected");
}

#[tokio::test]
async fn restrict_blocks_deleting_a_referenced_row() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();
    let o = Order::create(&db, &order(s.user_id, "ORD-R", 1999)).await.unwrap();
    OrderItem::create(&db, &order_item(o.id, s.vendor_id, s.product_id, s.variant_id, 1, 1999))
        .await
        .unwrap();

    // order_items.vendor_id uses ON DELETE RESTRICT, so removing the vendor fails
    // while an order item still references it.
    let result = db
        .execute("DELETE FROM vendors WHERE id = ?".into(), vec![Value::Int(s.vendor_id)])
        .await;
    assert!(result.is_err(), "RESTRICT should block deleting a referenced vendor");
}

#[tokio::test]
async fn unique_index_rejects_duplicates() {
    let db = db().await;
    User::create(&db, &user("dup@x.com")).await.unwrap();
    let result = User::create(&db, &user("dup@x.com")).await;
    assert!(result.is_err(), "duplicate email should be rejected by the unique index");
}

#[tokio::test]
async fn check_constraints_reject_bad_values() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();
    let c = Cart::create(&db, &cart(s.user_id)).await.unwrap();

    // quantity > 0
    assert!(
        CartItem::create(&db, &cart_item(c.id, s.variant_id, 0)).await.is_err(),
        "cart_items CHECK (quantity > 0) should reject 0"
    );
    // rating BETWEEN 1 AND 5
    assert!(
        Review::create(&db, &review(s.user_id, s.product_id, 6)).await.is_err(),
        "reviews CHECK should reject rating 6"
    );
    assert!(
        Review::create(&db, &review(s.user_id, s.product_id, 0)).await.is_err(),
        "reviews CHECK should reject rating 0"
    );
    // price_cents >= 0
    assert!(
        ProductVariant::create(&db, &variant(s.product_id, "NEG", -1)).await.is_err(),
        "product_variants CHECK (price_cents >= 0) should reject a negative price"
    );

    // A valid review still succeeds.
    assert!(Review::create(&db, &review(s.user_id, s.product_id, 5)).await.is_ok());
}

#[tokio::test]
async fn enum_check_rejects_unknown_value() {
    let db = db().await;
    // Bypass the type system with raw SQL: the enum CHECK on users.role must reject
    // a value outside the declared set.
    let result = db
        .execute(
            "INSERT INTO users (email, password_hash, role, status) \
             VALUES ('x@x.com', 'h', 'superadmin', 'active')"
                .into(),
            vec![],
        )
        .await;
    assert!(result.is_err(), "enum CHECK should reject an unknown role");
}
