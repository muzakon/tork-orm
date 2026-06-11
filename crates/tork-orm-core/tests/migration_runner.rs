//! Tests for the migration runner against in-memory SQLite. Migrations are
//! hand-written `MigrationTrait` impls (the `#[migration]` macro lands later).

use tork_orm_core::migration::*;
use tork_orm_core::{Database, Result, Value};

struct CreateUsers;

impl MigrationTrait for CreateUsers {
    fn revision(&self) -> &'static str {
        "20260611_000001"
    }

    fn name(&self) -> &'static str {
        "create_users"
    }

    fn up<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            schema
                .create_table("users")
                .column(Column::new("id").bigint().primary_key().auto_increment())
                .column(Column::new("username").varchar(50).not_null())
                .execute()
                .await?;
            Ok(())
        })
    }

    fn down<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            schema.drop_table("users").execute().await?;
            Ok(())
        })
    }
}

struct CreatePosts;

impl MigrationTrait for CreatePosts {
    fn revision(&self) -> &'static str {
        "20260611_000002"
    }

    fn name(&self) -> &'static str {
        "create_posts"
    }

    fn up<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            schema
                .create_table("posts")
                .column(Column::new("id").bigint().primary_key().auto_increment())
                .column(Column::new("user_id").bigint().not_null())
                .column(Column::new("title").varchar(255).not_null())
                .foreign_key(
                    ForeignKey::new()
                        .from("posts", "user_id")
                        .to("users", "id")
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .execute()
                .await?;
            Ok(())
        })
    }

    fn down<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            schema.drop_table("posts").execute().await?;
            Ok(())
        })
    }
}

/// A fresh set (the runner consumes the set, so each run needs its own).
fn migrations() -> MigrationSet {
    MigrationSet::new(vec![boxed(CreateUsers), boxed(CreatePosts)])
}

async fn table_exists(db: &Database, name: &str) -> bool {
    let rows = db
        .fetch_all(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?".into(),
            vec![Value::Text(name.into())],
        )
        .await
        .unwrap();
    !rows.is_empty()
}

#[tokio::test]
async fn up_applies_migrations_and_records_them() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    let applied = Migrator::new(&db, migrations()).up().await.unwrap();
    assert_eq!(applied, 2);

    // Both tables exist, and the foreign key relationship works.
    db.execute(
        "INSERT INTO users (username) VALUES (?)".into(),
        vec![Value::Text("alice".into())],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO posts (user_id, title) VALUES (?, ?)".into(),
        vec![Value::Int(1), Value::Text("hello".into())],
    )
    .await
    .unwrap();

    // The bookkeeping table records both, in one batch.
    let rows = db
        .fetch_all(
            "SELECT revision, name, batch FROM _tork_migrations ORDER BY revision".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<String>("revision").unwrap(), "20260611_000001");
    assert_eq!(rows[0].get::<String>("name").unwrap(), "create_users");
    assert_eq!(rows[0].get::<i64>("batch").unwrap(), 1);
    assert_eq!(rows[1].get::<i64>("batch").unwrap(), 1);
}

#[tokio::test]
async fn up_is_idempotent() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    assert_eq!(Migrator::new(&db, migrations()).up().await.unwrap(), 2);
    // A second run applies nothing.
    assert_eq!(Migrator::new(&db, migrations()).up().await.unwrap(), 0);
}

#[tokio::test]
async fn down_reverts_in_reverse_order() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    Migrator::new(&db, migrations()).up().await.unwrap();

    let reverted = Migrator::new(&db, migrations()).down(2).await.unwrap();
    assert_eq!(reverted, 2);

    // Both tables are gone and the bookkeeping table is empty.
    assert!(!table_exists(&db, "users").await);
    assert!(!table_exists(&db, "posts").await);
    let rows = db
        .fetch_all("SELECT revision FROM _tork_migrations".into(), vec![])
        .await
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn down_one_step_keeps_the_earlier_migration() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    Migrator::new(&db, migrations()).up().await.unwrap();

    // Reverting one step drops only the most recent (posts), keeping users.
    let reverted = Migrator::new(&db, migrations()).down(1).await.unwrap();
    assert_eq!(reverted, 1);
    assert!(table_exists(&db, "users").await);
    assert!(!table_exists(&db, "posts").await);
}
