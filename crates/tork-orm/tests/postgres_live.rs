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

use serde_json::json;
use time::macros::datetime;
use time::OffsetDateTime;
use tork_orm::migration::{SchemaManager, TriggerEvent};
use tork_orm::prelude::*;
use uuid::Uuid;

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

// ── JSON / JSONB ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Model)]
#[table(name = "pg_events")]
struct Event {
    #[field(primary_key, auto)]
    id: i64,
    payload: serde_json::Value,
}

#[tokio::test]
async fn jsonb_round_trip_and_operators() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_events",
        "CREATE TABLE pg_events (id BIGSERIAL PRIMARY KEY, payload JSONB NOT NULL)",
    )
    .await;

    let stored = Event::create(
        &db,
        &Event { id: 0, payload: json!({"kind": "click", "vip": true, "n": 3}) },
    )
    .await
    .unwrap();
    Event::create(
        &db,
        &Event { id: 0, payload: json!({"kind": "view", "vip": false}) },
    )
    .await
    .unwrap();

    // The JSON document round-trips exactly.
    let reloaded = Event::query().filter(Event::id.eq(stored.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.payload, json!({"kind": "click", "vip": true, "n": 3}));

    // `payload ->> 'kind' = 'click'`
    let clicks = Event::query()
        .filter(Event::payload.json_get_text("kind").eq("click"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(clicks.len(), 1);
    assert_eq!(clicks[0].payload["kind"], json!("click"));

    // `payload @> '{"vip": true}'`
    let vips = Event::query()
        .filter(Event::payload.json_contains(json!({"vip": true})))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(vips, 1);
}

// ── UUID ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_sessions")]
struct Session {
    #[field(primary_key)]
    id: Uuid,
    label: String,
}

#[tokio::test]
async fn uuid_primary_key_insert_and_query() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_sessions",
        "CREATE TABLE pg_sessions (id UUID PRIMARY KEY, label TEXT NOT NULL)",
    )
    .await;

    let id = Uuid::new_v4();
    let created = Session::create(&db, &Session { id, label: "first".into() }).await.unwrap();
    assert_eq!(created.id, id);

    let found = Session::query().filter(Session::id.eq(id)).one(&db).await.unwrap();
    assert_eq!(found, Session { id, label: "first".into() });

    let missing = Session::query()
        .filter(Session::id.eq(Uuid::new_v4()))
        .first(&db)
        .await
        .unwrap();
    assert!(missing.is_none());
}

// ── Arrays ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_posts")]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    tags: Vec<String>,
    scores: Vec<i64>,
}

#[tokio::test]
async fn array_round_trip_and_operators() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_posts",
        "CREATE TABLE pg_posts (\
            id BIGSERIAL PRIMARY KEY, \
            tags TEXT[] NOT NULL, \
            scores BIGINT[] NOT NULL)",
    )
    .await;

    let stored = Post::create(
        &db,
        &Post { id: 0, tags: vec!["rust".into(), "db".into()], scores: vec![10, 20] },
    )
    .await
    .unwrap();
    Post::create(
        &db,
        &Post { id: 0, tags: vec!["go".into(), "web".into()], scores: vec![5] },
    )
    .await
    .unwrap();

    // Arrays round-trip element-for-element.
    let reloaded = Post::query().filter(Post::id.eq(stored.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.tags, vec!["rust".to_string(), "db".to_string()]);
    assert_eq!(reloaded.scores, vec![10, 20]);

    // `'rust' = ANY(tags)`
    let rusty = Post::query().filter(Post::tags.any("rust".to_string())).all(&db).await.unwrap();
    assert_eq!(rusty.len(), 1);
    assert_eq!(rusty[0].id, stored.id);

    // `tags && ARRAY['web','mobile']` — overlaps the second post.
    let overlapping = Post::query()
        .filter(Post::tags.overlaps(["web".to_string(), "mobile".to_string()]))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(overlapping, 1);

    // `tags @> ARRAY['rust','db']` — contains both.
    let both = Post::query()
        .filter(Post::tags.array_contains(["rust".to_string(), "db".to_string()]))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(both, 1);
}

// ── Upsert (ON CONFLICT) ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_accounts2")]
struct Acct {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    email: String,
    balance: i64,
}

#[tokio::test]
async fn upsert_on_inserts_then_updates() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_accounts2",
        "CREATE TABLE pg_accounts2 (\
            id BIGSERIAL PRIMARY KEY, \
            email VARCHAR(50) NOT NULL UNIQUE, \
            balance BIGINT NOT NULL)",
    )
    .await;

    // First upsert inserts.
    let first = Acct::upsert_on(
        &db,
        &Acct { id: 0, email: "a@x.com".into(), balance: 100 },
        &["email"],
    )
    .await
    .unwrap();
    assert!(first.id > 0);
    assert_eq!(first.balance, 100);

    // Second upsert on the same email updates the existing row in place.
    let updated = Acct::upsert_on(
        &db,
        &Acct { id: 0, email: "a@x.com".into(), balance: 250 },
        &["email"],
    )
    .await
    .unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.balance, 250);
    assert_eq!(Acct::query().count(&db).await.unwrap(), 1);
}

// ── DB-side defaults (server-generated) ───────────────────────────────────────

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_tokens")]
struct Token {
    #[field(primary_key, default_uuid)]
    id: Uuid,
    label: String,
}

