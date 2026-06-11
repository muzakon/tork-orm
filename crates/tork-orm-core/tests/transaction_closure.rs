//! Tests for `Database::transaction()` — the closure-based API.

use tork_orm_core::{Database, Executor, OrmError};

async fn setup() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

async fn count(db: &Database) -> i64 {
    let rows = db
        .fetch_all("SELECT COUNT(*) as c FROM items".into(), vec![])
        .await
        .unwrap();
    rows[0].get::<i64>("c").unwrap()
}

#[tokio::test]
async fn ok_result_commits() {
    let db = setup().await;
    let inserted = db
        .transaction(|tx| {
            Box::pin(async move {
                tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
                    .await?;
                tx.execute("INSERT INTO items VALUES (2, 20)".into(), vec![])
                    .await?;
                Ok(2_usize)
            })
        })
        .await
        .unwrap();
    assert_eq!(inserted, 2);
    assert_eq!(count(&db).await, 2);
}

#[tokio::test]
async fn err_result_rolls_back() {
    let db = setup().await;
    let result = db
        .transaction(|tx| {
            Box::pin(async move {
                tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
                    .await?;
                // Fail the transaction.
                Err::<(), _>(OrmError::query("intentional"))
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(count(&db).await, 0);
}

#[tokio::test]
async fn closure_returns_a_value() {
    let db = setup().await;
    db.execute("INSERT INTO items VALUES (7, 99)".into(), vec![])
        .await
        .unwrap();
    let n = db
        .transaction(|tx| {
            Box::pin(async move {
                let rows = tx
                    .fetch_all("SELECT n FROM items WHERE id = 7".into(), vec![])
                    .await?;
                Ok(rows[0].get::<i64>("n").unwrap())
            })
        })
        .await
        .unwrap();
    assert_eq!(n, 99);
}
