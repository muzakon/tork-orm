//! Free-function builders for scalar SQL functions.
//!
//! These complement the [`Column`](crate::Column) methods (`col.lower()`) with a
//! call form (`lower(col)`) and a generic escape hatch ([`func`]) for any function
//! the built-ins do not cover. Each argument is anything convertible into an
//! [`Expr`], so a column, a value, or another function call all work.

use crate::query::expr::Expr;

/// Builds a call to an arbitrary scalar function, `name(args...)`.
///
/// The escape hatch when no dedicated helper exists. The name is emitted verbatim.
///
/// # Examples
///
/// ```
/// use tork_orm_core::query::func::func;
/// use tork_orm_core::query::expr::Expr;
///
/// let expr = func("date", [Expr::column("events", "created_at")]);
/// # let _ = expr;
/// ```
pub fn func<I>(name: &str, args: I) -> Expr
where
    I: IntoIterator,
    I::Item: Into<Expr>,
{
    Expr::func(name, args.into_iter().map(Into::into))
}

/// `lower(arg)`.
pub fn lower(arg: impl Into<Expr>) -> Expr {
    Expr::func("lower", [arg.into()])
}

/// `upper(arg)`.
pub fn upper(arg: impl Into<Expr>) -> Expr {
    Expr::func("upper", [arg.into()])
}

/// `length(arg)`.
pub fn length(arg: impl Into<Expr>) -> Expr {
    Expr::func("length", [arg.into()])
}

/// `trim(arg)`.
pub fn trim(arg: impl Into<Expr>) -> Expr {
    Expr::func("trim", [arg.into()])
}

/// `abs(arg)`.
pub fn abs(arg: impl Into<Expr>) -> Expr {
    Expr::func("abs", [arg.into()])
}

/// `coalesce(first, second)`.
pub fn coalesce(first: impl Into<Expr>, second: impl Into<Expr>) -> Expr {
    Expr::func("coalesce", [first.into(), second.into()])
}

/// `round(arg)` — rounds to the nearest integer.
pub fn round(arg: impl Into<Expr>) -> Expr {
    Expr::func("round", [arg.into()])
}

/// `ceil(arg)` — smallest integer not less than the argument.
pub fn ceil(arg: impl Into<Expr>) -> Expr {
    Expr::func("ceil", [arg.into()])
}

/// `floor(arg)` — largest integer not greater than the argument.
pub fn floor(arg: impl Into<Expr>) -> Expr {
    Expr::func("floor", [arg.into()])
}

/// `substr(arg, start)` — 1-based substring from `start` to the end.
pub fn substr(arg: impl Into<Expr>, start: impl Into<Expr>) -> Expr {
    Expr::func("substr", [arg.into(), start.into()])
}

/// `substr(arg, start, len)` — 1-based substring of exactly `len` characters.
pub fn substr_len(arg: impl Into<Expr>, start: impl Into<Expr>, len: impl Into<Expr>) -> Expr {
    Expr::func("substr", [arg.into(), start.into(), len.into()])
}

/// `concat(args...)` — concatenates two or more strings.
pub fn concat<I>(args: I) -> Expr
where
    I: IntoIterator,
    I::Item: Into<Expr>,
{
    Expr::func("concat", args.into_iter().map(Into::into))
}

/// `nullif(a, b)` — returns NULL when `a` equals `b`, otherwise `a`.
pub fn nullif(a: impl Into<Expr>, b: impl Into<Expr>) -> Expr {
    Expr::func("nullif", [a.into(), b.into()])
}

/// `greatest(a, b, ...)` — returns the largest value from the list.
pub fn greatest<I>(args: I) -> Expr
where
    I: IntoIterator,
    I::Item: Into<Expr>,
{
    Expr::func("greatest", args.into_iter().map(Into::into))
}

/// `least(a, b, ...)` — returns the smallest value from the list.
pub fn least<I>(args: I) -> Expr
where
    I: IntoIterator,
    I::Item: Into<Expr>,
{
    Expr::func("least", args.into_iter().map(Into::into))
}

/// `random()` — returns a random value between 0.0 and 1.0.
pub fn random_value() -> Expr {
    Expr::func("random", [] as [Expr; 0])
}

/// `regexp_like(column, pattern)` — tests whether the column matches the regex pattern.
pub fn regex_match(column: impl Into<Expr>, pattern: &str) -> Expr {
    Expr::func("regexp_like", [column.into(), Expr::value(crate::value::Value::Text(pattern.to_string()))])
}

/// `regexp_replace(column, pattern, replacement)` — replaces regex matches.
pub fn regex_replace(column: impl Into<Expr>, pattern: &str, replacement: &str) -> Expr {
    Expr::func("regexp_replace", [
        column.into(),
        Expr::value(crate::value::Value::Text(pattern.to_string())),
        Expr::value(crate::value::Value::Text(replacement.to_string())),
    ])
}

/// `split_part(column, delimiter, field)` — splits on `delimiter` and returns the
/// `field`-th part (1-based).
pub fn split_part(column: impl Into<Expr>, delimiter: &str, field: i64) -> Expr {
    Expr::func("split_part", [
        column.into(),
        Expr::value(crate::value::Value::Text(delimiter.to_string())),
        Expr::value(crate::value::Value::Int(field)),
    ])
}

/// `replace(column, from, to)` — replaces all occurrences of `from` with `to`.
pub fn replace(column: impl Into<Expr>, from: &str, to: &str) -> Expr {
    Expr::func("replace", [
        column.into(),
        Expr::value(crate::value::Value::Text(from.to_string())),
        Expr::value(crate::value::Value::Text(to.to_string())),
    ])
}

/// `left(column, n)` — returns the first `n` characters.
pub fn left(column: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("left", [column.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `right(column, n)` — returns the last `n` characters.
pub fn right(column: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("right", [column.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `repeat(column, n)` — repeats the string `n` times.
pub fn repeat(column: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("repeat", [column.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `reverse(column)` — reverses the string.
pub fn reverse(column: impl Into<Expr>) -> Expr {
    Expr::func("reverse", [column.into()])
}

/// `position(substring IN column)` — returns the position of the first occurrence.
pub fn position(substring: &str, column: impl Into<Expr>) -> Expr {
    Expr::func("position", [
        Expr::value(crate::value::Value::Text(substring.to_string())),
        column.into(),
    ])
}

/// `string_agg(column, delimiter)` — concatenates non-null values with a delimiter.
pub fn string_aggregation(column: impl Into<Expr>, delimiter: &str) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::StringAggregation, [
        column.into(),
        Expr::value(crate::value::Value::Text(delimiter.to_string())),
    ])
}

/// `array_agg(column)` — collects values into a PostgreSQL array.
pub fn array_aggregation(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::ArrayAggregation, [column.into()])
}

/// `json_agg(column)` — aggregates values as a JSON array.
pub fn json_aggregation(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::JsonAggregation, [column.into()])
}

/// `jsonb_agg(column)` — aggregates values as a JSONB array.
pub fn jsonb_aggregation(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::JsonbAggregation, [column.into()])
}

/// `bool_and(column)` — true if every non-null value is true.
pub fn bool_and(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::BoolAnd, [column.into()])
}

/// `bool_or(column)` — true if any non-null value is true.
pub fn bool_or(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::BoolOr, [column.into()])
}
