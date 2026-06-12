//! Tests for how an enum column renders to DDL on each dialect. No database is
//! involved; these assert the exact SQL produced: a native `ENUM(...)` on MySQL
//! and a text column plus a `CHECK (... IN (...))` constraint elsewhere.

use tork_orm_core::dialect::{Dialect, MySqlDialect, PostgresDialect, SqliteDialect};
use tork_orm_core::migration::{Column, SchemaManager};

/// Builds the `CREATE TABLE` for a small table with a non-null and a nullable
/// enum column, under the given dialect.
async fn create_table(dialect: &dyn Dialect) -> String {
    let mut schema = SchemaManager::collect(dialect);
    schema
        .create_table("accounts")
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(
            Column::new("status")
                .enum_type("status", &["active", "inactive", "on_hold"])
                .not_null(),
        )
        .column(Column::new("tier").enum_type("tier", &["free", "pro"]))
        .execute()
        .await
        .unwrap();
    schema.into_collected().remove(0)
}

#[tokio::test]
async fn mysql_uses_native_enum_without_check() {
    let sql = create_table(&MySqlDialect::new()).await;
    assert!(
        sql.contains("`status` ENUM('active', 'inactive', 'on_hold') NOT NULL"),
        "unexpected MySQL DDL: {sql}"
    );
    // A nullable enum keeps the native type and stays nullable.
    assert!(sql.contains("`tier` ENUM('free', 'pro')"), "unexpected MySQL DDL: {sql}");
    // MySQL's ENUM type constrains the column itself, so no CHECK is emitted.
    assert!(!sql.contains("CHECK"), "MySQL enum should not add a CHECK: {sql}");
}

#[tokio::test]
async fn postgres_uses_varchar_plus_check() {
    let sql = create_table(&PostgresDialect::new()).await;
    assert!(
        sql.contains(
            "\"status\" VARCHAR(255) NOT NULL CHECK (\"status\" IN ('active', 'inactive', 'on_hold'))"
        ),
        "unexpected PostgreSQL DDL: {sql}"
    );
    // The nullable enum is validated too, without a NOT NULL.
    assert!(
        sql.contains("\"tier\" VARCHAR(255) CHECK (\"tier\" IN ('free', 'pro'))"),
        "unexpected PostgreSQL DDL: {sql}"
    );
}

#[tokio::test]
async fn sqlite_uses_text_plus_check() {
    let sql = create_table(&SqliteDialect::new()).await;
    assert!(
        sql.contains(
            "\"status\" TEXT NOT NULL CHECK (\"status\" IN ('active', 'inactive', 'on_hold'))"
        ),
        "unexpected SQLite DDL: {sql}"
    );
    assert!(
        sql.contains("\"tier\" TEXT CHECK (\"tier\" IN ('free', 'pro'))"),
        "unexpected SQLite DDL: {sql}"
    );
}
