//! Tests for scalar function expressions.

use tork_orm_core::dialect::{predicate_sql, render_expr, SqliteDialect};
use tork_orm_core::query::expr::Expr;
use tork_orm_core::Value;

#[test]
fn function_renders_inline() {
    let dialect = SqliteDialect::new();
    let expr = Expr::func("lower", [Expr::column("users", "email")]);
    assert_eq!(predicate_sql(&dialect, &expr), "lower(\"users\".\"email\")");
}

#[test]
fn nested_and_multi_arg_functions() {
    let dialect = SqliteDialect::new();
    let expr = Expr::func(
        "coalesce",
        [
            Expr::func("upper", [Expr::column("t", "a")]),
            Expr::column("t", "b"),
        ],
    );
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "coalesce(upper(\"t\".\"a\"), \"t\".\"b\")"
    );
}

#[test]
fn function_argument_binds_in_query_mode() {
    let dialect = SqliteDialect::new();
    // A function over a bound value uses a placeholder outside inline mode.
    let expr = Expr::func("lower", [Expr::value(Value::Text("HELLO".into()))]);
    let (sql, params) = render_expr(&dialect, &expr);
    assert_eq!(sql, "lower(?)");
    assert_eq!(params, vec![Value::Text("HELLO".into())]);
}

#[test]
fn function_expression_compares_in_predicate() {
    let dialect = SqliteDialect::new();
    let expr = Expr::func("lower", [Expr::column("users", "email")]).eq("admin@x.com");
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "lower(\"users\".\"email\") = 'admin@x.com'"
    );
}
