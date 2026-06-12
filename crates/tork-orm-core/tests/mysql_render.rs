//! Tests for SQL rendering under the MySQL dialect. No database is involved; these
//! assert the exact SQL the dialect produces (backticks, MySQL types, upsert via
//! ON DUPLICATE KEY UPDATE, the FILTER->CASE emulation, and JSON operators).

use tork_orm_core::dialect::{render_expr, render_insert, MySqlDialect, PostgresDialect};
use tork_orm_core::migration::{Column, SchemaManager};
use tork_orm_core::query::expr::{AggFunc, BinaryOp};
use tork_orm_core::query::write::{Assignment, InsertStatement, OnConflict};
use tork_orm_core::{Expr, Value};

#[tokio::test]
async fn create_table_uses_backticks_and_mysql_types() {
    let dialect = MySqlDialect::new();
    let mut schema = SchemaManager::collect(&dialect);
    schema
        .create_table("users")
        .if_not_exists()
        .column(Column::new("id").bigint().primary_key().auto_increment())
        .column(Column::new("name").varchar(50).not_null())
        .column(Column::new("active").boolean().not_null())
        .column(Column::new("score").real())
        .column(Column::new("avatar").blob())
        .execute()
        .await
        .unwrap();

    assert_eq!(
        schema.into_collected()[0],
        "CREATE TABLE IF NOT EXISTS `users` (\
`id` BIGINT AUTO_INCREMENT PRIMARY KEY, \
`name` VARCHAR(50) NOT NULL, \
`active` TINYINT(1) NOT NULL, \
`score` DOUBLE, \
`avatar` BLOB)"
    );
}

#[test]
fn upsert_renders_on_duplicate_key_update() {
    let statement = InsertStatement {
        table: "accounts",
        columns: vec!["email", "balance"],
        rows: vec![vec![Value::Text("a@x.com".into()), Value::Int(100)]],
        returning: Vec::new(),
        on_conflict: OnConflict::Update {
            constraint: vec!["email"],
            updates: vec![Assignment::new("balance", Expr::excluded("balance"))],
        },
    };
    let (sql, _params) = render_insert(&MySqlDialect::new(), &statement);
    assert_eq!(
        sql,
        "INSERT INTO `accounts` (`email`, `balance`) VALUES (?, ?) \
ON DUPLICATE KEY UPDATE `balance` = VALUES(`balance`)"
    );
}

#[test]
fn do_nothing_renders_insert_ignore() {
    let statement = InsertStatement {
        table: "accounts",
        columns: vec!["email"],
        rows: vec![vec![Value::Text("a@x.com".into())]],
        returning: Vec::new(),
        on_conflict: OnConflict::DoNothing { constraint: vec!["email"] },
    };
    let (sql, _params) = render_insert(&MySqlDialect::new(), &statement);
    assert_eq!(sql, "INSERT IGNORE INTO `accounts` (`email`) VALUES (?)");
}

#[test]
fn aggregate_filter_is_emulated_with_case() {
    // COUNT(x) FILTER (WHERE active) — MySQL has no FILTER, so it becomes
    // COUNT(CASE WHEN active THEN x END).
    let aggregate = Expr::Aggregate {
        func: AggFunc::Count,
        args: vec![Expr::column("t", "x")],
        filter: Some(Box::new(Expr::column("t", "active"))),
    };
    let (mysql_sql, _) = render_expr(&MySqlDialect::new(), &aggregate);
    assert_eq!(mysql_sql, "COUNT(CASE WHEN `t`.`active` THEN `t`.`x` END)");

    // PostgreSQL keeps the native FILTER clause.
    let (pg_sql, _) = render_expr(&PostgresDialect::new(), &aggregate);
    assert_eq!(pg_sql, "COUNT(\"t\".\"x\") FILTER (WHERE \"t\".\"active\")");
}

#[test]
fn json_get_uses_mysql_path_and_postgres_key() {
    // `payload -> 'k'` (PostgreSQL) vs `payload -> '$.k'` (MySQL).
    let json_get = Expr::binary(
        Expr::column("t", "payload"),
        BinaryOp::JsonGet,
        Expr::value(Value::Text("k".into())),
    );
    let (mysql_sql, _) = render_expr(&MySqlDialect::new(), &json_get);
    assert_eq!(mysql_sql, "`t`.`payload` -> '$.k'");
    let (pg_sql, _) = render_expr(&PostgresDialect::new(), &json_get);
    assert_eq!(pg_sql, "\"t\".\"payload\" -> 'k'");
}

#[test]
fn json_contains_uses_function_on_mysql_and_operator_on_postgres() {
    let contains = Expr::binary(
        Expr::column("t", "payload"),
        BinaryOp::Contains,
        Expr::value(Value::Text("{\"vip\": true}".into())),
    );
    let (mysql_sql, _) = render_expr(&MySqlDialect::new(), &contains);
    assert_eq!(mysql_sql, "JSON_CONTAINS(`t`.`payload`, ?)");
    let (pg_sql, _) = render_expr(&PostgresDialect::new(), &contains);
    assert_eq!(pg_sql, "\"t\".\"payload\" @> $1");
}
