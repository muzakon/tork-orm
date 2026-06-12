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

// ── round / ceil / floor ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Model)]
#[table(name = "products")]
struct Product {
    #[field(primary_key, auto)]
    id: i64,
    price: f64,
}

#[test]
fn round_ceil_floor_free_functions() {
    let dialect = SqliteDialect::new();
    assert_eq!(predicate_sql(&dialect, &round(Product::price)), "round(\"products\".\"price\")");
    assert_eq!(predicate_sql(&dialect, &ceil(Product::price)),  "ceil(\"products\".\"price\")");
    assert_eq!(predicate_sql(&dialect, &floor(Product::price)), "floor(\"products\".\"price\")");
}

#[test]
fn round_ceil_floor_column_sugar() {
    let dialect = SqliteDialect::new();
    assert_eq!(predicate_sql(&dialect, &Product::price.round()), "round(\"products\".\"price\")");
    assert_eq!(predicate_sql(&dialect, &Product::price.ceil()),  "ceil(\"products\".\"price\")");
    assert_eq!(predicate_sql(&dialect, &Product::price.floor()), "floor(\"products\".\"price\")");
}

// ── substr ────────────────────────────────────────────────────────────────────

#[test]
fn substr_two_arg_free_function() {
    let dialect = SqliteDialect::new();
    let expr = substr(User::email, Expr::value(Value::Int(2)));
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "substr(\"users\".\"email\", 2)"
    );
}

#[test]
fn substr_three_arg_free_function() {
    let dialect = SqliteDialect::new();
    let expr = substr_len(User::email, Expr::value(Value::Int(2)), Expr::value(Value::Int(5)));
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "substr(\"users\".\"email\", 2, 5)"
    );
}

#[test]
fn substr_column_sugar() {
    let dialect = SqliteDialect::new();
    assert_eq!(
        predicate_sql(&dialect, &User::email.substr(2)),
        "substr(\"users\".\"email\", 2)"
    );
    assert_eq!(
        predicate_sql(&dialect, &User::email.substr_len(2, 5)),
        "substr(\"users\".\"email\", 2, 5)"
    );
}

// ── concat ────────────────────────────────────────────────────────────────────

#[test]
fn concat_variadic() {
    let dialect = SqliteDialect::new();
    let expr = concat([Expr::from(User::first_name), Expr::from(User::last_name)]);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "concat(\"users\".\"first_name\", \"users\".\"last_name\")"
    );
}
