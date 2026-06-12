//! Live integration tests against a real PostgreSQL server.
//!
//! Bring the database up first:
//!
//! ```text
//! docker compose up -d
//! cargo test -p tork-orm --features postgres --test postgres_live
//! ```
//!
//! The connection URL defaults to the `docker-compose.yml` service; override it with
//! `TEST_DATABASE_URL`. Each test uses its own table, so they are isolated and run in
//! parallel.
#![cfg(feature = "postgres")]

use time::macros::datetime;
use time::OffsetDateTime;
use tork_orm::prelude::*;

/// The connection URL for the test database.
fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tork:tork@localhost:55432/tork_test".to_string())
}

/// Connects to the test database with a small pool.
async fn connect() -> Database {
    Database::connect(&database_url(), 5)
        .await
        .expect("connect to the test PostgreSQL (is `docker compose up -d` running?)")
}

/// Drops and recreates a table so each test starts from a known state.
async fn reset(db: &Database, drop_sql: &str, create_sql: &str) {
    db.execute(drop_sql.into(), vec![]).await.unwrap();
    db.execute(create_sql.into(), vec![]).await.unwrap();
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_accounts")]
struct Account {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    name: String,
    balance: i64,
    active: bool,
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_typebag")]
struct TypeBag {
    #[field(primary_key, auto)]
    id: i64,
    flag: bool,
    small: i32,
    big: i64,
    ratio: f64,
    label: String,
    payload: Vec<u8>,
    recorded_at: OffsetDateTime,
}

#[tokio::test]
async fn connects_and_runs_basic_sql() {
    let db = connect().await;
    let rows = db.fetch_all("SELECT 1 AS one".into(), vec![]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i64>("one").unwrap(), 1);
}

#[tokio::test]
async fn create_returns_generated_id_then_crud() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_accounts",
        "CREATE TABLE pg_accounts (\
            id BIGSERIAL PRIMARY KEY, \
            name VARCHAR(50) NOT NULL, \
            balance BIGINT NOT NULL, \
            active BOOLEAN NOT NULL)",
    )
    .await;

    // create() uses RETURNING to fetch the BIGSERIAL id.
    let alice = Account::create(
        &db,
        &Account { id: 0, name: "alice".into(), balance: 100, active: true },
    )
    .await
    .unwrap();
    assert!(alice.id > 0);
    assert_eq!(alice.name, "alice");

    Account::create(&db, &Account { id: 0, name: "bob".into(), balance: 50, active: false })
        .await
        .unwrap();

    // Filter ($N placeholders), order, limit.
    let active = Account::query()
        .filter(Account::active.eq(true))
        .order_by(Account::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "alice");

    // Update and re-read.
    let changed = Account::query()
        .filter(Account::name.eq("bob"))
        .update(&db, [Account::active.set(true)])
        .await
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(Account::query().filter(Account::active.eq(true)).count(&db).await.unwrap(), 2);

    // Delete.
    let removed = Account::query().filter(Account::name.eq("alice")).delete(&db).await.unwrap();
    assert_eq!(removed, 1);
    assert_eq!(Account::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn all_value_types_round_trip() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_typebag",
        "CREATE TABLE pg_typebag (\
            id BIGSERIAL PRIMARY KEY, \
            flag BOOLEAN NOT NULL, \
            small INTEGER NOT NULL, \
            big BIGINT NOT NULL, \
            ratio DOUBLE PRECISION NOT NULL, \
            label TEXT NOT NULL, \
            payload BYTEA NOT NULL, \
            recorded_at TIMESTAMPTZ NOT NULL)",
    )
    .await;

    let original = TypeBag {
        id: 0,
        flag: true,
        // `small` is an INTEGER (INT4) column; the ORM binds Value::Int(i64) and the
        // driver narrows it to i32, exercising the adaptive ToSql path.
        small: -12345,
        big: 9_000_000_000,
        ratio: 2.5,
        label: "héllo".into(),
        payload: vec![0, 1, 2, 250, 255],
        recorded_at: datetime!(2026-06-12 14:30:05 UTC),
    };

    let stored = TypeBag::create(&db, &original).await.unwrap();
    assert!(stored.id > 0);

    let reloaded = TypeBag::query()
        .filter(TypeBag::id.eq(stored.id))
        .one(&db)
        .await
        .unwrap();

    assert_eq!(reloaded.flag, true);
    assert_eq!(reloaded.small, -12345);
    assert_eq!(reloaded.big, 9_000_000_000);
    assert_eq!(reloaded.ratio, 2.5);
    assert_eq!(reloaded.label, "héllo");
    assert_eq!(reloaded.payload, vec![0, 1, 2, 250, 255]);
    assert_eq!(reloaded.recorded_at, datetime!(2026-06-12 14:30:05 UTC));
}

