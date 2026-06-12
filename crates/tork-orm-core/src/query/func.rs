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
