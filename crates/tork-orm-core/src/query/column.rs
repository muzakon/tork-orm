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
        Expr::aggregate(AggFunc::Count, [self.expr()])
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

    /// `column IS DISTINCT FROM value` — NULL-safe inequality.
    ///
    /// Two NULLs are not distinct (equal), a NULL and a non-NULL are distinct.
    pub fn is_distinct_from<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::IsDistinctFrom,
            Expr::value(value.into_sql_value()),
        )
    }

    /// `column IS NOT DISTINCT FROM value` — NULL-safe equality.
    ///
    /// Two NULLs are not distinct (i.e. equal), a NULL and a non-NULL are distinct.
    pub fn is_not_distinct_from<V: IntoSqlValue<T>>(self, value: V) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::IsNotDistinctFrom,
            Expr::value(value.into_sql_value()),
        )
    }

    /// `string_agg(column, delimiter)` — concatenates non-null values with a delimiter.
    ///
    /// Only available for `String` columns; use the free function version for other
    /// types.
    pub fn string_aggregation(self, delimiter: &str) -> Expr
    where
        T: crate::value::BindValue,
    {
        Expr::aggregate(AggFunc::StringAggregation, [
            self.expr(),
            Expr::value(Value::Text(delimiter.to_string())),
        ])
    }

    /// `array_agg(column)` — collects values into a PostgreSQL array.
    pub fn array_aggregation(self) -> Expr
    where
        T: crate::value::BindValue,
    {
        Expr::aggregate(AggFunc::ArrayAggregation, [self.expr()])
    }

    /// `json_agg(column)` — aggregates values as a JSON array.
    pub fn json_aggregation(self) -> Expr
    where
        T: crate::value::BindValue,
    {
        Expr::aggregate(AggFunc::JsonAggregation, [self.expr()])
    }

    /// `jsonb_agg(column)` — aggregates values as a JSONB array.
    pub fn jsonb_aggregation(self) -> Expr
    where
        T: crate::value::BindValue,
    {
        Expr::aggregate(AggFunc::JsonbAggregation, [self.expr()])
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
        Expr::aggregate(AggFunc::Sum, [self.expr()])
    }

    /// `AVG(column)`.
    pub fn avg(self) -> Expr {
        Expr::aggregate(AggFunc::Avg, [self.expr()])
    }

    /// `MIN(column)`.
    pub fn min(self) -> Expr {
        Expr::aggregate(AggFunc::Min, [self.expr()])
    }

    /// `MAX(column)`.
    pub fn max(self) -> Expr {
        Expr::aggregate(AggFunc::Max, [self.expr()])
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

    /// `round(column)`.
    pub fn round(self) -> Expr {
        Expr::func("round", [self.expr()])
    }

    /// `ceil(column)`.
    pub fn ceil(self) -> Expr {
        Expr::func("ceil", [self.expr()])
    }

    /// `floor(column)`.
    pub fn floor(self) -> Expr {
        Expr::func("floor", [self.expr()])
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

    /// `substr(column, start)` — 1-based, returns from `start` to end.
    pub fn substr(self, start: i64) -> Expr {
        Expr::func("substr", [self.expr(), Expr::value(Value::Int(start))])
    }

    /// `substr(column, start, len)` — 1-based, returns `len` characters.
    pub fn substr_len(self, start: i64, len: i64) -> Expr {
        Expr::func(
            "substr",
            [
                self.expr(),
                Expr::value(Value::Int(start)),
                Expr::value(Value::Int(len)),
            ],
        )
    }

    /// `regexp_like(column, pattern)` — tests whether the column matches the regex pattern.
    pub fn regex_match(self, pattern: &str) -> Expr {
        Expr::func("regexp_like", [self.expr(), Expr::value(Value::Text(pattern.to_string()))])
    }

    /// `regexp_replace(column, pattern, replacement)` — replaces regex matches.
    pub fn regex_replace(self, pattern: &str, replacement: &str) -> Expr {
        Expr::func("regexp_replace", [
            self.expr(),
            Expr::value(Value::Text(pattern.to_string())),
            Expr::value(Value::Text(replacement.to_string())),
        ])
    }

    /// `split_part(column, delimiter, field)` — splits on `delimiter` and returns the
    /// `field`-th part (1-based).
    pub fn split_part(self, delimiter: &str, field: i64) -> Expr {
        Expr::func("split_part", [
            self.expr(),
            Expr::value(Value::Text(delimiter.to_string())),
            Expr::value(Value::Int(field)),
        ])
    }

    /// `replace(column, from, to)` — replaces all occurrences of `from` with `to`.
    pub fn replace(self, from: &str, to: &str) -> Expr {
        Expr::func("replace", [
            self.expr(),
            Expr::value(Value::Text(from.to_string())),
            Expr::value(Value::Text(to.to_string())),
        ])
    }

    /// `left(column, n)` — returns the first `n` characters.
    pub fn left(self, n: i64) -> Expr {
        Expr::func("left", [self.expr(), Expr::value(Value::Int(n))])
    }

    /// `right(column, n)` — returns the last `n` characters.
    pub fn right(self, n: i64) -> Expr {
        Expr::func("right", [self.expr(), Expr::value(Value::Int(n))])
    }

    /// `repeat(column, n)` — repeats the string `n` times.
    pub fn repeat(self, n: i64) -> Expr {
        Expr::func("repeat", [self.expr(), Expr::value(Value::Int(n))])
    }

    /// `reverse(column)` — reverses the string.
    pub fn reverse(self) -> Expr {
        Expr::func("reverse", [self.expr()])
    }

    /// `position(substring IN column)` — returns the position of the first occurrence.
    pub fn position(self, substring: &str) -> Expr {
        Expr::func("position", [
            Expr::value(Value::Text(substring.to_string())),
            self.expr(),
        ])
    }
}

/// Boolean column operations — additional aggregates.
impl<M> Column<M, bool> {
    /// `bool_and(column)` — true if every non-null value is true.
    pub fn bool_and(self) -> Expr {
        Expr::aggregate(AggFunc::BoolAnd, [self.expr()])
    }

    /// `bool_or(column)` — true if any non-null value is true.
    pub fn bool_or(self) -> Expr {
        Expr::aggregate(AggFunc::BoolOr, [self.expr()])
    }
}

/// JSON column operators (PostgreSQL `jsonb`).
impl<M> Column<M, serde_json::Value> {
    /// `column -> key` — extracts a JSON field by name, as JSON.
    pub fn json_get(self, key: &str) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::JsonGet,
            Expr::value(Value::Text(key.to_string())),
        )
    }

    /// `column ->> key` — extracts a JSON field by name, as text.
    ///
    /// Returns a text-valued expression; chain a comparison such as `.eq("active")`
    /// to use it as a filter.
    pub fn json_get_text(self, key: &str) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::JsonGetText,
            Expr::value(Value::Text(key.to_string())),
        )
    }

    /// `column @> value` — tests whether the JSON document contains `value`.
    pub fn json_contains(self, value: serde_json::Value) -> Expr {
        Expr::binary(self.expr(), BinaryOp::Contains, Expr::value(Value::Json(value)))
    }

    /// `column ? key` — does the JSON key exist as a top-level key?
    pub fn json_has_key(self, key: &str) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::JsonKeyExists,
            Expr::value(Value::Text(key.to_string())),
        )
    }

    /// `column ?| keys` — do any of the given keys exist as top-level keys?
    pub fn json_has_any(self, keys: &[&str]) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::JsonKeyExistsAny,
            Expr::value(Value::Array(keys.iter().map(|k| Value::Text(k.to_string())).collect())),
        )
    }

    /// `column ?& keys` — do all of the given keys exist as top-level keys?
    pub fn json_has_all(self, keys: &[&str]) -> Expr {
        Expr::binary(
            self.expr(),
            BinaryOp::JsonKeyExistsAll,
            Expr::value(Value::Array(keys.iter().map(|k| Value::Text(k.to_string())).collect())),
        )
    }

    /// `column #> path` — extracts a JSON sub-object at the given path, as JSON.
    ///
    /// The path is a `&[&str]` of key names, rendered as a PostgreSQL text array
    /// literal (e.g. `'{a,b}'`).
    pub fn json_path(self, path: &[&str]) -> Expr {
        let path_array: Vec<Value> = path.iter().map(|k| Value::Text(k.to_string())).collect();
        Expr::binary(
            self.expr(),
            BinaryOp::JsonPath,
            Expr::value(Value::Array(path_array)),
        )
    }

    /// `column #>> path` — extracts a JSON sub-object at the given path, as text.
    pub fn json_path_text(self, path: &[&str]) -> Expr {
        let path_array: Vec<Value> = path.iter().map(|k| Value::Text(k.to_string())).collect();
        Expr::binary(
            self.expr(),
            BinaryOp::JsonPathText,
            Expr::value(Value::Array(path_array)),
        )
    }
}

