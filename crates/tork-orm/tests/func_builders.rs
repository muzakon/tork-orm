//! Tests for the function-builder ergonomics (column methods and free functions).

use tork_orm::dialect::{predicate_sql, SqliteDialect};
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    email: String,
    first_name: String,
    last_name: String,
}

#[test]
fn column_method_and_free_function_agree() {
    let dialect = SqliteDialect::new();
    let method = User::email.lower();
    let free = lower(User::email);
    assert_eq!(predicate_sql(&dialect, &method), "lower(\"users\".\"email\")");
    assert_eq!(
        predicate_sql(&dialect, &free),
        "lower(\"users\".\"email\")"
    );
}

#[test]
fn coalesce_and_generic_func() {
    let dialect = SqliteDialect::new();
    let expr = coalesce(User::first_name, User::last_name);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "coalesce(\"users\".\"first_name\", \"users\".\"last_name\")"
    );

    let custom = func("substr", [User::email.into(), Expr::value(Value::Int(1))]);
    assert_eq!(
        predicate_sql(&dialect, &custom),
        "substr(\"users\".\"email\", 1)"
    );
}

#[test]
fn function_predicate_compares() {
    let dialect = SqliteDialect::new();
    let expr = User::email.lower().eq("admin@x.com");
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "lower(\"users\".\"email\") = 'admin@x.com'"
    );
}
