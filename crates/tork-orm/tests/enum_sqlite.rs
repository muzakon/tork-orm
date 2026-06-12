//! Live enum tests against in-memory SQLite: a `#[field(db_enum)]` column
//! round-trips through the typed model API, filters by value, and the database
//! rejects a value outside the declared set via the generated `CHECK`.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
enum Status {
    Active,
    Inactive,
    #[db_enum(rename = "on_hold")]
    OnHold,
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "accounts")]
struct Account {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    name: String,
    #[field(db_enum)]
    status: Status,
    #[field(db_enum)]
    tier: Option<Status>,
}

/// Creates the table using the same DDL the schema renderer emits for SQLite
/// (a TEXT column plus a CHECK constraint), so the live CHECK is exercised.
async fn account_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE accounts (\
            id INTEGER PRIMARY KEY, \
            name TEXT NOT NULL, \
            status TEXT NOT NULL CHECK (status IN ('active', 'inactive', 'on_hold')), \
            tier TEXT CHECK (tier IN ('active', 'inactive', 'on_hold')))"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

fn account(name: &str, status: Status, tier: Option<Status>) -> Account {
    Account { id: 0, name: name.into(), status, tier }
}

#[tokio::test]
async fn enum_round_trips_through_the_model() {
    let db = account_db().await;
    let stored = Account::create(&db, &account("alice", Status::OnHold, Some(Status::Active)))
        .await
        .unwrap();
    assert_eq!(stored.status, Status::OnHold);
    assert_eq!(stored.tier, Some(Status::Active));

    let found = Account::find(&db, stored.id).await.unwrap();
    assert_eq!(found.status, Status::OnHold);
    assert_eq!(found.tier, Some(Status::Active));
}

#[tokio::test]
async fn nullable_enum_stores_none() {
    let db = account_db().await;
    let stored = Account::create(&db, &account("bob", Status::Active, None))
        .await
        .unwrap();
    let found = Account::find(&db, stored.id).await.unwrap();
    assert_eq!(found.tier, None);
}

#[tokio::test]
async fn filters_by_enum_value() {
    let db = account_db().await;
    Account::create(&db, &account("alice", Status::Active, None)).await.unwrap();
    Account::create(&db, &account("bob", Status::Inactive, None)).await.unwrap();
    Account::create(&db, &account("carol", Status::Active, None)).await.unwrap();

    let active = Account::query()
        .filter(Account::status.eq(Status::Active))
        .order_by(Account::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 2);
    assert_eq!(active[0].name, "alice");
    assert_eq!(active[1].name, "carol");
}

#[tokio::test]
async fn database_rejects_a_value_outside_the_set() {
    let db = account_db().await;
    // The typed API cannot construct an invalid variant, so write raw SQL to prove
    // the CHECK constraint is live.
    let result = db
        .execute(
            "INSERT INTO accounts (name, status) VALUES ('mallory', 'deleted')".into(),
            vec![],
        )
        .await;
    assert!(result.is_err(), "the CHECK constraint should reject an unknown value");
}