#[tokio::test]
async fn default_uuid_is_generated_by_the_database() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_tokens",
        "CREATE TABLE pg_tokens (\
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(), \
            label TEXT NOT NULL)",
    )
    .await;

    // The id is constructed nil but omitted from INSERT; the server fills it.
    let stored = Token::create(&db, &Token { id: Uuid::nil(), label: "first".into() })
        .await
        .unwrap();
    assert_ne!(stored.id, Uuid::nil());
    assert_eq!(stored.label, "first");

    let found = Token::query().filter(Token::id.eq(stored.id)).one(&db).await.unwrap();
    assert_eq!(found, stored);
}

#[derive(Debug, Clone, Model)]
#[table(name = "pg_audit")]
struct Audit {
    #[field(primary_key, auto)]
    id: i64,
    action: String,
    #[field(default_now)]
    at: OffsetDateTime,
}

#[tokio::test]
async fn default_now_is_filled_by_the_server() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_audit",
        "CREATE TABLE pg_audit (\
            id BIGSERIAL PRIMARY KEY, \
            action TEXT NOT NULL, \
            at TIMESTAMPTZ NOT NULL DEFAULT now())",
    )
    .await;

    // `at` is omitted from INSERT; the server stamps it with now().
    let stored = Audit::create(
        &db,
        &Audit { id: 0, action: "login".into(), at: OffsetDateTime::UNIX_EPOCH },
    )
    .await
    .unwrap();
    // The server time is far after the epoch placeholder we passed.
    assert!(stored.at.year() >= 2025);
}

// ── Trigger DSL (end to end) ──────────────────────────────────────────────────

#[tokio::test]
async fn create_trigger_fires_on_insert() {
    let db = connect().await;
    db.execute("DROP TABLE IF EXISTS pg_trig".into(), vec![]).await.unwrap();
    db.execute(
        "CREATE TABLE pg_trig (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, name_upper TEXT)".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "CREATE OR REPLACE FUNCTION pg_trig_upper() RETURNS trigger AS $$ \
         BEGIN NEW.name_upper := upper(NEW.name); RETURN NEW; END; $$ LANGUAGE plpgsql"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    // Build the trigger through the DSL and apply the rendered SQL.
    let mut schema = SchemaManager::collect(&**db.dialect());
    schema
        .create_trigger("pg_trig_t")
        .before()
        .event(TriggerEvent::Insert)
        .on("pg_trig")
        .for_each_row()
        .body("EXECUTE FUNCTION pg_trig_upper()")
        .execute()
        .await
        .unwrap();
    for sql in schema.into_collected() {
        db.execute(sql, vec![]).await.unwrap();
    }

    db.execute(
        "INSERT INTO pg_trig (name) VALUES ($1)".into(),
        vec![Value::Text("hello".into())],
    )
    .await
    .unwrap();

    let rows = db.fetch_all("SELECT name_upper FROM pg_trig".into(), vec![]).await.unwrap();
    assert_eq!(rows[0].get::<String>("name_upper").unwrap(), "HELLO");
}

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
enum AccountStatus {
    Active,
    Inactive,
    #[db_enum(rename = "on_hold")]
    OnHold,
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_enum_accounts")]
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
async fn enum_round_trips_and_check_rejects_unknown() {
    let db = connect().await;
    reset(
        &db,
        "DROP TABLE IF EXISTS pg_enum_accounts",
        "CREATE TABLE pg_enum_accounts (\
            id BIGSERIAL PRIMARY KEY, \
            name VARCHAR(50) NOT NULL, \
            status VARCHAR(255) NOT NULL CHECK (status IN ('active', 'inactive', 'on_hold')), \
            tier VARCHAR(255) CHECK (tier IN ('active', 'inactive', 'on_hold')))",
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

    // The CHECK constraint rejects a value outside the declared set.
    let bad = db
        .execute(
            "INSERT INTO pg_enum_accounts (name, status) VALUES ('mallory', 'deleted')".into(),
            vec![],
        )
        .await;
    assert!(bad.is_err(), "CHECK should reject an unknown enum value");
}

/// Serializes the tests that share the `pg_items` table.
static ITEMS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_items")]
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
        "DROP TABLE IF EXISTS pg_items",
        "CREATE TABLE pg_items (id BIGSERIAL PRIMARY KEY, category VARCHAR(50) NOT NULL, price BIGINT NOT NULL)",
    )
    .await;
    for (category, price) in
        [("books", 10), ("books", 30), ("toys", 20), ("toys", 5), ("toys", 40)]
    {
        db.execute(
            "INSERT INTO pg_items (category, price) VALUES ($1, $2)".into(),
            vec![Value::Text(category.into()), Value::Int(price)],
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn distinct_on_keeps_the_top_row_per_group() {
    let _guard = ITEMS_LOCK.lock().await;
    let db = item_db().await;
    // The cheapest item in each category: DISTINCT ON (category) ORDER BY category, price.
    let rows = Item::query()
        .distinct_on((Item::category,))
        .order_by(Item::category.asc())
        .order_by(Item::price.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].category, "books");
    assert_eq!(rows[0].price, 10);
    assert_eq!(rows[1].category, "toys");
    assert_eq!(rows[1].price, 5);
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
    // SKIP LOCKED is a no-op here (nothing else holds a lock) but must execute.
    let rows = Item::query()
        .filter(Item::category.eq("toys"))
        .for_update()
        .skip_locked()
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
}

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "pg_docs")]
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
        "DROP TABLE IF EXISTS pg_docs",
        "CREATE TABLE pg_docs (\
            id BIGSERIAL PRIMARY KEY, \
            body VARCHAR(50) NOT NULL, \
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
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
