//! The [`UnionQuery`] type — a `UNION` / `UNION ALL` combinator over `QuerySet`.

use std::marker::PhantomData;

use crate::dialect::render_union;
use crate::executor::Executor;
use crate::model::{FromRow, Model};
use crate::query::ast::{OrderTerm, UnionStatement};
use crate::query::queryset::QuerySet;

/// A typed `UNION` (or `UNION ALL`) query over model `M`.
///
/// Built from two [`QuerySet`]s with [`QuerySet::union`] or
/// [`QuerySet::union_all`]. Chainable: call `.union`/`.union_all` again to
/// append more branches. Supports the same ordering and pagination modifiers
/// as `QuerySet`, applied to the whole combined result.
///
/// # Examples
///
/// ```no_run
/// # use tork_orm_core::{Database, Model, Value};
/// # #[derive(Clone)] struct User;
/// # impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } }
/// # impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
/// # async fn run(db: Database) -> tork_orm_core::Result<()> {
/// let rows = User::query()
///     .filter(User::query().filter(tork_orm_core::Expr::Value(Value::Bool(true))).into_statement().filters.is_empty().then(|| tork_orm_core::Expr::Value(Value::Bool(true))).unwrap_or(tork_orm_core::Expr::Value(Value::Bool(false))))
///     .union(User::query())
///     .all(&db)
///     .await?;
/// # let _ = rows; Ok(())
/// # }
/// ```
pub struct UnionQuery<M: Model> {
    statement: UnionStatement,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Model> UnionQuery<M> {
    /// Creates a union of `first` and `second`.
    pub(crate) fn new(first: QuerySet<M>, second: QuerySet<M>, all: bool) -> Self {
        Self {
            statement: UnionStatement {
                first: first.into_statement(),
                rest: vec![(all, second.into_statement())],
                order_by: Vec::new(),
                limit: None,
                offset: None,
                lock: None,
            },
            _marker: PhantomData,
        }
    }

    /// Consumes this union and returns the underlying [`UnionStatement`].
    ///
    /// Useful for building CTE bodies in `WITH` clauses.
    pub fn into_statement(self) -> UnionStatement {
        self.statement
    }

    /// Appends another branch as `UNION` (distinct).
    pub fn union(mut self, other: QuerySet<M>) -> Self {
        self.statement.rest.push((false, other.into_statement()));
        self
    }

    /// Appends another branch as `UNION ALL` (preserves duplicates).
    pub fn union_all(mut self, other: QuerySet<M>) -> Self {
        self.statement.rest.push((true, other.into_statement()));
        self
    }

    /// Appends an ordering term applied to the whole combined result.
    pub fn order_by(mut self, term: OrderTerm) -> Self {
        self.statement.order_by.push(term);
        self
    }

    /// Limits the number of rows returned from the whole combined result.
    pub fn limit(mut self, limit: u64) -> Self {
        self.statement.limit = Some(limit);
        self
    }

    /// Skips the given number of leading rows from the whole combined result.
    pub fn offset(mut self, offset: u64) -> Self {
        self.statement.offset = Some(offset);
        self
    }

    /// Runs the union and returns every matching row as `M`.
    pub async fn all(self, executor: impl Executor) -> crate::Result<Vec<M>> {
        self.all_as::<M>(executor).await
    }

    /// Runs the union and maps each row into an arbitrary [`FromRow`] type.
    pub async fn all_as<T: FromRow>(self, executor: impl Executor) -> crate::Result<Vec<T>> {
        crate::query::queryset::validate_union_for_dialect(executor.dialect(), &self.statement)?;
        let (sql, params) = render_union(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Runs the union and returns the first row, if any.
    pub async fn first<E: Executor>(mut self, executor: E) -> crate::Result<Option<M>> {
        self.statement.limit = Some(1);
        crate::query::queryset::validate_union_for_dialect(executor.dialect(), &self.statement)?;
        let (sql, params) = render_union(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        match rows.first() {
            Some(row) => M::from_row(row).map(Some),
            None => Ok(None),
        }
    }

    /// Runs a `COUNT(*)` wrapping the whole union.
    pub async fn count<E: Executor>(self, executor: E) -> crate::Result<i64> {
        // Wrap the union in a derived table: SELECT COUNT(*) FROM (<union>) AS _u
        crate::query::queryset::validate_union_for_dialect(executor.dialect(), &self.statement)?;
        let (inner_sql, params) = render_union(executor.dialect(), &self.statement);
        let sql = format!("SELECT COUNT(*) FROM ({inner_sql}) AS _u");
        let rows = executor.fetch_all(sql, params).await?;
        match rows.first() {
            Some(row) => row.get_index::<i64>(0),
            None => Ok(0),
        }
    }
}
