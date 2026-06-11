//! Tests for the `SchemaManager` index builders.

#![cfg(feature = "migrations")]

use tork_orm_core::dialect::SqliteDialect;
use tork_orm_core::migration::{IndexColumn, SchemaManager};
use tork_orm_core::query::expr::{BinaryOp, Expr};
use tork_orm_core::{Database, Value};

#[tokio::test]
async fn collect_mode_renders_compound_desc_unique_index() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_index("uq_posts_user_created")
        .on_table("posts")
        .unique()
        .columns([
            IndexColumn::new("user_id"),
            IndexColumn::new("created_at").desc(),
        ])
        .if_not_exists()
        .execute()
        .await
        .unwrap();
    let sql = schema.into_collected();
    assert_eq!(
        sql,
        vec![
            "CREATE UNIQUE INDEX IF NOT EXISTS \"uq_posts_user_created\" ON \"posts\" \
             (\"user_id\", \"created_at\" DESC)"
                .to_string()
        ]
    );
}

#[tokio::test]
async fn partial_index_collects_inline_predicate() {
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_index("idx_posts_published")
        .on_table("posts")
        .column(IndexColumn::new("created_at"))
        .where_(Expr::binary(
            Expr::column("posts", "status"),
            BinaryOp::Eq,
            Expr::value(Value::Text("published".to_string())),
        ))
        .execute()
        .await
        .unwrap();
    assert_eq!(
        schema.into_collected()[0],
        "CREATE INDEX \"idx_posts_published\" ON \"posts\" (\"created_at\") \
         WHERE \"posts\".\"status\" = 'published'"
    );
}

#[tokio::test]
async fn rendered_index_applies_to_sqlite() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, slug TEXT NOT NULL)"
            .into(),
        vec![],
    )
    .await
    .unwrap();

    // Render the index through the builder, then apply the SQL to the database.
    let dialect = SqliteDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_index("uq_posts_user_slug")
        .on_table("posts")
        .unique()
        .columns([IndexColumn::new("user_id"), IndexColumn::new("slug")])
        .execute()
        .await
        .unwrap();
    for statement in schema.into_collected() {
        db.execute(statement, vec![]).await.unwrap();
    }

    db.execute(
        "INSERT INTO posts (user_id, slug) VALUES (?, ?)".into(),
        vec![Value::Int(1), Value::Text("hello".into())],
    )
    .await
    .unwrap();

    // The unique index rejects a duplicate (user_id, slug).
    let duplicate = db
        .execute(
            "INSERT INTO posts (user_id, slug) VALUES (?, ?)".into(),
            vec![Value::Int(1), Value::Text("hello".into())],
        )
        .await;
    assert!(duplicate.is_err());
}
