//! Free-function builders for scalar SQL functions.
//!
//! These complement the [`Column`](crate::Column) methods (`col.lower()`) with a
//! call form (`lower(col)`) and a generic escape hatch ([`func`]) for any function
//! the built-ins do not cover. Each argument is anything convertible into an
//! [`Expr`], so a column, a value, or another function call all work.

use crate::query::expr::Expr;
use crate::Value;

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

/// `replace(column, from, to)` — replaces all occurrences of `from` with `to`.
pub fn replace(column: impl Into<Expr>, from: &str, to: &str) -> Expr {
    Expr::func("replace", [
        column.into(),
        Expr::value(crate::value::Value::Text(from.to_string())),
        Expr::value(crate::value::Value::Text(to.to_string())),
    ])
}

/// `position(substring IN column)` — returns the position of the first occurrence.
pub fn position(substring: &str, column: impl Into<Expr>) -> Expr {
    Expr::func("position", [
        Expr::value(crate::value::Value::Text(substring.to_string())),
        column.into(),
    ])
}

// ---------------------------------------------------------------------------
// Date / Time functions
// ---------------------------------------------------------------------------

/// `CURRENT_TIMESTAMP` — returns the current date and time at the session
/// time zone.
///
/// Standard SQL. Use `.over().end()` to mark a window function call.
pub fn current_timestamp() -> Expr {
    Expr::raw("CURRENT_TIMESTAMP")
}

/// `CURRENT_DATE` — returns the current date at the session time zone.
pub fn current_date() -> Expr {
    Expr::raw("CURRENT_DATE")
}

/// `CURRENT_TIME` — returns the current time at the session time zone.
pub fn current_time() -> Expr {
    Expr::raw("CURRENT_TIME")
}

/// `NOW()` — returns the current date and time (equivalent to
/// `CURRENT_TIMESTAMP`).
pub fn now() -> Expr {
    Expr::func("NOW", [] as [Expr; 0])
}

/// `EXTRACT(field FROM source)` — retrieves a sub-field such as `YEAR`,
/// `MONTH`, `DAY`, `HOUR`, `MINUTE`, `SECOND` from a date/time expression.
///
/// Standard SQL. Available in SQLite and PostgreSQL alike.
pub fn extract(field: &str, source: impl Into<Expr>) -> Expr {
    Expr::Extract {
        field: field.to_string(),
        source: Box::new(source.into()),
    }
}

/// `date_trunc(field, source)` — truncates a timestamp to the specified
/// precision (`year`, `month`, `day`, `hour`, `minute`, `second`, etc.).
///
/// PostgreSQL-specific.
#[cfg(feature = "postgres")]
pub fn date_trunc(field: &str, source: impl Into<Expr>) -> Expr {
    Expr::func("date_trunc", [
        Expr::value(crate::value::Value::Text(field.to_string())),
        source.into(),
    ])
}

/// `AGE(end, start)` — computes `end - start` as an interval.
///
/// PostgreSQL-specific.
#[cfg(feature = "postgres")]
pub fn age(end: impl Into<Expr>, start: impl Into<Expr>) -> Expr {
    Expr::func("AGE", [end.into(), start.into()])
}

/// `TO_CHAR(source, format)` — formats a timestamp according to the given
/// format string.
///
/// Format patterns follow PostgreSQL's `to_char` conventions.
///
/// PostgreSQL-specific.
#[cfg(feature = "postgres")]
pub fn to_char(source: impl Into<Expr>, format: &str) -> Expr {
    Expr::func("TO_CHAR", [
        source.into(),
        Expr::value(crate::value::Value::Text(format.to_string())),
    ])
}

/// `timezone(zone, expr)` — converts a timestamp to the target time zone.
///
/// Equivalent to the `AT TIME ZONE` SQL syntax. PostgreSQL-specific.
#[cfg(feature = "postgres")]
pub fn at_time_zone(zone: &str, expr: impl Into<Expr>) -> Expr {
    Expr::func("timezone", [
        Expr::value(crate::value::Value::Text(zone.to_string())),
        expr.into(),
    ])
}

// ---------------------------------------------------------------------------
// Window functions — pure functions designed for OVER()
// ---------------------------------------------------------------------------

/// `ROW_NUMBER()` — assigns a unique sequential integer to each row within a
/// partition. Typically used with `.over().order_by(...)`.
pub fn row_number() -> Expr {
    Expr::func("ROW_NUMBER", [] as [Expr; 0])
}

/// `RANK()` — ranks rows with gaps: rows with equal values in the `ORDER BY`
/// get the same rank, and the next rank is skipped.
pub fn rank() -> Expr {
    Expr::func("RANK", [] as [Expr; 0])
}

/// `DENSE_RANK()` — ranks rows without gaps: equal values share a rank, and
/// the next rank follows immediately.
pub fn dense_rank() -> Expr {
    Expr::func("DENSE_RANK", [] as [Expr; 0])
}

