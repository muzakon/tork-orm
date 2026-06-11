//! Tests for typed column expressions: the generated handles, the comparison and
//! logical builders, and how they render to SQL with bound parameters.

use tork_orm::dialect::{render_expr, SqliteDialect};
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50))]
    username: String,
    email: String,
    is_active: bool,
    nickname: Option<String>,
}

/// Renders an expression for SQLite and returns the SQL plus its bound values.
fn render(expr: &Expr) -> (String, Vec<Value>) {
    render_expr(&SqliteDialect::new(), expr)
}

#[test]
fn equality_binds_the_value() {
    let (sql, params) = render(&User::is_active.eq(true));
    assert_eq!(sql, "\"users\".\"is_active\" = ?");
    assert_eq!(params, vec![Value::Bool(true)]);
}

#[test]
fn string_columns_accept_a_string_slice() {
    let (sql, params) = render(&User::username.eq("alice"));
    assert_eq!(sql, "\"users\".\"username\" = ?");
    assert_eq!(params, vec![Value::Text("alice".into())]);
}

#[test]
fn integer_literal_comparison_infers_cleanly() {
    // The literal needs no type annotation despite the generic comparison method.
    let (sql, params) = render(&User::id.gt(3));
    assert_eq!(sql, "\"users\".\"id\" > ?");
    assert_eq!(params, vec![Value::Int(3)]);
}

#[test]
fn ordering_operators_render() {
    assert_eq!(render(&User::id.ge(1)).0, "\"users\".\"id\" >= ?");
    assert_eq!(render(&User::id.le(9)).0, "\"users\".\"id\" <= ?");
    assert_eq!(render(&User::id.lt(9)).0, "\"users\".\"id\" < ?");
    assert_eq!(render(&User::id.ne(0)).0, "\"users\".\"id\" <> ?");
}

#[test]
fn any_renders_an_or_group() {
    let predicate = Expr::any([
        User::username.eq("alice"),
        User::email.eq("alice@example.com"),
    ]);
    let (sql, params) = render(&predicate);
    assert_eq!(
        sql,
        "(\"users\".\"username\" = ? OR \"users\".\"email\" = ?)"
    );
    assert_eq!(
        params,
        vec![
            Value::Text("alice".into()),
            Value::Text("alice@example.com".into()),
        ]
    );
}

#[test]
fn all_renders_an_and_group() {
    let predicate = Expr::all([User::is_active.eq(true), User::id.gt(10)]);
    let (sql, _) = render(&predicate);
    assert_eq!(
        sql,
        "(\"users\".\"is_active\" = ? AND \"users\".\"id\" > ?)"
    );
}

#[test]
fn not_wraps_its_operand() {
    let (sql, _) = render(&Expr::not(User::is_active.eq(true)));
    assert_eq!(sql, "NOT (\"users\".\"is_active\" = ?)");
}

#[test]
fn nested_and_or_compose() {
    // is_active AND (username = ? OR email = ?)
    let predicate = Expr::all([
        User::is_active.eq(true),
        Expr::any([
            User::username.eq("alice"),
            User::email.eq("alice@example.com"),
        ]),
    ]);
    let (sql, params) = render(&predicate);
    assert_eq!(
        sql,
        "(\"users\".\"is_active\" = ? AND (\"users\".\"username\" = ? OR \"users\".\"email\" = ?))"
    );
    assert_eq!(params.len(), 3);
}

#[test]
fn in_list_binds_each_value() {
    let (sql, params) = render(&User::id.in_list([1, 2, 3]));
    assert_eq!(sql, "\"users\".\"id\" IN (?, ?, ?)");
    assert_eq!(params, vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
}

#[test]
fn empty_in_list_is_always_false() {
    let empty: [i64; 0] = [];
    let (sql, params) = render(&User::id.in_list(empty));
    assert_eq!(sql, "0 = 1");
    assert!(params.is_empty());
}

#[test]
fn null_tests_render() {
    assert_eq!(
        render(&User::nickname.is_null()).0,
        "\"users\".\"nickname\" IS NULL"
    );
    assert_eq!(
        render(&User::nickname.is_not_null()).0,
        "\"users\".\"nickname\" IS NOT NULL"
    );
}

#[test]
fn nullable_column_compares_against_inner_type() {
    // `nickname` is `Option<String>`, but the handle is typed on `String`, so it
    // takes a plain string slice.
    let (sql, params) = render(&User::nickname.eq("ace"));
    assert_eq!(sql, "\"users\".\"nickname\" = ?");
    assert_eq!(params, vec![Value::Text("ace".into())]);
}

// ── LIKE / ILIKE ─────────────────────────────────────────────────────────────

#[test]
fn like_renders_binary_with_pattern_bound() {
    let (sql, params) = render(&User::username.like("ali%"));
    assert_eq!(sql, "\"users\".\"username\" LIKE ?");
    assert_eq!(params, vec![Value::Text("ali%".into())]);
}

#[test]
fn ilike_renders_as_lower_both_sides() {
    let (sql, params) = render(&User::username.ilike("ALICE"));
    assert_eq!(sql, "lower(\"users\".\"username\") LIKE lower(?)");
    assert_eq!(params, vec![Value::Text("ALICE".into())]);
}

#[test]
fn like_on_optional_string_column() {
    let (sql, params) = render(&User::nickname.like("%ace%"));
    assert_eq!(sql, "\"users\".\"nickname\" LIKE ?");
    assert_eq!(params, vec![Value::Text("%ace%".into())]);
}

// ── BETWEEN ──────────────────────────────────────────────────────────────────

#[test]
fn between_renders_with_two_bound_params() {
    let (sql, params) = render(&User::id.between(1_i64, 10_i64));
    assert_eq!(sql, "\"users\".\"id\" BETWEEN ? AND ?");
    assert_eq!(params, vec![Value::Int(1), Value::Int(10)]);
}

#[test]
fn between_on_string_column() {
    let (sql, params) = render(&User::username.between("a", "m"));
    assert_eq!(sql, "\"users\".\"username\" BETWEEN ? AND ?");
    assert_eq!(params, vec![Value::Text("a".into()), Value::Text("m".into())]);
}

// ── NOT IN ────────────────────────────────────────────────────────────────────

#[test]
fn not_in_renders_as_not_over_in_list() {
    let (sql, params) = render(&User::id.not_in([1_i64, 2, 3]));
    assert_eq!(sql, "NOT (\"users\".\"id\" IN (?, ?, ?))");
    assert_eq!(params, vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
}

#[test]
fn not_in_empty_list_is_always_true() {
    let empty: [i64; 0] = [];
    let (sql, params) = render(&User::id.not_in(empty));
    // NOT (0 = 1) — always true
    assert_eq!(sql, "NOT (0 = 1)");
    assert!(params.is_empty());
}

#[test]
fn not_in_string_slice() {
    let (sql, params) = render(&User::username.not_in(["alice", "bob"]));
    assert_eq!(sql, "NOT (\"users\".\"username\" IN (?, ?))");
    assert_eq!(
        params,
        vec![Value::Text("alice".into()), Value::Text("bob".into())]
    );
}
