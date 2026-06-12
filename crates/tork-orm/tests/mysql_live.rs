//! Live integration tests against a real MySQL server.
//!
//! Bring the database up first:
//!
//! ```text
//! docker compose up -d
//! cargo test -p tork-orm --features mysql --test mysql_live
//! ```
//!
//! The connection URL defaults to the `docker-compose.yml` service; override it with
//! `TEST_MYSQL_URL`. Each test uses its own table, so they are isolated.
#![cfg(feature = "mysql")]

use serde_json::json;
use time::macros::datetime;
use time::OffsetDateTime;
use tork_orm::prelude::*;
use tork_orm::{group_concat, AggFunc};

/// Serializes the tests that share the `my_users` table, since they each
/// `DROP`/`CREATE` it and would otherwise race when run in parallel.
static USERS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The connection URL for the test database.
fn database_url() -> String {
    std::env::var("TEST_MYSQL_URL")
        .unwrap_or_else(|_| "mysql://tork:tork@localhost:3307/tork_test".to_string())
}

async fn connect() -> Database {
    Database::connect(&database_url(), 5)
        .await
        .expect("connect to the test MySQL (is `docker compose up -d` running?)")
}

/// Drops and recreates a table so each test starts from a known state.
async fn reset(db: &Database, drop_sql: &str, create_sql: &str) {
    db.execute(drop_sql.into(), vec![]).await.unwrap();
    db.execute(create_sql.into(), vec![]).await.unwrap();
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    name: String,
    active: bool,
    score: f64,
}

async fn user_db() -> Database {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_users",
        "CREATE TABLE my_users (\
            id BIGINT AUTO_INCREMENT PRIMARY KEY, \
            name VARCHAR(50) NOT NULL, \
            active TINYINT(1) NOT NULL, \
            score DOUBLE NOT NULL)",
    )
    .await;
    db
}

#[tokio::test]
async fn connects_and_runs_basic_sql() {
    let db = connect().await;
    let rows = db.fetch_all("SELECT 1 AS one".into(), vec![]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i64>("one").unwrap(), 1);
}

#[tokio::test]
async fn create_returns_autoincrement_id_then_crud() {
    let _guard = USERS_LOCK.lock().await;
    let db = user_db().await;

    // No RETURNING on MySQL: create() re-selects by LAST_INSERT_ID().
    let alice = User::create(
        &db,
        &User { id: 0, name: "alice".into(), active: true, score: 1.5 },
    )
    .await
    .unwrap();
    assert!(alice.id > 0);
    assert_eq!(alice.name, "alice");

    User::create(&db, &User { id: 0, name: "bob".into(), active: false, score: 2.0 })
        .await
        .unwrap();

    // Filter (`?` placeholders), order, limit.
    let active = User::query()
        .filter(User::active.eq(true))
        .order_by(User::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "alice");

    let changed = User::query()
        .filter(User::name.eq("bob"))
        .update(&db, [User::active.set(true)])
        .await
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(User::query().filter(User::active.eq(true)).count(&db).await.unwrap(), 2);

    let removed = User::query().filter(User::name.eq("alice")).delete(&db).await.unwrap();
    assert_eq!(removed, 1);
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_typebag")]
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
async fn all_value_types_round_trip() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_typebag",
        "CREATE TABLE my_typebag (\
            id BIGINT AUTO_INCREMENT PRIMARY KEY, \
            flag TINYINT(1) NOT NULL, \
            small INT NOT NULL, \
            big BIGINT NOT NULL, \
            ratio DOUBLE NOT NULL, \
            label TEXT NOT NULL, \
            payload BLOB NOT NULL, \
            recorded_at DATETIME NOT NULL)",
    )
    .await;

    let original = TypeBag {
        id: 0,
        flag: true,
        small: -12345,
        big: 9_000_000_000,
        ratio: 2.5,
        label: "héllo".into(),
        payload: vec![0, 1, 2, 250, 255],
        recorded_at: datetime!(2026-06-12 14:30:05 UTC),
    };
    let stored = TypeBag::create(&db, &original).await.unwrap();
    assert!(stored.id > 0);

    let reloaded = TypeBag::query().filter(TypeBag::id.eq(stored.id)).one(&db).await.unwrap();
    assert!(reloaded.flag);
    assert_eq!(reloaded.small, -12345);
    assert_eq!(reloaded.big, 9_000_000_000);
    assert_eq!(reloaded.ratio, 2.5);
    assert_eq!(reloaded.label, "héllo");
    assert_eq!(reloaded.payload, vec![0, 1, 2, 250, 255]);
    assert_eq!(reloaded.recorded_at, datetime!(2026-06-12 14:30:05 UTC));
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_accounts")]
struct Acct {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    email: String,
    balance: i64,
}

