//! Row-locking and concurrency against a real PostgreSQL server.
//!
//! ```text
//! docker compose up -d
//! cargo test -p ecommerce --features postgres --test concurrency_pg
//! ```
//!
//! These cover what SQLite cannot: real `FOR UPDATE` / `SKIP LOCKED` semantics,
//! lock release on commit, and many concurrent transactions contending on one row
//! through a bounded connection pool (the autoscaling / high-concurrency case).
#![cfg(feature = "postgres")]

use std::sync::Arc;

use tork_orm::prelude::*;
use tork_orm::Value;

#[derive(Debug, Clone, Model)]
#[table(name = "stock_test")]
struct Stock {
    #[field(primary_key)]
    id: i64,
    qty: i64,
    state: String,
}

/// Serializes the tests, which all reset and share the `stock_test` table.
static TABLE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tork:tork@localhost:55432/tork_test".to_string())
}

async fn fresh(pool: u32) -> Database {
    let db = Database::connect(&url(), pool).await.expect("connect to PostgreSQL");
    db.execute("DROP TABLE IF EXISTS stock_test".into(), vec![]).await.unwrap();
    db.execute(
        "CREATE TABLE stock_test (id BIGINT PRIMARY KEY, qty BIGINT NOT NULL, state TEXT NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

#[tokio::test]
async fn skip_locked_skips_a_row_held_by_another_transaction() {
    let _guard = TABLE_LOCK.lock().await;
    let db = fresh(8).await;
    db.execute("INSERT INTO stock_test VALUES (1, 100, 'pending')".into(), vec![])
        .await
        .unwrap();

    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    // Worker A locks row 1 and holds the lock across the barrier.
    let a_db = db.clone();
    let a_barrier = barrier.clone();
    let a = tokio::spawn(async move {
        a_db.transaction(|tx| {
            Box::pin(async move {
                let locked = Stock::query().filter(Stock::id.eq(1)).for_update().all(tx).await?;
                assert_eq!(locked.len(), 1);
                a_barrier.wait().await; // tell B the row is locked
                a_barrier.wait().await; // wait until B has tried
                Ok(())
            })
        })
        .await
        .unwrap();
    });

    barrier.wait().await; // row is now locked by A
    // Worker B with SKIP LOCKED must not see the locked row.
    let skipped = Stock::query()
        .filter(Stock::id.eq(1))
        .for_update()
        .skip_locked()
        .all(&db)
        .await
        .unwrap();
    assert!(skipped.is_empty(), "SKIP LOCKED must skip the row A holds");
    barrier.wait().await; // let A commit and release the lock
    a.await.unwrap();

    // After A commits, the lock is released and the row is lockable again.
    let after = Stock::query().filter(Stock::id.eq(1)).for_update().all(&db).await.unwrap();
    assert_eq!(after.len(), 1, "the lock must be released on commit");
}

#[tokio::test]
async fn two_workers_claim_distinct_rows_with_skip_locked() {
    let _guard = TABLE_LOCK.lock().await;
    let db = fresh(8).await;
    for id in 1..=4 {
        db.execute(
            "INSERT INTO stock_test VALUES ($1, 0, 'pending')".into(),
            vec![Value::Int(id)],
        )
        .await
        .unwrap();
    }

    // Each worker grabs one pending row with FOR UPDATE SKIP LOCKED inside a tx and
    // marks it done. Run concurrently; they must claim different rows (the job-queue
    // pattern), never the same one twice.
    let claim = |db: Database| async move {
        db.transaction(|tx| {
            Box::pin(async move {
                let row = Stock::query()
                    .filter(Stock::state.eq("pending"))
                    .order_by(Stock::id.asc())
                    .limit(1)
                    .for_update()
                    .skip_locked()
                    .first(tx)
                    .await?;
                if let Some(row) = row {
                    tx.execute(
                        "UPDATE stock_test SET state = 'done' WHERE id = $1".into(),
                        vec![Value::Int(row.id)],
                    )
                    .await?;
                    return Ok(Some(row.id));
                }
                Ok(None)
            })
        })
        .await
    };

    let (a, b) = tokio::join!(claim(db.clone()), claim(db.clone()));
    let a = a.unwrap();
    let b = b.unwrap();
    assert!(a.is_some() && b.is_some(), "both workers should claim a row");
    assert_ne!(a, b, "the two workers must claim different rows");
}

#[tokio::test]
async fn many_concurrent_decrements_serialize_with_no_lost_updates() {
    let _guard = TABLE_LOCK.lock().await;
    // The high-concurrency / autoscaling case: many tasks contend on one row through
    // a small pool. Each takes FOR UPDATE, decrements, and commits (releasing the
    // lock for the next). The total must be exact — no lost updates, no deadlock.
    // Override the volume with ECOM_STRESS_OPS (e.g. 100000 for a load test).
    let ops: i64 = std::env::var("ECOM_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);

    let db = fresh(16).await;
    db.execute(
        "INSERT INTO stock_test VALUES (1, $1, 'x')".into(),
        vec![Value::Int(ops)],
    )
    .await
    .unwrap();

    let mut handles = Vec::with_capacity(ops as usize);
    for _ in 0..ops {
        let d = db.clone();
        handles.push(tokio::spawn(async move {
            d.transaction(|tx| {
                Box::pin(async move {
                    // Lock the row, then decrement it.
                    let _row = Stock::query().filter(Stock::id.eq(1)).for_update().one(tx).await?;
                    tx.execute("UPDATE stock_test SET qty = qty - 1 WHERE id = 1".into(), vec![])
                        .await?;
                    Ok(())
                })
            })
            .await
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }

    let remaining = Stock::query().filter(Stock::id.eq(1)).one(&db).await.unwrap().qty;
    assert_eq!(remaining, 0, "every one of {ops} decrements must apply exactly once");
}