/// `NTILE(n)` — divides rows into `n` buckets as evenly as possible.
pub fn ntile(n: i64) -> Expr {
    Expr::func("NTILE", [Expr::value(crate::value::Value::Int(n))])
}

/// `LAG(expr [, offset [, default]])` — accesses the value from the previous
/// row within the partition.
pub fn lag(expr: impl Into<Expr>) -> Expr {
    Expr::func("LAG", [expr.into()])
}

/// `LAG(expr, offset)` — accesses the value `offset` rows before the current row.
pub fn lag_offset(expr: impl Into<Expr>, offset: i64) -> Expr {
    Expr::func("LAG", [expr.into(), Expr::value(crate::value::Value::Int(offset))])
}

/// `LAG(expr, offset, default)` — with a fallback when there is no preceding row.
pub fn lag_default(expr: impl Into<Expr>, offset: i64, default: impl Into<Expr>) -> Expr {
    Expr::func("LAG", [
        expr.into(),
        Expr::value(crate::value::Value::Int(offset)),
        default.into(),
    ])
}

/// `LEAD(expr)` — accesses the value from the next row within the partition.
pub fn lead(expr: impl Into<Expr>) -> Expr {
    Expr::func("LEAD", [expr.into()])
}

/// `LEAD(expr, offset)` — accesses the value `offset` rows after the current row.
pub fn lead_offset(expr: impl Into<Expr>, offset: i64) -> Expr {
    Expr::func("LEAD", [expr.into(), Expr::value(crate::value::Value::Int(offset))])
}

/// `LEAD(expr, offset, default)` — with a fallback when there is no following row.
pub fn lead_default(expr: impl Into<Expr>, offset: i64, default: impl Into<Expr>) -> Expr {
    Expr::func("LEAD", [
        expr.into(),
        Expr::value(crate::value::Value::Int(offset)),
        default.into(),
    ])
}

/// `FIRST_VALUE(expr)` — returns the value from the first row of the window frame.
pub fn first_value(expr: impl Into<Expr>) -> Expr {
    Expr::func("FIRST_VALUE", [expr.into()])
}

/// `LAST_VALUE(expr)` — returns the value from the last row of the window frame.
pub fn last_value(expr: impl Into<Expr>) -> Expr {
    Expr::func("LAST_VALUE", [expr.into()])
}

