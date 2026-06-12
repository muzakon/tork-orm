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

// ── NULLIF / GREATEST / LEAST ─────────────────────────────────────────────────

#[test]
fn nullif_free_function() {
    let dialect = SqliteDialect::new();
    let expr = nullif(User::first_name, User::last_name);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "nullif(\"users\".\"first_name\", \"users\".\"last_name\")"
    );
}

#[test]
fn greatest_free_function() {
    let dialect = SqliteDialect::new();
    let expr = greatest([User::id.into(), Expr::value(Value::Int(0))]);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "greatest(\"users\".\"id\", 0)"
    );
}

#[test]
fn least_free_function() {
    let dialect = SqliteDialect::new();
    let expr = least([User::id.into(), Expr::value(Value::Int(100))]);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "least(\"users\".\"id\", 100)"
    );
}

// ── RANDOM ────────────────────────────────────────────────────────────────────

#[test]
fn random_value_renders() {
    let dialect = SqliteDialect::new();
    assert_eq!(predicate_sql(&dialect, &random_value()), "random()");
}

// ── REGEX / SPLIT / REPLACE FREE FUNCTIONS ────────────────────────────────────

#[test]
fn replace_free_function() {
    let dialect = SqliteDialect::new();
    let expr = replace(User::email, "example", "test");
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "replace(\"users\".\"email\", 'example', 'test')"
    );
}

#[test]
fn position_free_function() {
    let dialect = SqliteDialect::new();
    let expr = position("@", User::email);
    assert_eq!(predicate_sql(&dialect, &expr), "position('@', \"users\".\"email\")");
}

// ── POSTGRES-SPECIFIC FREE FUNCTIONS ──────────────────────────────────────────
// These functions are only available when the `postgres` feature is enabled.

#[cfg(feature = "postgres")]
#[test]
fn regex_match_free_function() {
    let dialect = SqliteDialect::new();
    let expr = regex_match(User::email, "^admin@");
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "regexp_like(\"users\".\"email\", '^admin@')"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn regex_replace_free_function() {
    let dialect = SqliteDialect::new();
    let expr = regex_replace(User::email, "@old\\.com", "@new.com");
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "regexp_replace(\"users\".\"email\", '@old\\.com', '@new.com')"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn split_part_free_function() {
    let dialect = SqliteDialect::new();
    let expr = split_part(User::email, "@", 2);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "split_part(\"users\".\"email\", '@', 2)"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn left_free_function() {
    let dialect = SqliteDialect::new();
    let expr = left(User::email, 3);
    assert_eq!(predicate_sql(&dialect, &expr), "left(\"users\".\"email\", 3)");
}

#[cfg(feature = "postgres")]
#[test]
fn right_free_function() {
    let dialect = SqliteDialect::new();
    let expr = right(User::email, 5);
    assert_eq!(predicate_sql(&dialect, &expr), "right(\"users\".\"email\", 5)");
}

#[cfg(feature = "postgres")]
#[test]
fn repeat_free_function() {
    let dialect = SqliteDialect::new();
    let expr = repeat(User::email, 2);
    assert_eq!(predicate_sql(&dialect, &expr), "repeat(\"users\".\"email\", 2)");
}

#[cfg(feature = "postgres")]
#[test]
fn reverse_free_function() {
    let dialect = SqliteDialect::new();
    let expr = reverse(User::email);
    assert_eq!(predicate_sql(&dialect, &expr), "reverse(\"users\".\"email\")");
}

// ── AGGREGATE FREE FUNCTIONS (PostgreSQL-specific) ────────────────────────────

#[cfg(feature = "postgres")]
#[test]
fn string_aggregation_free_function() {
    let dialect = SqliteDialect::new();
    let expr = string_aggregation(User::email, ",");
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "string_agg(\"users\".\"email\", ',')"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn array_aggregation_free_function() {
    let dialect = SqliteDialect::new();
    let expr = array_aggregation(User::id);
    assert_eq!(predicate_sql(&dialect, &expr), "array_agg(\"users\".\"id\")");
}

#[cfg(feature = "postgres")]
#[test]
fn json_aggregation_free_function() {
    let dialect = SqliteDialect::new();
    let expr = json_aggregation(User::email);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "json_agg(\"users\".\"email\")"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn jsonb_aggregation_free_function() {
    let dialect = SqliteDialect::new();
    let expr = jsonb_aggregation(User::email);
    assert_eq!(
        predicate_sql(&dialect, &expr),
        "jsonb_agg(\"users\".\"email\")"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn bool_and_free_function() {
    #[derive(Debug, Clone, Model)]
    #[table(name = "flags")]
    struct Flag {
        #[field(primary_key, auto)]
        id: i64,
        active: bool,
    }
    let dialect = SqliteDialect::new();
    let expr = bool_and(Flag::active);
    assert_eq!(predicate_sql(&dialect, &expr), "bool_and(\"flags\".\"active\")");
}

#[cfg(feature = "postgres")]
#[test]
fn bool_or_free_function() {
    #[derive(Debug, Clone, Model)]
    #[table(name = "flags")]
    struct Flag {
        #[field(primary_key, auto)]
        id: i64,
        active: bool,
    }
    let dialect = SqliteDialect::new();
    let expr = bool_or(Flag::active);
    assert_eq!(predicate_sql(&dialect, &expr), "bool_or(\"flags\".\"active\")");
}
