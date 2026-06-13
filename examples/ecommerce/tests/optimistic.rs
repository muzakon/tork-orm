//! Optimistic locking: the `version` column on inventory items detects a
//! concurrent stock update and rejects the stale writer.

use ecommerce::models::*;
use ecommerce::testkit::*;
use tork_orm::prelude::*;
use tork_orm::ErrorKind;

async fn db() -> Database {
    migrated(":memory:", 1).await.unwrap()
}

#[tokio::test]
async fn concurrent_stock_update_conflicts() {
    let db = db().await;
    let s = seed(&db, 100).await.unwrap();

    // Two workers load the same inventory row (both at version 1).
    let mut a = InventoryItem::find(&db, s.inventory_id).await.unwrap();
    let mut b = InventoryItem::find(&db, s.inventory_id).await.unwrap();

    // Worker A reserves 10 units and wins.
    a.quantity_on_hand -= 10;
    a.save(&db).await.unwrap();
    assert_eq!(a.version, 2);

    // Worker B is stale (still version 1) and is rejected, so it cannot silently
    // overwrite A's stock count.
    b.quantity_on_hand -= 5;
    let err = b.save(&db).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Conflict);

    // The database holds A's write only.
    let fresh = InventoryItem::find(&db, s.inventory_id).await.unwrap();
    assert_eq!(fresh.quantity_on_hand, 90);
    assert_eq!(fresh.version, 2);
}
