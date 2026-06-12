//! Tests for DDL rendering under the PostgreSQL dialect. No database is involved;
//! these assert the exact SQL the dialect produces so a future live driver has a
//! verified contract to rely on.

use tork_orm_core::dialect::PostgresDialect;
use tork_orm_core::migration::{Column, ForeignKey, ForeignKeyAction, SchemaManager};

#[tokio::test]
async fn create_table_uses_postgres_types_and_identity() {
    let dialect = PostgresDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("users")
        .if_not_exists()
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(Column::new("username").varchar(50).not_null().unique())
        .column(Column::new("is_active").boolean().not_null().default(true))
        .column(Column::new("avatar").blob())
        .column(Column::new("score").real())
        .timestamps()
        .execute()
        .await
        .unwrap();

    let statements = schema.into_collected();
    assert_eq!(statements.len(), 1);
    assert_eq!(
        statements[0],
        "CREATE TABLE IF NOT EXISTS \"users\" (\
\"id\" BIGSERIAL PRIMARY KEY, \
\"username\" VARCHAR(50) NOT NULL UNIQUE, \
\"is_active\" BOOLEAN NOT NULL DEFAULT true, \
\"avatar\" BYTEA, \
\"score\" DOUBLE PRECISION, \
\"created_at\" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP, \
\"updated_at\" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP)"
    );
}

#[tokio::test]
async fn foreign_key_renders_table_level_constraint() {
    let dialect = PostgresDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("posts")
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(Column::new("user_id").bigint().not_null())
        .foreign_key(
            ForeignKey::new()
                .from("posts", "user_id")
                .to("users", "id")
                .on_delete(ForeignKeyAction::Cascade),
        )
        .execute()
        .await
        .unwrap();

    assert_eq!(
        schema.into_collected()[0],
        "CREATE TABLE \"posts\" (\
\"id\" BIGSERIAL PRIMARY KEY, \
\"user_id\" BIGINT NOT NULL, \
FOREIGN KEY (\"user_id\") REFERENCES \"users\" (\"id\") ON DELETE CASCADE)"
    );
}
