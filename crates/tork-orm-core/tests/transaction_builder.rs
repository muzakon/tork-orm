//! Tests for `TransactionBuilder` and SQLite isolation levels.

use tork_orm_core::{Database, Executor, IsolationLevel};

async fn setup() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, v INTEGER NOT NULL)".into(),
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
async fn deferred_commits() {
    let db = setup().await;
    {
        let mut tx = db.transaction_with().deferred().begin().await.unwrap();
        tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(count(&db).await, 1);
}

#[tokio::test]
async fn immediate_commits() {
    let db = setup().await;
    {
        let mut tx = db.transaction_with().immediate().begin().await.unwrap();
        tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(count(&db).await, 1);
}

#[tokio::test]
async fn exclusive_commits() {
    let db = setup().await;
    {
        let mut tx = db.transaction_with().exclusive().begin().await.unwrap();
        tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(count(&db).await, 1);
}

#[tokio::test]
async fn builder_run_commits_on_ok() {
    let db = setup().await;
    db.transaction_with()
        .immediate()
        .run(|tx| {
            Box::pin(async move {
                tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(count(&db).await, 1);
}

#[tokio::test]
async fn builder_run_rolls_back_on_err() {
    let db = setup().await;
    let result = db
        .transaction_with()
        .deferred()
        .run(|tx| {
            Box::pin(async move {
                tx.execute("INSERT INTO items VALUES (1, 10)".into(), vec![])
                    .await?;
                Err::<(), _>(tork_orm_core::OrmError::query("boom"))
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(count(&db).await, 0);
}

#[tokio::test]
async fn isolation_level_default_is_deferred() {
    assert_eq!(IsolationLevel::default(), IsolationLevel::Deferred);
}
