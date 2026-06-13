//! Basic transaction tests: commit, rollback, and auto-rollback on drop.

use tork_orm_core::{Database, Executor};

async fn setup() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE counters (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO counters VALUES (1, 0)".into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

async fn read_n(db: &Database) -> i64 {
    let rows = db
        .fetch_all("SELECT n FROM counters WHERE id = 1".into(), vec![])
        .await
        .unwrap();
    rows[0].get::<i64>("n").unwrap()
}

#[tokio::test]
async fn commit_persists() {
    let db = setup().await;
    // Transaction must be dropped before using `db` again: the pinned connection
    // holds the pool's single semaphore permit for its entire lifetime.
    {
        let mut tx = db.begin().await.unwrap();
        tx.execute(
            "UPDATE counters SET n = 42 WHERE id = 1".into(),
            vec![],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(read_n(&db).await, 42);
}

#[tokio::test]
async fn rollback_discards() {
    let db = setup().await;
    {
        let mut tx = db.begin().await.unwrap();
        tx.execute(
            "UPDATE counters SET n = 99 WHERE id = 1".into(),
            vec![],
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();
    }
    assert_eq!(read_n(&db).await, 0);
}

#[tokio::test]
async fn drop_without_commit_rolls_back() {
    let db = setup().await;
    {
        let tx = db.begin().await.unwrap();
        tx.execute(
            "UPDATE counters SET n = 77 WHERE id = 1".into(),
            vec![],
        )
        .await
        .unwrap();
        // tx drops here without commit
    }
    assert_eq!(read_n(&db).await, 0);
}

#[tokio::test]
async fn query_inside_transaction_is_visible_to_itself() {
    let db = setup().await;
    {
        let mut tx = db.begin().await.unwrap();
        tx.execute(
            "UPDATE counters SET n = 55 WHERE id = 1".into(),
            vec![],
        )
        .await
        .unwrap();
        // Reads inside the same transaction see the update.
        let rows = tx
            .fetch_all("SELECT n FROM counters WHERE id = 1".into(), vec![])
            .await
            .unwrap();
        let n = rows[0].get::<i64>("n").unwrap();
        assert_eq!(n, 55);
        tx.rollback().await.unwrap();
    }
    // After rollback the original value is restored.
    assert_eq!(read_n(&db).await, 0);
}

#[tokio::test]
async fn concurrent_queries_on_a_transaction_serialize() {
    let db = setup().await;
    let mut tx = db.begin().await.unwrap();

    // Two queries issued concurrently on the same transaction must serialize on
    // the single pinned connection rather than the second failing with
    // "pinned connection is already in use".
    let (a, b) = tokio::join!(
        tx.fetch_all("SELECT n FROM counters WHERE id = 1".into(), vec![]),
        tx.fetch_all("SELECT n FROM counters WHERE id = 1".into(), vec![]),
    );
    assert!(a.is_ok(), "first concurrent query failed: {a:?}");
    assert!(b.is_ok(), "second concurrent query failed: {b:?}");

    tx.commit().await.unwrap();
}

#[tokio::test]
async fn serializable_isolation_runs_on_sqlite() {
    let db = setup().await;
    // SQLite maps the standard level onto a plain BEGIN; the builder method works.
    db.transaction_with()
        .serializable()
        .run(|tx| {
            Box::pin(async move {
                tx.execute("UPDATE counters SET n = 5 WHERE id = 1".into(), vec![])
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(read_n(&db).await, 5);
}

#[tokio::test]
async fn transaction_retry_recovers_from_a_transient_conflict() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use tork_orm_core::OrmError;

    let db = setup().await;
    let attempts = AtomicU32::new(0);

    // The first attempt fails with a lock-style (retryable) error; the second
    // succeeds, so the helper retries and commits.
    let result: tork_orm_core::Result<i64> = db
        .transaction_retry(5, |tx| {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    return Err(OrmError::query("database is locked"));
                }
                tx.execute("UPDATE counters SET n = 7 WHERE id = 1".into(), vec![])
                    .await?;
                Ok(7)
            })
        })
        .await;

    assert_eq!(result.unwrap(), 7);
    assert_eq!(attempts.load(Ordering::SeqCst), 2, "retried once after the conflict");
    assert_eq!(read_n(&db).await, 7);
}

#[tokio::test]
async fn transaction_retry_gives_up_on_a_non_retryable_error() {
    use tork_orm_core::OrmError;

    let db = setup().await;
    let result: tork_orm_core::Result<()> = db
        .transaction_retry(5, |_tx| {
            Box::pin(async move { Err(OrmError::query("syntax error near FROM")) })
        })
        .await;
    assert!(result.is_err());
}
