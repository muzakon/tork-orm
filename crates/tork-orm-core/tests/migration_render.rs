//! Tests for DDL rendering: the schema builder in collect mode produces exact,
//! deterministic SQL for SQLite. No database is involved.

use tork_orm_core::dialect::SqliteDialect;
use tork_orm_core::migration::{Column, ForeignKey, ForeignKeyAction, SchemaManager};

#[tokio::test]
async fn create_table_renders_columns_and_constraints() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("users")
        .if_not_exists()
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(Column::new("username").varchar(50).not_null().unique())
        .column(Column::new("is_active").boolean().not_null().default(true))
        .timestamps()
        .execute()
        .await
        .unwrap();

    let statements = schema.into_collected();
    assert_eq!(statements.len(), 1);
    assert_eq!(
        statements[0],
        "CREATE TABLE IF NOT EXISTS \"users\" (\
\"id\" INTEGER PRIMARY KEY AUTOINCREMENT, \
\"username\" VARCHAR(50) NOT NULL UNIQUE, \
\"is_active\" BOOLEAN NOT NULL DEFAULT 1, \
\"created_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, \
\"updated_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)"
    );
}

#[tokio::test]
async fn foreign_key_renders_table_level_constraint() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
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
        .await
        .unwrap();

    let statements = schema.into_collected();
    assert_eq!(
        statements[0],
        "CREATE TABLE \"posts\" (\
\"id\" INTEGER PRIMARY KEY AUTOINCREMENT, \
\"user_id\" BIGINT NOT NULL, \
\"title\" VARCHAR(255) NOT NULL, \
FOREIGN KEY (\"user_id\") REFERENCES \"users\" (\"id\") ON DELETE CASCADE)"
    );
}

#[tokio::test]
async fn drop_table_renders() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema.drop_table("users").if_exists().execute().await.unwrap();
    assert_eq!(schema.into_collected()[0], "DROP TABLE IF EXISTS \"users\"");
}

/// Renders the standard sample table, used to prove rendering is deterministic.
async fn render_sample() -> Vec<String> {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("users")
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(Column::new("email").varchar(255).not_null().unique())
        .timestamps()
        .execute()
        .await
        .unwrap();
    schema.into_collected()
}

#[tokio::test]
async fn rendering_is_deterministic() {
    // The same migration renders byte-identically every time (the checksum
    // depends on this).
    assert_eq!(render_sample().await, render_sample().await);
}

#[tokio::test]
async fn text_defaults_are_escaped() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("notes")
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(Column::new("label").text().not_null().default("a'b"))
        .execute()
        .await
        .unwrap();
    // The embedded quote is doubled, so it cannot break out of the literal.
    assert!(schema.into_collected()[0].contains("DEFAULT 'a''b'"));
}

#[tokio::test]
async fn composite_primary_key_is_table_level() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("memberships")
        .column(Column::new("user_id").bigint().not_null())
        .column(Column::new("group_id").bigint().not_null())
        .primary_key(&["user_id", "group_id"])
        .execute()
        .await
        .unwrap();
    assert_eq!(
        schema.into_collected()[0],
        "CREATE TABLE \"memberships\" (\
\"user_id\" BIGINT NOT NULL, \
\"group_id\" BIGINT NOT NULL, \
PRIMARY KEY (\"user_id\", \"group_id\"))"
    );
}
