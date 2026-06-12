//! Typed column handles.
//!
//! `#[derive(Model)]` generates one [`Column`] associated constant per field, so a
//! column is used as a value: `User::is_active`. Its comparison methods build
//! [`Expr`] nodes whose right-hand side is a bound parameter, and the column's Rust
//! type constrains what it can be compared against, so a type mismatch is a compile
//! error rather than a run-time surprise.

use std::marker::PhantomData;

use crate::query::ast::OrderItem;
use crate::query::expr::{AggFunc, BinaryOp, Expr};
use crate::query::queryset::QuerySet;
use crate::query::write::Assignment;
use crate::value::{BindValue, Value};

/// Accepted by [`Column::set`] — either a type-safe literal (`col.set(42_i64)`)
/// or any expression (`col.set(col.add(1_i64))`).
pub trait IntoAssignExpr<T> {
    /// Converts `self` into an assignment expression.
    fn into_assign_expr(self) -> Expr;
}

macro_rules! impl_assign_literal {
    ($t:ty) => {
        impl IntoAssignExpr<$t> for $t {
            fn into_assign_expr(self) -> Expr {
                Expr::value(self.to_value())
            }
        }
        impl IntoAssignExpr<Option<$t>> for $t {
            fn into_assign_expr(self) -> Expr {
                Expr::value(self.to_value())
            }
        }
    };
}

impl_assign_literal!(i64);
impl_assign_literal!(i32);
impl_assign_literal!(f64);
impl_assign_literal!(bool);
impl_assign_literal!(String);

// &str → String column
impl IntoAssignExpr<String> for &str {
    fn into_assign_expr(self) -> Expr {
        Expr::value(Value::Text(self.to_string()))
    }
}

// Value directly — useful for NULL assignments on nullable columns
impl<T> IntoAssignExpr<T> for Value {
    fn into_assign_expr(self) -> Expr {
        Expr::value(self)
    }
}

// Expr — the escape hatch; accepts any column type
impl<T> IntoAssignExpr<T> for Expr {
    fn into_assign_expr(self) -> Expr {
        self
    }
}

/// Marker for column types that support numeric aggregates (`sum`, `avg`, `min`,
/// `max`).
pub trait Numeric {}

impl Numeric for i64 {}
impl Numeric for i32 {}
impl Numeric for f64 {}

/// Converts a comparison right-hand side into a bound [`Value`], constrained by the
/// column's type `T`.
///
/// There are exactly two ways to satisfy it: any `T` that is itself a
/// [`BindValue`] (the identity case), and a `&str` when the column type is
/// `String`. Keeping the set small avoids the ambiguity a blanket `Into<T>` bound
/// would create for integer literals, while still letting `column.eq("text")` take
/// a string slice.
pub trait IntoSqlValue<T> {
    /// Lowers `self` into a bound value.
    fn into_sql_value(self) -> Value;
}

impl<T: BindValue> IntoSqlValue<T> for T {
    fn into_sql_value(self) -> Value {
        self.to_value()
    }
}

impl IntoSqlValue<String> for &str {
    fn into_sql_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

/// A typed reference to a model column.
///
/// `M` is the owning model and `T` is the column's Rust type. The handle is
/// zero-sized beyond two `&'static str`s and is `Copy`, so passing it by value is
/// free.
///
/// # Examples
///
/// ```
/// use tork_orm_core::Column;
///
/// struct User;
/// const IS_ACTIVE: Column<User, bool> = Column::new("users", "is_active");
/// let predicate = IS_ACTIVE.eq(true);
/// # let _ = predicate;
/// ```
pub struct Column<M, T> {
    table: &'static str,
    name: &'static str,
    // `fn() -> (M, T)` keeps `Column` `Send`, `Sync`, and `Copy` regardless of the
    // model and column types, which are only used to constrain comparisons.
    _marker: PhantomData<fn() -> (M, T)>,
}

impl<M, T> Column<M, T> {
    /// Creates a column handle for `table`.`name`.
    pub const fn new(table: &'static str, name: &'static str) -> Self {
        Self {
            table,
            name,
            _marker: PhantomData,
        }
    }

    /// Returns the owning table name.
    pub fn table(&self) -> &'static str {
        self.table
    }

