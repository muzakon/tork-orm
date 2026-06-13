//! Transactions: a multi-table order placement commits atomically, rolls back
//! fully on error, and nests savepoints. Against in-memory SQLite.

use ecommerce::models::*;
use ecommerce::testkit::*;
use tork_orm::prelude::*;
use tork_orm::ErrorKind;

async fn db() -> Database {
    migrated(":memory:", 1).await.unwrap()
}

#[tokio::test]
async fn commit_persists_a_multi_table_order() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();

    db.transaction(|tx| {
        Box::pin(async move {
            let o = Order::create(tx, &order(s.user_id, "ORD-1", 3998)).await?;
            OrderItem::create(
                tx,
                &order_item(o.id, s.vendor_id, s.product_id, s.variant_id, 2, 1999),
            )
            .await?;
            Payment::create(tx, &payment(o.id, 3998)).await?;
            Ok(())
        })
    })
    .await
    .unwrap();

    assert_eq!(Order::query().count(&db).await.unwrap(), 1);
    assert_eq!(OrderItem::query().count(&db).await.unwrap(), 1);
    assert_eq!(Payment::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn rollback_discards_every_table_on_error() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();

    let result: Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                let o = Order::create(tx, &order(s.user_id, "ORD-2", 1999)).await?;
                OrderItem::create(
                    tx,
                    &order_item(o.id, s.vendor_id, s.product_id, s.variant_id, 1, 1999),
                )
                .await?;
                // Something goes wrong after the writes; the whole transaction unwinds.
                Err(OrmError::query("payment gateway timed out"))
            })
        })
        .await;

    assert!(result.is_err());
    assert_eq!(Order::query().count(&db).await.unwrap(), 0);
    assert_eq!(OrderItem::query().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn savepoint_rolls_back_inner_work_only() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();

    db.transaction(|tx| {
        Box::pin(async move {
            let o = Order::create(tx, &order(s.user_id, "ORD-3", 1999)).await?;

            // A nested savepoint that fails: its writes are undone, the outer order
            // survives because we swallow the savepoint error.
            let inner: Result<()> = tx
                .savepoint(|sp| {
                    Box::pin(async move {
                        OrderItem::create(
                            sp,
                            &order_item(o.id, s.vendor_id, s.product_id, s.variant_id, 1, 1999),
                        )
                        .await?;
                        Err(OrmError::query("inner step failed"))
                    })
                })
                .await;
            assert!(inner.is_err());

            Ok(())
        })
    })
    .await
    .unwrap();

    // The order committed; the savepoint's order item was rolled back.
    assert_eq!(Order::query().count(&db).await.unwrap(), 1);
    assert_eq!(OrderItem::query().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn unique_violation_inside_a_transaction_unwinds_it() {
    let db = db().await;
    seed(&db, 100).await.unwrap();

    // The seed already created buyer@x.com; a second insert of the same email must
    // fail and take the whole transaction down with it.
    let result: Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                User::create(tx, &user("fresh@x.com")).await?;
                User::create(tx, &user("buyer@x.com")).await?; // duplicate email
                Ok(())
            })
        })
        .await;

    assert!(result.is_err());
    // Neither the fresh user nor anything else from the transaction persisted.
    assert_eq!(User::query().filter(User::email.eq("fresh@x.com")).count(&db).await.unwrap(), 0);
    let _ = ErrorKind::Conflict; // documents the error family we expect from constraints
}
