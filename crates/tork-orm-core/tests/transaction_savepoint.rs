//! Tests for nested savepoints within a transaction.

use tork_orm_core::{Database, Executor, OrmError};

async fn setup() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE vals (id INTEGER PRIMARY KEY, v INTEGER NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

async fn count(db: &Database) -> i64 {
    let rows = db
        .fetch_all("SELECT COUNT(*) as c FROM vals".into(), vec![])
        .await
        .unwrap();
    rows[0].get::<i64>("c").unwrap()
}

async fn ids(db: &Database) -> Vec<i64> {
    let rows = db
        .fetch_all("SELECT id FROM vals ORDER BY id".into(), vec![])
        .await
        .unwrap();
    rows.iter().map(|r| r.get::<i64>("id").unwrap()).collect()
}

#[tokio::test]
async fn savepoint_ok_commits_inner_work() {
    let db = setup().await;
    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO vals VALUES (1, 100)".into(), vec![])
                .await?;
            tx.savepoint(|sp| {
                Box::pin(async move {
                    sp.execute("INSERT INTO vals VALUES (2, 200)".into(), vec![])
                        .await?;
                    Ok(())
                })
            })
            .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(ids(&db).await, vec![1, 2]);
}

#[tokio::test]
async fn savepoint_err_rolls_back_only_inner_work() {
    let db = setup().await;
    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO vals VALUES (1, 100)".into(), vec![])
                .await?;
            // Inner savepoint fails — only its work is lost.
            let inner = tx
                .savepoint(|sp| {
                    Box::pin(async move {
                        sp.execute("INSERT INTO vals VALUES (2, 200)".into(), vec![])
                            .await?;
                        Err::<(), _>(OrmError::query("inner fail"))
                    })
                })
                .await;
            assert!(inner.is_err());
            // Outer transaction continues; only row 1 persists.
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(ids(&db).await, vec![1]);
}

#[tokio::test]
async fn nested_savepoints_independent_rollback() {
    let db = setup().await;
    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO vals VALUES (1, 10)".into(), vec![])
                .await?;
            // First savepoint succeeds.
            tx.savepoint(|sp| {
                Box::pin(async move {
                    sp.execute("INSERT INTO vals VALUES (2, 20)".into(), vec![])
                        .await?;
                    Ok(())
                })
            })
            .await?;
            // Second savepoint fails.
            let _ = tx
                .savepoint(|sp| {
                    Box::pin(async move {
                        sp.execute("INSERT INTO vals VALUES (3, 30)".into(), vec![])
                            .await?;
                        Err::<(), _>(OrmError::query("sp2 fail"))
                    })
                })
                .await;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(ids(&db).await, vec![1, 2]);
}

#[tokio::test]
async fn savepoint_counter_increments_per_call() {
    let db = setup().await;
    db.transaction(|tx| {
        Box::pin(async move {
            // Both savepoints must be named differently.
            for i in 0_i64..5 {
                tx.savepoint(|sp| {
                    Box::pin(async move {
                        sp.execute(
                            format!("INSERT INTO vals VALUES ({i}, {i})"),
                            vec![],
                        )
                        .await?;
                        Ok(())
                    })
                })
                .await?;
            }
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(count(&db).await, 5);
}