/// `NTH_VALUE(expr, n)` — returns the value from the n-th row of the window
/// frame (1-based).
pub fn nth_value(expr: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("NTH_VALUE", [expr.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `PERCENT_RANK()` — returns the relative rank of the current row: (rank - 1) /
/// (total rows - 1).
pub fn percent_rank() -> Expr {
    Expr::func("PERCENT_RANK", [] as [Expr; 0])
}

/// `CUME_DIST()` — cumulative distribution: the number of rows preceding or peer
/// to the current row divided by the total number of rows.
pub fn cume_dist() -> Expr {
    Expr::func("CUME_DIST", [] as [Expr; 0])
}

// ---------------------------------------------------------------------------
// PostgreSQL-specific functions
// ---------------------------------------------------------------------------

/// `regexp_like(column, pattern)` — tests whether the column matches the regex pattern.
#[cfg(feature = "postgres")]
pub fn regex_match(column: impl Into<Expr>, pattern: &str) -> Expr {
    Expr::func("regexp_like", [column.into(), Expr::value(crate::value::Value::Text(pattern.to_string()))])
}

/// `regexp_replace(column, pattern, replacement)` — replaces regex matches.
#[cfg(feature = "postgres")]
pub fn regex_replace(column: impl Into<Expr>, pattern: &str, replacement: &str) -> Expr {
    Expr::func("regexp_replace", [
        column.into(),
        Expr::value(crate::value::Value::Text(pattern.to_string())),
        Expr::value(crate::value::Value::Text(replacement.to_string())),
    ])
}

/// `split_part(column, delimiter, field)` — splits on `delimiter` and returns the
/// `field`-th part (1-based).
#[cfg(feature = "postgres")]
pub fn split_part(column: impl Into<Expr>, delimiter: &str, field: i64) -> Expr {
    Expr::func("split_part", [
        column.into(),
        Expr::value(crate::value::Value::Text(delimiter.to_string())),
        Expr::value(crate::value::Value::Int(field)),
    ])
}

/// `left(column, n)` — returns the first `n` characters.
#[cfg(feature = "postgres")]
pub fn left(column: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("left", [column.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `right(column, n)` — returns the last `n` characters.
#[cfg(feature = "postgres")]
pub fn right(column: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("right", [column.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `repeat(column, n)` — repeats the string `n` times.
#[cfg(feature = "postgres")]
pub fn repeat(column: impl Into<Expr>, n: i64) -> Expr {
    Expr::func("repeat", [column.into(), Expr::value(crate::value::Value::Int(n))])
}

/// `reverse(column)` — reverses the string.
#[cfg(feature = "postgres")]
pub fn reverse(column: impl Into<Expr>) -> Expr {
    Expr::func("reverse", [column.into()])
}

/// `string_agg(column, delimiter)` — concatenates non-null values with a delimiter.
#[cfg(feature = "postgres")]
pub fn string_aggregation(column: impl Into<Expr>, delimiter: &str) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::StringAggregation, [
        column.into(),
        Expr::value(crate::value::Value::Text(delimiter.to_string())),
    ])
}

/// `array_agg(column)` — collects values into a PostgreSQL array.
#[cfg(feature = "postgres")]
pub fn array_aggregation(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::ArrayAggregation, [column.into()])
}

/// `json_agg(column)` — aggregates values as a JSON array.
#[cfg(feature = "postgres")]
pub fn json_aggregation(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::JsonAggregation, [column.into()])
}

/// `jsonb_agg(column)` — aggregates values as a JSONB array.
#[cfg(feature = "postgres")]
pub fn jsonb_aggregation(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::JsonbAggregation, [column.into()])
}

/// `bool_and(column)` — true if every non-null value is true.
#[cfg(feature = "postgres")]
pub fn bool_and(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::BoolAnd, [column.into()])
}

/// `bool_or(column)` — true if any non-null value is true.
#[cfg(feature = "postgres")]
pub fn bool_or(column: impl Into<Expr>) -> Expr {
    Expr::aggregate(crate::query::expr::AggFunc::BoolOr, [column.into()])
}

// ---------------------------------------------------------------------------
// Full-text search (PostgreSQL)
// ---------------------------------------------------------------------------

/// `to_tsvector(config, text)` — converts plain text into a `tsvector`
/// for full-text search.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "postgres")] {
/// use tork_orm_core::query::func::{to_tsvector, to_tsquery};
/// use tork_orm_core::query::expr::Expr;
///
/// let v = to_tsvector("english", Expr::column("articles", "body"));
/// let q = to_tsquery("english", Expr::value(tork_orm_core::Value::Text("search & terms".into())));
/// # let _ = v;
/// # let _ = q;
/// # }
/// ```
#[cfg(feature = "postgres")]
pub fn to_tsvector(config: &str, text: impl Into<Expr>) -> Expr {
    Expr::func("to_tsvector", [Expr::value(Value::Text(config.to_string())), text.into()])
}

/// `to_tsvector_simple(text)` — `to_tsvector('simple', text)` shorthand.
#[cfg(feature = "postgres")]
pub fn to_tsvector_simple(text: impl Into<Expr>) -> Expr {
    to_tsvector("simple", text)
}

/// `to_tsquery(config, query)` — parses a lexeme-based query into a `tsquery`.
#[cfg(feature = "postgres")]
pub fn to_tsquery(config: &str, query_text: impl Into<Expr>) -> Expr {
    Expr::func("to_tsquery", [Expr::value(Value::Text(config.to_string())), query_text.into()])
}

/// `plainto_tsquery(config, query)` — parses plain text into a `tsquery`
/// (automatically adds AND between terms).
#[cfg(feature = "postgres")]
pub fn plainto_tsquery(config: &str, query_text: impl Into<Expr>) -> Expr {
    Expr::func("plainto_tsquery", [Expr::value(Value::Text(config.to_string())), query_text.into()])
}

/// `phraseto_tsquery(config, query)` — parses phrase text into a `tsquery`
/// (terms must appear consecutively).
#[cfg(feature = "postgres")]
pub fn phraseto_tsquery(config: &str, query_text: impl Into<Expr>) -> Expr {
    Expr::func("phraseto_tsquery", [Expr::value(Value::Text(config.to_string())), query_text.into()])
}

/// `ts_rank(vector, query)` — ranks documents by relevance.
#[cfg(feature = "postgres")]
pub fn ts_rank(vector: impl Into<Expr>, query: impl Into<Expr>) -> Expr {
    Expr::func("ts_rank", [vector.into(), query.into()])
}

/// `ts_rank_cd(vector, query)` — ranks documents using coverage density.
#[cfg(feature = "postgres")]
pub fn ts_rank_cd(vector: impl Into<Expr>, query: impl Into<Expr>) -> Expr {
    Expr::func("ts_rank_cd", [vector.into(), query.into()])
}

/// `ts_headline(config, text, query)` — generates a highlighted excerpt.
#[cfg(feature = "postgres")]
pub fn ts_headline(
    config: &str,
    text: impl Into<Expr>,
    query: impl Into<Expr>,
) -> Expr {
    Expr::func(
        "ts_headline",
        [
            Expr::value(Value::Text(config.to_string())),
            text.into(),
            query.into(),
        ],
    )
}

/// `tsquery(query_text)` — casts a string literal to `tsquery`.
#[cfg(feature = "postgres")]
pub fn tsquery(query_text: &str) -> Expr {
    Expr::func("tsquery", [Expr::value(Value::Text(query_text.to_string()))])
}