#[tokio::test]
async fn upsert_on_inserts_then_updates_via_on_duplicate_key() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_accounts",
        "CREATE TABLE my_accounts (\
            id BIGINT AUTO_INCREMENT PRIMARY KEY, \
            email VARCHAR(50) NOT NULL UNIQUE, \
            balance BIGINT NOT NULL)",
    )
    .await;

    let first = Acct::upsert_on(
        &db,
        &Acct { id: 0, email: "a@x.com".into(), balance: 100 },
        &["email"],
    )
    .await
    .unwrap();
    assert!(first.id > 0);
    assert_eq!(first.balance, 100);

    let updated = Acct::upsert_on(
        &db,
        &Acct { id: 0, email: "a@x.com".into(), balance: 250 },
        &["email"],
    )
    .await
    .unwrap();
    assert_eq!(updated.balance, 250);
    assert_eq!(Acct::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn transaction_commit_and_rollback_and_savepoint() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_ledger",
        "CREATE TABLE my_ledger (id BIGINT AUTO_INCREMENT PRIMARY KEY, amount BIGINT NOT NULL)",
    )
    .await;

    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO my_ledger (amount) VALUES (10)".into(), vec![]).await?;
            Ok(())
        })
    })
    .await
    .unwrap();

    let result: tork_orm::Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                tx.execute("INSERT INTO my_ledger (amount) VALUES (20)".into(), vec![]).await?;
                Err(OrmError::query("force rollback"))
            })
        })
        .await;
    assert!(result.is_err());

    // Savepoint: the inner insert is undone, the outer survives.
    db.transaction(|tx| {
        Box::pin(async move {
            tx.execute("INSERT INTO my_ledger (amount) VALUES (30)".into(), vec![]).await?;
            let _ = tx
                .savepoint(|sp| {
                    Box::pin(async move {
                        sp.execute("INSERT INTO my_ledger (amount) VALUES (99)".into(), vec![])
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

    let rows = db
        .fetch_all("SELECT amount FROM my_ledger ORDER BY amount".into(), vec![])
        .await
        .unwrap();
    let amounts: Vec<i64> = rows.iter().map(|r| r.get::<i64>("amount").unwrap()).collect();
    assert_eq!(amounts, vec![10, 30]);
}

// ── JSON (native in MySQL) ────────────────────────────────────────────────────

#[derive(Debug, Clone, Model)]
#[table(name = "my_events")]
struct Event {
    #[field(primary_key, auto)]
    id: i64,
    payload: serde_json::Value,
}

#[tokio::test]
async fn json_round_trip_and_operators() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_events",
        "CREATE TABLE my_events (id BIGINT AUTO_INCREMENT PRIMARY KEY, payload JSON NOT NULL)",
    )
    .await;

    let stored = Event::create(
        &db,
        &Event { id: 0, payload: json!({"kind": "click", "vip": true}) },
    )
    .await
    .unwrap();
    Event::create(&db, &Event { id: 0, payload: json!({"kind": "view", "vip": false}) })
        .await
        .unwrap();

    let reloaded = Event::query().filter(Event::id.eq(stored.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.payload, json!({"kind": "click", "vip": true}));

    // `payload ->> '$.kind' = 'click'`
    let clicks = Event::query()
        .filter(Event::payload.json_get_text("kind").eq("click"))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(clicks, 1);

    // `JSON_CONTAINS(payload, '{"vip": true}')`
    let vips = Event::query()
        .filter(Event::payload.json_contains(json!({"vip": true})))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(vips, 1);
}

// ── MySQL-specific function + emulated FILTER + FULL JOIN error ────────────────

#[derive(Debug, QueryResult)]
struct Concatenated {
    names: String,
}

#[derive(Debug, QueryResult)]
struct Counted {
    n: i64,
}

#[tokio::test]
async fn group_concat_and_emulated_filter() {
    let _guard = USERS_LOCK.lock().await;
    let db = user_db().await;
    User::create(&db, &User { id: 0, name: "alice".into(), active: true, score: 1.0 }).await.unwrap();
    User::create(&db, &User { id: 0, name: "bob".into(), active: false, score: 2.0 }).await.unwrap();
    User::create(&db, &User { id: 0, name: "carol".into(), active: true, score: 3.0 }).await.unwrap();

    // GROUP_CONCAT (MySQL-specific, behind the `mysql` feature).
    let concat = User::query()
        .order_by(User::id.asc())
        .select((group_concat(User::name).as_("names"),))
        .all_as::<Concatenated>(&db)
        .await
        .unwrap();
    assert_eq!(concat[0].names, "alice,bob,carol");

    // Aggregate FILTER, emulated as COUNT(CASE WHEN active THEN id END).
    let counted = User::query()
        .select((
            Expr::aggregate(AggFunc::Count, [User::id.expr()])
                .filter(User::active.eq(true))
                .as_("n"),
        ))
        .all_as::<Counted>(&db)
        .await
        .unwrap();
    assert_eq!(counted[0].n, 2);
}

#[derive(Debug, Clone, Model)]
#[table(name = "my_posts")]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = User::id)]
    user_id: i64,
    title: String,
}

#[relations]
impl User {
    #[has_many(Post, foreign_key = Post::user_id)]
    pub fn posts() {}
}

#[tokio::test]
async fn full_outer_join_is_rejected() {
    let _guard = USERS_LOCK.lock().await;
    let db = user_db().await;
    let err = User::query().full_join(User::posts()).all(&db).await.unwrap_err();
    assert!(
        err.to_string().contains("FULL OUTER JOIN"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn window_function_and_cte_run_on_mysql() {
    let _guard = USERS_LOCK.lock().await;
    let db = user_db().await;
    for (name, active) in [("a", true), ("b", true), ("c", false)] {
        db.execute(
            "INSERT INTO my_users (name, active, score) VALUES (?, ?, 0)".into(),
            vec![Value::Text(name.into()), Value::Bool(active)],
        )
        .await
        .unwrap();
    }

    // Window function (MySQL 8).
    let rows = db
        .fetch_all(
            "SELECT name, ROW_NUMBER() OVER (ORDER BY id) AS rn FROM my_users".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(rows[0].get::<i64>("rn").unwrap(), 1);
    assert_eq!(rows[2].get::<i64>("rn").unwrap(), 3);

    // CTE (MySQL 8).
    let count = db
        .fetch_all(
            "WITH actives AS (SELECT id FROM my_users WHERE active = 1) \
             SELECT COUNT(*) AS c FROM actives"
                .into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(count[0].get::<i64>("c").unwrap(), 2);
}

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
enum AccountStatus {
    Active,
    Inactive,
    #[db_enum(rename = "on_hold")]
    OnHold,
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_enum_accounts")]
struct EnumAccount {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    name: String,
    #[field(db_enum)]
    status: AccountStatus,
    #[field(db_enum)]
    tier: Option<AccountStatus>,
}

#[tokio::test]
async fn enum_round_trips_and_native_enum_rejects_unknown() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_enum_accounts",
        "CREATE TABLE my_enum_accounts (\
            id BIGINT AUTO_INCREMENT PRIMARY KEY, \
            name VARCHAR(50) NOT NULL, \
            status ENUM('active', 'inactive', 'on_hold') NOT NULL, \
            tier ENUM('active', 'inactive', 'on_hold'))",
    )
    .await;

    let stored = EnumAccount::create(
        &db,
        &EnumAccount {
            id: 0,
            name: "alice".into(),
            status: AccountStatus::OnHold,
            tier: Some(AccountStatus::Active),
        },
    )
    .await
    .unwrap();
    assert_eq!(stored.status, AccountStatus::OnHold);
    assert_eq!(stored.tier, Some(AccountStatus::Active));

    EnumAccount::create(
        &db,
        &EnumAccount { id: 0, name: "bob".into(), status: AccountStatus::Active, tier: None },
    )
    .await
    .unwrap();

    let actives = EnumAccount::query()
        .filter(EnumAccount::status.eq(AccountStatus::Active))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(actives.len(), 1);
    assert_eq!(actives[0].name, "bob");
    assert_eq!(actives[0].tier, None);

    // MySQL's strict mode rejects a value outside the native ENUM set.
    let bad = db
        .execute(
            "INSERT INTO my_enum_accounts (name, status) VALUES ('mallory', 'deleted')".into(),
            vec![],
        )
        .await;
    assert!(bad.is_err(), "native ENUM should reject an unknown value");
}

/// Serializes the tests that share the `my_items` table.
static ITEMS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_items")]
struct Item {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    category: String,
    price: i64,
}

async fn item_db() -> Database {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_items",
        "CREATE TABLE my_items (id BIGINT AUTO_INCREMENT PRIMARY KEY, category VARCHAR(50) NOT NULL, price BIGINT NOT NULL)",
    )
    .await;
    for (category, price) in [("books", 10), ("toys", 20), ("toys", 5), ("toys", 40)] {
        db.execute(
            "INSERT INTO my_items (category, price) VALUES (?, ?)".into(),
            vec![Value::Text(category.into()), Value::Int(price)],
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn keyset_pagination_walks_pages() {
    let _guard = ITEMS_LOCK.lock().await;
    let db = item_db().await;
    let page1 = Item::query().order_by(Item::id.asc()).limit(2).all(&db).await.unwrap();
    assert_eq!(page1.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1, 2]);

    let cursor = vec![page1.last().unwrap().id.to_value()];
    let page2 = Item::query()
        .order_by(Item::id.asc())
        .keyset_after(cursor)
        .limit(2)
        .all(&db)
        .await
        .unwrap();
    assert_eq!(page2.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 4]);
}

#[tokio::test]
async fn for_update_skip_locked_runs() {
    let _guard = ITEMS_LOCK.lock().await;
    let db = item_db().await;
    let rows = Item::query()
        .filter(Item::category.eq("toys"))
        .for_update()
        .skip_locked()
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn distinct_on_is_rejected_on_mysql() {
    let _guard = ITEMS_LOCK.lock().await;
    let db = item_db().await;
    let result = Item::query().distinct_on((Item::category,)).all(&db).await;
    assert!(result.is_err(), "DISTINCT ON should be rejected on MySQL");
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_docs")]
struct Doc {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    body: String,
    #[field(updated_at)]
    updated_at: OffsetDateTime,
    #[field(version)]
    version: i64,
}

#[tokio::test]
async fn lifecycle_version_conflict_and_timestamp() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_docs",
        "CREATE TABLE my_docs (\
            id BIGINT AUTO_INCREMENT PRIMARY KEY, \
            body VARCHAR(50) NOT NULL, \
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, \
            version BIGINT NOT NULL)",
    )
    .await;

    let created = Doc::create(
        &db,
        &Doc { id: 0, body: "v1".into(), updated_at: OffsetDateTime::UNIX_EPOCH, version: 1 },
    )
    .await
    .unwrap();
    assert_eq!(created.version, 1);
    assert!(created.updated_at.year() >= 2026);

    let mut a = Doc::find(&db, created.id).await.unwrap();
    let mut b = Doc::find(&db, created.id).await.unwrap();

    a.body = "from-a".into();
    a.save(&db).await.unwrap();
    assert_eq!(a.version, 2);

    b.body = "from-b".into();
    assert_eq!(b.save(&db).await.unwrap_err().kind(), ErrorKind::Conflict);
    assert_eq!(Doc::find(&db, created.id).await.unwrap().body, "from-a");
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "my_notes")]
struct Note {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    body: String,
    #[field(deleted_at)]
    deleted_at: Option<OffsetDateTime>,
}

#[tokio::test]
async fn soft_delete_scope_and_restore() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS my_notes",
        "CREATE TABLE my_notes (id BIGINT AUTO_INCREMENT PRIMARY KEY, body VARCHAR(50) NOT NULL, deleted_at DATETIME NULL)",
    )
    .await;

    let a = Note::create(&db, &Note { id: 0, body: "a".into(), deleted_at: None }).await.unwrap();
    Note::create(&db, &Note { id: 0, body: "b".into(), deleted_at: None }).await.unwrap();

    a.delete(&db).await.unwrap();
    assert_eq!(Note::query().count(&db).await.unwrap(), 1);
    assert_eq!(Note::query().with_deleted().count(&db).await.unwrap(), 2);

    let deleted = Note::query().only_deleted().one(&db).await.unwrap();
    assert!(deleted.deleted_at.is_some());
    deleted.restore(&db).await.unwrap();
    assert_eq!(Note::query().count(&db).await.unwrap(), 2);

    let removed = Note::query().with_deleted().hard_delete(&db).await.unwrap();
    assert_eq!(removed, 2);
}
