//! End-to-end migration test: define migrations with the `#[migration]` macro,
//! apply them, use the ORM against the migrated schema, roll back, and verify
//! per-migration atomicity.

// Explicit migration imports so the migration `Column` shadows the query-side
// `Column<M, T>` brought in by the prelude glob. (`SchemaManager` appears in the
// migration signatures as tokens the `#[migration]` macro consumes, so it needs no
// import here; a real migration file glob-imports `tork_orm::migration::*`.)
use tork_orm::migration::{
    boxed, migration, Column, ForeignKey, ForeignKeyAction, MigrationSet, Migrator,
};
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    username: String,
}

#[derive(Debug, Clone, Model)]
#[table(name = "posts")]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = User::id)]
    user_id: i64,
    title: String,
}

struct CreateUsers;

#[migration]
impl CreateUsers {
    fn revision() -> &'static str {
        "20260611_000001"
    }
    fn name() -> &'static str {
        "create_users"
    }
    async fn up(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema
            .create_table("users")
            .column(Column::new("id").bigint().primary_key().auto_increment())
            .column(Column::new("username").varchar(50).not_null())
            .execute()
            .await?;
        Ok(())
    }
    async fn down(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema.drop_table("users").execute().await?;
        Ok(())
    }
}

struct CreatePosts;

#[migration]
impl CreatePosts {
    fn revision() -> &'static str {
        "20260611_000002"
    }
    fn name() -> &'static str {
        "create_posts"
    }
    async fn up(schema: &mut SchemaManager<'_>) -> Result<()> {
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
    }
    async fn down(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema.drop_table("posts").execute().await?;
        Ok(())
    }
}

/// A migration that creates a table and then fails, to test rollback.
struct BadMigration;

#[migration]
impl BadMigration {
    fn revision() -> &'static str {
        "20260611_000002"
    }
    fn name() -> &'static str {
        "bad"
    }
    async fn up(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema
            .create_table("widgets")
            .column(Column::new("id").bigint().primary_key().auto_increment())
            .execute()
            .await?;
        // This statement fails, so the whole migration must roll back.
        schema.raw("THIS IS NOT VALID SQL").await?;
        Ok(())
    }
    async fn down(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema.drop_table("widgets").if_exists().execute().await?;
        Ok(())
    }
}

fn schema_migrations() -> MigrationSet {
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
async fn migrate_then_use_the_orm() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    let applied = Migrator::new(&db, schema_migrations()).up().await.unwrap();
    assert_eq!(applied, 2);

    // The ORM works against the migrated schema, including the foreign key.
    let user = User::create(
        &db,
        &User {
            id: 0,
            username: "alice".into(),
        },
    )
    .await
    .unwrap();
    let post = Post::create(
        &db,
        &Post {
            id: 0,
            user_id: user.id,
            title: "hello".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(post.user_id, user.id);

    let posts = Post::query()
        .filter(Post::user_id.eq(user.id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(posts.len(), 1);

    // Rolling back removes both tables.
    let reverted = Migrator::new(&db, schema_migrations()).down(2).await.unwrap();
    assert_eq!(reverted, 2);
    assert!(!table_exists(&db, "users").await);
    assert!(!table_exists(&db, "posts").await);
}

#[tokio::test]
async fn failed_migration_rolls_back_and_keeps_earlier_ones() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    let set = MigrationSet::new(vec![boxed(CreateUsers), boxed(BadMigration)]);

    let error = Migrator::new(&db, set).up().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Query);

    // The first migration committed and stays; the failed one left no partial
    // table and no bookkeeping row.
    assert!(table_exists(&db, "users").await);
    assert!(!table_exists(&db, "widgets").await);
    let rows = db
        .fetch_all("SELECT revision FROM _tork_migrations".into(), vec![])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String>("revision").unwrap(), "20260611_000001");
}