#[tokio::test]
async fn update_and_delete_returning() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_returning",
        "CREATE TABLE pg_returning (\
            id BIGSERIAL PRIMARY KEY, \
            name VARCHAR(50) NOT NULL, \
            balance BIGINT NOT NULL, \
            active BOOLEAN NOT NULL)",
    )
    .await;

    for (name, balance) in [("a", 1_i64), ("b", 2), ("c", 3)] {
        db.execute(
            "INSERT INTO pg_returning (name, balance, active) VALUES ($1, $2, $3)".into(),
            vec![Value::Text(name.into()), Value::Int(balance), Value::Bool(true)],
        )
        .await
        .unwrap();
    }

    let updated = ReturningRow::query()
        .filter(ReturningRow::balance.lt(3_i64))
        .update_returning(&db, [ReturningRow::active.set(false)])
        .await
        .unwrap();
    assert_eq!(updated.len(), 2);
    assert!(updated.iter().all(|r| !r.active));

    let removed = ReturningRow::query()
        .filter(ReturningRow::name.eq("c"))
        .delete_returning(&db)
        .await
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].balance, 3);
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_returning")]
struct ReturningRow {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    name: String,
    balance: i64,
    active: bool,
}

#[tokio::test]
async fn transaction_commit_persists_and_rollback_discards() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_ledger",
        "CREATE TABLE pg_ledger (id BIGSERIAL PRIMARY KEY, amount BIGINT NOT NULL)",
    )
    .await;

    // Commit path.
    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO pg_ledger (amount) VALUES (10)".into(), vec![]).await?;
            Ok(())
        })
    })
    .await
    .unwrap();

    // Rollback path: the closure returns an error, so the INSERT is undone.
    let result: tork_orm::Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                tx.execute("INSERT INTO pg_ledger (amount) VALUES (20)".into(), vec![]).await?;
                Err(OrmError::query("force rollback"))
            })
        })
        .await;
    assert!(result.is_err());

    let rows = db.fetch_all("SELECT amount FROM pg_ledger".into(), vec![]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i64>("amount").unwrap(), 10);
}

#[tokio::test]
async fn savepoint_rolls_back_inner_only() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_savepoint",
        "CREATE TABLE pg_savepoint (id BIGSERIAL PRIMARY KEY, amount BIGINT NOT NULL)",
    )
    .await;

    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO pg_savepoint (amount) VALUES (1)".into(), vec![]).await?;
            // The inner savepoint fails and is rolled back; the outer INSERT survives.
            let _ = tx
                .savepoint(|sp| {
                    Box::pin(async move {
                        sp.execute("INSERT INTO pg_savepoint (amount) VALUES (2)".into(), vec![])
                            .await?;
                        Err::<(), _>(OrmError::query("inner fails"))
                    })
                })
                .await;
            Ok(())
        })
    })
    .await
    .unwrap();

    let rows = db.fetch_all("SELECT amount FROM pg_savepoint".into(), vec![]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i64>("amount").unwrap(), 1);
}
