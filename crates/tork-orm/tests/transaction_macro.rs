//! Smoke-tests the `transaction!` macro from the facade.

use tork_orm::prelude::*;
use tork_orm::transaction;

#[tokio::test]
async fn macro_commits_on_ok() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await.unwrap();

    let count = transaction!(db, |tx| async move {
        tx.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
        tx.execute("INSERT INTO t VALUES (2)".into(), vec![]).await?;
        Ok(2_usize)
    })
    .await
    .unwrap();

    assert_eq!(count, 2);
    let rows = db
        .fetch_all("SELECT COUNT(*) as c FROM t".into(), vec![])
        .await
        .unwrap();
    assert_eq!(rows[0].get::<i64>("c").unwrap(), 2);
}

#[tokio::test]
async fn macro_rolls_back_on_err() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await.unwrap();

    let result = transaction!(db, |tx| async move {
        tx.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
        Err::<(), _>(OrmError::query("simulated failure"))
    })
    .await;

    assert!(result.is_err());
    let rows = db
        .fetch_all("SELECT COUNT(*) as c FROM t".into(), vec![])
        .await
        .unwrap();
    assert_eq!(rows[0].get::<i64>("c").unwrap(), 0);
}
