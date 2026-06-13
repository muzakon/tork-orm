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