    /// Returns the column name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns this column as an expression node.
    pub fn expr(&self) -> Expr {
        Expr::column(self.table, self.name)
    }

    /// `column = value`
    pub fn eq<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.compare(BinaryOp::Eq, value)
    }

    /// `column <> value`
    pub fn ne<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.compare(BinaryOp::Ne, value)
    }

    /// `column > value`
    pub fn gt<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.compare(BinaryOp::Gt, value)
    }

    /// `column >= value`
    pub fn ge<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.compare(BinaryOp::Ge, value)
    }

    /// `column < value`
    pub fn lt<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.compare(BinaryOp::Lt, value)
    }

    /// `column <= value`
    pub fn le<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.compare(BinaryOp::Le, value)
    }

    /// `column IN (values...)`
    pub fn in_list<V, I>(self, values: I) -> Expr
    where
        V: IntoSqlValue<T>,
        I: IntoIterator<Item = V>,
    {
        let values = values.into_iter().map(IntoSqlValue::into_sql_value).collect();
        Expr::in_list(self.expr(), values)
    }

    /// `column NOT IN (values...)`
    ///
    /// An empty iterator matches all rows (SQL: `NOT (0 = 1)` = always true).
    pub fn not_in<V, I>(self, values: I) -> Expr
    where
        V: IntoSqlValue<T>,
        I: IntoIterator<Item = V>,
    {
        Expr::not(self.in_list(values))
    }

    /// `column IN (SELECT ...)` — matches rows where the column value appears in
    /// the subquery result. The subquery must return a single column.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// Post::query()
    ///     .filter(Post::user_id.in_subquery(
    ///         User::query().filter(User::is_active.eq(true)).select((User::id,)),
    ///     ))
    ///     .all(&db)
    ///     .await?
    /// ```
    pub fn in_subquery<X: crate::model::Model>(self, qs: QuerySet<X>) -> Expr {
        Expr::in_subquery(self.expr(), qs.into_statement(), false)
    }

    /// `column NOT IN (SELECT ...)` — excludes rows where the column value
    /// appears in the subquery result.
    pub fn not_in_subquery<X: crate::model::Model>(self, qs: QuerySet<X>) -> Expr {
        Expr::in_subquery(self.expr(), qs.into_statement(), true)
    }

    /// `column BETWEEN low AND high` (inclusive on both ends).
    pub fn between<V: IntoSqlValue<T>>(self, low: V, high: V) -> Expr {
        Expr::between(
            self.expr(),
            Expr::value(low.into_sql_value()),
            Expr::value(high.into_sql_value()),
        )
    }

    /// `column IS NULL`
    pub fn is_null(self) -> Expr {
        Expr::is_null(self.expr(), false)
    }

    /// `column IS NOT NULL`
    pub fn is_not_null(self) -> Expr {
        Expr::is_null(self.expr(), true)
    }

    /// Orders by this column ascending.
    pub fn asc(self) -> OrderItem {
        OrderItem::new(self.expr(), false)
    }

    /// Orders by this column descending.
    pub fn desc(self) -> OrderItem {
        OrderItem::new(self.expr(), true)
    }

    /// Builds a `SET column = …` assignment for use with
    /// [`QuerySet::update`](crate::QuerySet::update).
    ///
    /// Accepts either a type-safe literal or any [`Expr`]:
    /// ```ignore
    /// Post::title.set("New Title")                           // literal
    /// Post::view_count.set(Post::view_count.add(1_i64))      // expression
    /// ```
    pub fn set<A: IntoAssignExpr<T>>(self, value: A) -> Assignment {
        Assignment::new(self.name, value.into_assign_expr())
    }

    /// Aliases this column in a projection, `column AS "alias"`.
    pub fn as_(self, alias: &'static str) -> Expr {
        self.expr().as_(alias)
    }

    /// `COUNT(column)` for use in a projection or `HAVING`.
    pub fn count(self) -> Expr {
        Expr::aggregate(AggFunc::Count, self.expr())
    }

    /// `lower(column)`.
    pub fn lower(self) -> Expr {
        Expr::func("lower", [self.expr()])
    }

    /// `upper(column)`.
    pub fn upper(self) -> Expr {
        Expr::func("upper", [self.expr()])
    }

    /// `length(column)`.
    pub fn length(self) -> Expr {
        Expr::func("length", [self.expr()])
    }

    /// `trim(column)`.
    pub fn trim(self) -> Expr {
        Expr::func("trim", [self.expr()])
    }

    /// `abs(column)`.
    pub fn abs(self) -> Expr {
        Expr::func("abs", [self.expr()])
    }

    /// Builds a binary comparison against a bound value.
    fn compare<V: IntoSqlValue<T>>(self, op: BinaryOp, value: V) -> Expr {
        Expr::binary(self.expr(), op, Expr::value(value.into_sql_value()))
    }
}