/// Array column operators (PostgreSQL `element[]`).
///
/// Bounding on `T: BindValue` excludes `Vec<u8>` blob columns (`u8` is not
/// `BindValue`), which therefore get none of these methods.
impl<M, T: BindValue> Column<M, Vec<T>> {
    /// `value = ANY(column)` — tests whether the array contains `value`.
    pub fn any(self, value: T) -> Expr {
        Expr::binary(
            Expr::value(value.to_value()),
            BinaryOp::Eq,
            Expr::func("ANY", [self.expr()]),
        )
    }

    /// `column @> items` — tests whether the array contains every one of `items`.
    pub fn array_contains(self, items: impl IntoIterator<Item = T>) -> Expr {
        let array = Value::Array(items.into_iter().map(|item| item.to_value()).collect());
        Expr::binary(self.expr(), BinaryOp::Contains, Expr::value(array))
    }

    /// `column && items` — tests whether the array overlaps `items` (shares any element).
    pub fn overlaps(self, items: impl IntoIterator<Item = T>) -> Expr {
        let array = Value::Array(items.into_iter().map(|item| item.to_value()).collect());
        Expr::binary(self.expr(), BinaryOp::Overlap, Expr::value(array))
    }

    /// `column <@ items` — tests whether the array is contained by `items`
    /// (every element of the column is in items).
    pub fn contained_by(self, items: impl IntoIterator<Item = T>) -> Expr {
        let array = Value::Array(items.into_iter().map(|item| item.to_value()).collect());
        Expr::binary(self.expr(), BinaryOp::ArrayContainedBy, Expr::value(array))
    }

    /// `array_append(column, value)` — appends an element to the array.
    pub fn array_append(self, value: T) -> Expr {
        Expr::func("array_append", [self.expr(), Expr::value(value.to_value())])
    }

    /// `array_remove(column, value)` — removes all occurrences of `value` from the array.
    pub fn array_remove(self, value: T) -> Expr {
        Expr::func("array_remove", [self.expr(), Expr::value(value.to_value())])
    }

    /// `array_position(column, value)` — returns the index of the first occurrence of `value` (1-based).
    pub fn array_position(self, value: T) -> Expr {
        Expr::func("array_position", [self.expr(), Expr::value(value.to_value())])
    }

    /// `array_length(column, dimension)` — returns the length of the array along the given dimension (1-based).
    pub fn array_length(self, dimension: i64) -> Expr {
        Expr::func("array_length", [self.expr(), Expr::value(Value::Int(dimension))])
    }
}

impl<M, T> Clone for Column<M, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, T> Copy for Column<M, T> {}
