//! Throughput / connection-pool stress on a file-backed SQLite database (WAL):
//! many concurrent tasks write through a bounded pool. All succeed, the pool
//! releases connections (no leak, no deadlock), and the final count is exact.
//!
//! Override the volume with `ECOM_STRESS_OPS` (e.g. 200000 for a load run).

use ecommerce::models::*;
use ecommerce::testkit::*;
use tork_orm::prelude::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_concurrent_writes_through_the_pool() {
    let ops: usize = std::env::var("ECOM_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);

    // A file (not :memory:) so the pool can hold more than one connection and WAL
    // allows concurrent access.
    let path = std::env::temp_dir().join(format!("ecom_stress_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let url = format!("sqlite://{}", path.display());

    let db = migrated(&url, 8).await.unwrap();

    let mut handles = Vec::with_capacity(ops);
    for i in 0..ops {
        let d = db.clone();
        handles.push(tokio::spawn(async move {
            User::create(&d, &user(&format!("user{i}@x.com"))).await.map(|_| ())
        }));
    }
    let mut ok = 0usize;
    for h in handles {
        h.await.unwrap().unwrap();
        ok += 1;
    }
    assert_eq!(ok, ops);
    assert_eq!(User::query().count(&db).await.unwrap() as usize, ops);

    // The pool still works after the burst (connections were released, not leaked).
    let probe = User::query().filter(User::email.eq("user0@x.com")).count(&db).await.unwrap();
    assert_eq!(probe, 1);

    db.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}