impl<M, T> From<Column<M, T>> for Expr {
    fn from(column: Column<M, T>) -> Self {
        column.expr()
    }
}

impl<M, T: Numeric> Column<M, T> {
    /// `SUM(column)`.
    pub fn sum(self) -> Expr {
        Expr::aggregate(AggFunc::Sum, self.expr())
    }

    /// `AVG(column)`.
    pub fn avg(self) -> Expr {
        Expr::aggregate(AggFunc::Avg, self.expr())
    }

    /// `MIN(column)`.
    pub fn min(self) -> Expr {
        Expr::aggregate(AggFunc::Min, self.expr())
    }

    /// `MAX(column)`.
    pub fn max(self) -> Expr {
        Expr::aggregate(AggFunc::Max, self.expr())
    }

    /// `column + value`
    pub fn add<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.expr().add(Expr::value(value.into_sql_value()))
    }

    /// `column - value`
    pub fn sub<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.expr().sub(Expr::value(value.into_sql_value()))
    }

    /// `column * value`
    pub fn mul<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.expr().mul(Expr::value(value.into_sql_value()))
    }

    /// `column / value`
    pub fn div<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.expr().div(Expr::value(value.into_sql_value()))
    }

    /// `column % value`
    pub fn rem<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        self.expr().rem(Expr::value(value.into_sql_value()))
    }
}

impl<M> Column<M, String> {
    /// `column LIKE pattern`
    ///
    /// Wildcards (`%` for any sequence, `_` for a single character) are the
    /// caller's responsibility and are passed through unchanged.
    pub fn like(self, pattern: &str) -> Expr {
        Expr::binary(
            self.expr(),
            crate::query::expr::BinaryOp::Like,
            Expr::value(Value::Text(pattern.to_string())),
        )
    }

    /// `column ILIKE pattern` — case-insensitive LIKE.
    ///
    /// On SQLite this is rendered as `lower(col) LIKE lower(pattern)`, which
    /// covers non-ASCII Unicode correctly. On backends that have a native
    /// `ILIKE` keyword it is rendered verbatim.
    pub fn ilike(self, pattern: &str) -> Expr {
        Expr::binary(
            self.expr(),
            crate::query::expr::BinaryOp::ILike,
            Expr::value(Value::Text(pattern.to_string())),
        )
    }

    /// `column LIKE 'prefix%'` — matches rows where the column starts with `prefix`.
    pub fn starts_with(self, prefix: &str) -> Expr {
        self.like(&format!("{prefix}%"))
    }

    /// `column LIKE '%suffix'` — matches rows where the column ends with `suffix`.
    pub fn ends_with(self, suffix: &str) -> Expr {
        self.like(&format!("%{suffix}"))
    }

    /// `column LIKE '%needle%'` — matches rows where the column contains `needle`.
    pub fn contains(self, needle: &str) -> Expr {
        self.like(&format!("%{needle}%"))
    }

    /// Case-insensitive `starts_with`.
    pub fn istarts_with(self, prefix: &str) -> Expr {
        self.ilike(&format!("{prefix}%"))
    }

    /// Case-insensitive `ends_with`.
    pub fn iends_with(self, suffix: &str) -> Expr {
        self.ilike(&format!("%{suffix}"))
    }

    /// Case-insensitive `contains`.
    pub fn icontains(self, needle: &str) -> Expr {
        self.ilike(&format!("%{needle}%"))
    }
}

impl<M, T> Clone for Column<M, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, T> Copy for Column<M, T> {}
