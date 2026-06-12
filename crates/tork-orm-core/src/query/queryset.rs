//! The query builder.
//!
//! A [`QuerySet`] accumulates a [`SelectStatement`] through chainable builders and
//! runs it through an [`Executor`](crate::Executor) with a terminal method. The
//! builders mirror a readable, filter-first style: `filter` adds an `AND`
//! predicate, `filter_any` adds an `OR` group, and so on.

use std::marker::PhantomData;

use crate::dialect::{render_count, render_delete, render_exists, render_select, render_update};
use crate::error::OrmError;
use crate::executor::Executor;
use crate::model::{FromRow, Model};
use crate::query::ast::{JoinKind, OrderItem, SelectItem, SelectStatement};
use crate::query::column::Column;
use crate::query::expr::{BinaryOp, Expr};
use crate::query::write::{Assignment, DeleteStatement, UpdateStatement};

/// A typed query over a model `M`.
///
/// # Examples
///
/// ```no_run
/// use tork_orm_core::{Database, Model};
/// # use tork_orm_core::{Row, Value};
/// # struct User;
/// # impl tork_orm_core::FromRow for User {
/// #     fn from_row(_: &Row) -> tork_orm_core::Result<Self> { Ok(User) }
/// # }
/// # impl Model for User {
/// #     const TABLE: &'static str = "users";
/// #     const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[];
/// #     const PRIMARY_KEY: &'static str = "id";
/// #     fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] }
/// #     fn primary_key_value(&self) -> Value { Value::Null }
/// # }
/// # async fn run(db: Database) -> tork_orm_core::Result<()> {
/// let users = User::query().limit(20).all(&db).await?;
/// # let _ = users;
/// # Ok(())
/// # }
/// ```
pub struct QuerySet<M: Model> {
    statement: SelectStatement,
    _marker: PhantomData<fn() -> M>,
}

/// A page of results returned by [`QuerySet::paginate`] and
/// [`QuerySet::paginate_as`].
///
/// Carries the items for the current page together with pagination metadata so
/// callers can render page links and "X of Y" summaries without additional
/// queries.
///
/// # Examples
///
/// ```no_run
/// # use tork_orm_core::query::Page;
/// # struct User;
/// let page = Page::<User> {
///     items: vec![],
///     total: 0,
///     page: 1,
///     page_size: 20,
///     pages: 0,
/// };
/// # let _ = page;
/// ```
#[derive(Debug, Clone)]
pub struct Page<T> {
    /// The rows on this page.
    pub items: Vec<T>,
    /// The total number of matching rows across all pages.
    pub total: i64,
    /// The current page number (1-based).
    pub page: u64,
    /// The number of items per page.
    pub page_size: u64,
    /// The total number of pages (`ceil(total / page_size)`).
    pub pages: u64,
}

impl<M: Model> QuerySet<M> {
    /// Builds a query selecting every column of `M` from its table.
    pub fn new() -> Self {
        let projection = M::COLUMNS
            .iter()
            .map(|column| SelectItem::Column {
                table: M::TABLE,
                column: column.name,
            })
            .collect();
        Self {
            statement: SelectStatement::new(M::TABLE, projection),
            _marker: PhantomData,
        }
    }

    /// Adds a predicate joined with `AND`.
    pub fn filter(mut self, predicate: Expr) -> Self {
        self.statement.filters.push(predicate);
        self
    }

    /// Adds a raw SQL predicate joined with `AND`.
    ///
    /// Write `?` for each bound value; params are passed as any `BindValue`
    /// (plain Rust values — no `Value::` wrapping required).
    ///
    /// For SQL without `?` placeholders, pass an empty params iterator or use
    /// [`Expr::raw`] as a filter value directly.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # struct User;
    /// # impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } }
    /// # impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let users = User::query()
    ///     .filter_raw("LENGTH(username) > ?", [5_i64])
    ///     .all(&db)
    ///     .await?;
    /// # let _ = users; Ok(())
    /// # }
    /// ```
    pub fn filter_raw<V, I>(mut self, sql: impl Into<String>, params: I) -> Self
    where
        V: crate::value::BindValue,
        I: IntoIterator<Item = V>,
    {
        let raw_params = params.into_iter().map(|v| v.to_value()).collect();
        self.statement.filters.push(Expr::Raw { sql: sql.into(), params: raw_params });
        self
    }

    /// Adds an `AND (a OR b OR ...)` group from several predicates.
    pub fn filter_any(mut self, predicates: impl IntoIterator<Item = Expr>) -> Self {
        self.statement.filters.push(Expr::any(predicates));
        self
    }

    /// Adds an `AND (a AND b AND ...)` group from several predicates.
    pub fn filter_all(mut self, predicates: impl IntoIterator<Item = Expr>) -> Self {
        self.statement.filters.push(Expr::all(predicates));
        self
    }

    /// Adds an `AND NOT (...)` of a predicate.
    pub fn filter_not(mut self, predicate: Expr) -> Self {
        self.statement.filters.push(Expr::not(predicate));
        self
    }

    /// Joins a related table for filtering or aggregation (`INNER JOIN`).
    ///
    /// The join does not load the related rows; use it to filter the queried
    /// model by columns on the related table. A `has_many` join can repeat parent
    /// rows, so pair it with [`distinct`](Self::distinct) when selecting parents.
    /// Only a relation defined on `M` is accepted, so the join is type-checked.
    pub fn join<C>(mut self, relation: crate::relation::Relation<M, C>) -> Self {
        self.statement.joins.push(relation.join_node());
        self
    }

    /// Left-joins a related table (`LEFT JOIN`).
    ///
    /// Unlike [`join`](Self::join), rows from `M` are included even when no
    /// matching row exists on the related side. Unmatched columns from the
    /// related table are `NULL`. Useful for optional relations such as computing
    /// post counts for all users including those with zero posts.
    pub fn left_join<C>(mut self, relation: crate::relation::Relation<M, C>) -> Self {
        self.statement
            .joins
            .push(relation.join_node_with_kind(JoinKind::Left));
        self
    }

    /// Right-joins a related table (`RIGHT JOIN`).
    ///
    /// All rows from the related side are included; unmatched rows from `M`
    /// produce `NULL` on the left. The inverse of `left_join`.
    ///
    /// Not supported by SQLite before 3.39; available in the AST for future
    /// backends and SQLite 3.39+.
    pub fn right_join<C>(mut self, relation: crate::relation::Relation<M, C>) -> Self {
        self.statement
            .joins
            .push(relation.join_node_with_kind(JoinKind::Right));
        self
    }

    /// Full-outer-joins a related table (`FULL OUTER JOIN`).
    ///
    /// Rows from both sides are always included; unmatched columns are `NULL`.
    ///
    /// Not supported by SQLite before 3.39; available in the AST for future
    /// backends and SQLite 3.39+.
    pub fn full_join<C>(mut self, relation: crate::relation::Relation<M, C>) -> Self {
        self.statement
            .joins
            .push(relation.join_node_with_kind(JoinKind::Full));
        self
    }

    /// Cross-joins a table (`CROSS JOIN`).
    ///
    /// Produces the cartesian product of every row in `M` with every row in
    /// `C`. No `ON` condition is needed or used. Useful for pairing small
    /// dimension tables (e.g. a sizes × colors grid). Use with care on large
    /// tables — the result set grows as *|M| × |C|*.
    ///
    /// Unlike the other join methods, `cross_join` does not take a `Relation`
    /// argument because no foreign key is involved. The target model `C` is
    /// named as a type parameter:
    ///
    /// ```ignore
    /// Size::query().cross_join::<Color>()
    /// ```
    pub fn cross_join<C: crate::model::Model>(mut self) -> Self {
        self.statement.joins.push(crate::query::ast::Join {
            kind: JoinKind::Cross,
            table: C::TABLE,
            left_table: "",
            left_column: "",
            right_table: "",
            right_column: "",
        });
        self
    }

    /// Preloads a related table in a separate, N+1-free query.
    ///
    /// Switches to a [`Preloader`](crate::preload::Preloader); the parents come
    /// back wrapped in [`Preloaded`](crate::preload::Preloaded). Constrain the
    /// related rows on the relation itself with `Relation::filter`/`order_by`.
    pub fn preload<C: Model>(
        self,
        relation: crate::relation::Relation<M, C>,
    ) -> crate::preload::Preloader<M> {
        crate::preload::Preloader::new(self).preload(relation)
    }

    /// Adds an ordering term (build it with `Column::asc`/`Column::desc`).
    pub fn order_by(mut self, term: OrderItem) -> Self {
        self.statement.order_by.push(term);
        self
    }

    /// Limits the number of returned rows.
    pub fn limit(mut self, limit: u64) -> Self {
        self.statement.limit = Some(limit);
        self
    }

    /// Skips the given number of leading rows.
    pub fn offset(mut self, offset: u64) -> Self {
        self.statement.offset = Some(offset);
        self
    }

    /// Returns only distinct rows.
    pub fn distinct(mut self) -> Self {
        self.statement.distinct = true;
        self
    }

    /// Ensures the query returns zero rows by adding a never-true filter
    /// (`0 = 1`). Useful for composing with `union` when you need an empty
    /// branch, or as a base that only produces rows once filters are applied.
    pub fn none(mut self) -> Self {
        self.statement.filters.push(Expr::binary(
            Expr::value(crate::value::Value::Int(0)),
            BinaryOp::Eq,
            Expr::value(crate::value::Value::Int(1)),
        ));
        self
    }

    /// Locks the selected rows with `FOR UPDATE` so no other transaction can
    /// modify or lock them until the current transaction commits.
    ///
    /// Only supported on backends with row-level locking (PostgreSQL, MySQL,
    /// SQLite 3.54+). Has no effect on read-only connections.
    pub fn for_update(mut self) -> Self {
        self.statement.for_update = true;
        self
    }

    /// Replaces the projection with a tuple of columns and expressions.
    ///
    /// Pair with [`all_as`](Self::all_as) and a `#[derive(QueryResult)]` DTO whose
    /// fields match the projected names (alias aggregates with
    /// [`Expr::as_`](crate::Expr::as_) so they have a stable name).
    pub fn select<P: crate::query::projection::Projection>(mut self, projection: P) -> Self {
        self.statement.projection = projection.into_select_items();
        self
    }

    /// Groups rows by a tuple of columns and expressions.
    pub fn group_by<G: crate::query::projection::ExprTuple>(mut self, group: G) -> Self {
        self.statement.group_by = group.into_exprs();
        self
    }

    /// Restricts grouped rows by a predicate (typically over an aggregate).
    pub fn having(mut self, predicate: Expr) -> Self {
        self.statement.having = Some(predicate);
        self
    }

    /// Returns the statement this query has assembled.
    ///
    /// Useful for inspecting or rendering a query without running it.
    pub fn statement(&self) -> &SelectStatement {
        &self.statement
    }

    /// Consumes this query set and returns the underlying statement.
    ///
    /// Primarily used internally to build subquery expressions. For most cases,
    /// prefer [`to_subquery`](Self::to_subquery) which wraps the result.
    pub fn into_statement(self) -> SelectStatement {
        self.statement
    }

    /// Converts this query into a scalar subquery expression `(SELECT ...)`.
    ///
    /// The result can be used in any expression position: a comparison, a
    /// projection, or a `HAVING` clause. For `IN (SELECT ...)` tests use
    /// [`Column::in_subquery`](crate::Column::in_subquery).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Scalar: posts with above-average view count
    /// let avg = Post::query()
    ///     .select(Post::view_count.avg().as_("avg"))
    ///     .to_subquery();
    ///
    /// let popular = Post::query()
    ///     .filter(Expr::binary(Post::view_count.expr(), BinaryOp::Gt, avg))
    ///     .all(&db)
    ///     .await?;
    /// ```
    pub fn to_subquery(self) -> crate::query::expr::Expr {
        crate::query::expr::Expr::subquery(self.statement)
    }

    /// Combines this query with `other` as `UNION` (removes duplicates).
    ///
    /// Returns a [`UnionQuery`](crate::query::UnionQuery) that can be extended
    /// with further branches and supports the same terminals (`all`, `first`,
    /// `count`) as `QuerySet`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let rows = User::query()
    ///     .filter(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar))
    ///     .union(User::query())
    ///     .all(&db).await?;
    /// # let _ = rows; Ok(())
    /// # }
    /// ```
    pub fn union(self, other: QuerySet<M>) -> crate::query::union::UnionQuery<M> {
        crate::query::union::UnionQuery::new(self, other, false)
    }

    /// Combines this query with `other` as `UNION ALL` (preserves duplicates).
    pub fn union_all(self, other: QuerySet<M>) -> crate::query::union::UnionQuery<M> {
        crate::query::union::UnionQuery::new(self, other, true)
    }

    /// Runs the query and returns every matching row as `M`.
    pub async fn all(self, executor: impl Executor) -> crate::Result<Vec<M>> {
        self.all_as::<M>(executor).await
    }

    /// Runs the query and maps each row into an arbitrary [`FromRow`] type.
    ///
    /// Only the result type needs naming: `all_as::<UserPostStats>(&db)`.
    pub async fn all_as<T: FromRow>(self, executor: impl Executor) -> crate::Result<Vec<T>> {
        let (sql, params) = render_select(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Runs the query and returns the first matching row, if any.
    pub async fn first<E: Executor>(mut self, executor: E) -> crate::Result<Option<M>> {
        self.statement.limit = Some(1);
        let (sql, params) = render_select(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        match rows.first() {
            Some(row) => M::from_row(row).map(Some),
            None => Ok(None),
        }
    }

    /// Runs the query expecting exactly one row.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) when no row
    /// matches and [`ErrorKind::MultipleFound`](crate::ErrorKind::MultipleFound)
    /// when more than one does.
    pub async fn one<E: Executor>(mut self, executor: E) -> crate::Result<M> {
        // Fetch two rows so a second match can be detected and reported.
        self.statement.limit = Some(2);
        let (sql, params) = render_select(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        match rows.len() {
            0 => Err(OrmError::not_found(format!(
                "no row in `{}` matched the query",
                M::TABLE
            ))),
            1 => M::from_row(&rows[0]),
            _ => Err(OrmError::multiple_found(format!(
                "more than one row in `{}` matched the query",
                M::TABLE
            ))),
        }
    }

    /// Runs the query expecting zero or one row.
    ///
    /// Returns `Ok(None)` when no row matches and `Ok(Some(m))` when exactly one
    /// does. Errors with [`ErrorKind::MultipleFound`] when more than one row
    /// matches — unlike [`first`](Self::first) which silently truncates.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::MultipleFound`](crate::ErrorKind::MultipleFound) when
    /// more than one row matches the query.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let user: Option<User> = User::query()
    ///     .filter(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar))
    ///     .one_or_none(&db)
    ///     .await?;
    /// # let _ = user; Ok(())
    /// # }
    /// ```
    pub async fn one_or_none<E: Executor>(
        mut self,
        executor: E,
    ) -> crate::Result<Option<M>> {
        // Fetch two rows so a second match can be detected.
        self.statement.limit = Some(2);
        let (sql, params) = render_select(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        match rows.len() {
            0 => Ok(None),
            1 => M::from_row(&rows[0]).map(Some),
            _ => Err(OrmError::multiple_found(format!(
                "more than one row in `{}` matched the query",
                M::TABLE
            ))),
        }
    }

    /// Runs a `COUNT(*)` over the query's filters.
    pub async fn count<E: Executor>(self, executor: E) -> crate::Result<i64> {
        let (sql, params) = render_count(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        match rows.first() {
            Some(row) => row.get_index::<i64>(0),
            None => Ok(0),
        }
    }

    /// Fetches a page of results.
    ///
    /// Runs a count query and a select query with the appropriate `LIMIT` /
    /// `OFFSET`. The page number is 1-based — passing `0` is treated as page 1.
    /// Returns a [`Page`] containing the items and pagination metadata.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value, query::Page};
    /// # struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let page: Page<User> = User::query()
    ///     .order_by(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::OrderItem::new(tork_orm_core::Expr::Value(Value::Int(0)), false)).unwrap_or(tork_orm_core::OrderItem::new(tork_orm_core::Expr::Value(Value::Int(0)), false)))
    ///     .paginate(&db, 1, 20)
    ///     .await?;
    /// println!("Page {} of {} ({} items)", page.page, page.pages, page.items.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn paginate<E: Executor>(
        self,
        executor: E,
        page: u64,
        page_size: u64,
    ) -> crate::Result<Page<M>> {
        self.paginate_as::<M, E>(executor, page, page_size).await
    }

    /// Fetches a page of results mapped to a custom [`FromRow`] type.
    ///
    /// Like [`paginate`](Self::paginate) but returns a `Page<T>` instead of
    /// `Page<M>`. Useful for paginated projections.
    pub async fn paginate_as<T: FromRow, E: Executor>(
        self,
        executor: E,
        page: u64,
        page_size: u64,
    ) -> crate::Result<Page<T>> {
        let page = page.max(1);
        let page_size = page_size.max(1);

        // Count total matching rows.
        let (count_sql, count_params) = render_count(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(count_sql, count_params).await?;
        let total: i64 = match rows.first() {
            Some(row) => row.get_index::<i64>(0)?,
            None => 0,
        };

        // Compute page count and clamp page.
        let pages = if total == 0 {
            1
        } else {
            (total as u64).div_ceil(page_size)
        };
        let page = page.min(pages);

        // Fetch the items for this page.
        let offset = (page - 1) * page_size;
        let mut statement = self.statement;
        statement.limit = Some(page_size);
        statement.offset = Some(offset);

        let (sql, params) = render_select(executor.dialect(), &statement);
        let rows = executor.fetch_all(sql, params).await?;
        let items: Vec<T> = rows.iter().map(T::from_row).collect::<crate::Result<_>>()?;

        Ok(Page {
            items,
            total,
            page,
            page_size,
            pages,
        })
    }

    /// Returns whether any row matches the query's filters.
    pub async fn exists<E: Executor>(self, executor: E) -> crate::Result<bool> {
        let (sql, params) = render_exists(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        match rows.first() {
            Some(row) => row.get_index::<bool>(0),
            None => Ok(false),
        }
    }

    /// Returns all matching rows in batches of `size`, using offset-based
    /// chunking. The last batch may be smaller than `size`.
    ///
    /// All batches are loaded eagerly (no server-side cursor). Useful for
    /// processing large result sets in a memory-constrained pipeline without
    /// loading everything at once.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let batches = User::query().chunk(&db, 100).await?;
    /// for batch in batches {
    ///     for user in batch {
    ///         println!("{:?}", user.primary_key_value());
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chunk<E: Executor>(
        self,
        executor: E,
        size: u64,
    ) -> crate::Result<Vec<Vec<M>>> {
        let size = size.max(1);
        let mut batches = Vec::new();
        let mut offset = 0u64;

        loop {
            let mut batch_stmt = self.statement.clone();
            batch_stmt.limit = Some(size);
            batch_stmt.offset = Some(offset);
            let (sql, params) = render_select(executor.dialect(), &batch_stmt);
            let rows = executor.fetch_all(sql, params).await?;
            if rows.is_empty() {
                break;
            }
            let batch: Vec<M> = rows.iter().map(M::from_row).collect::<crate::Result<_>>()?;
            let batch_len = batch.len() as u64;
            batches.push(batch);
            if batch_len < size {
                break;
            }
            offset += size;
        }

        Ok(batches)
    }

    /// Extracts a single column's values from every matching row.
    ///
    /// Replaces the projection with the given column and returns a flat `Vec`
    /// of its values — useful for building ID lists, dropdown options, or
    /// simple lookups without defining a DTO.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let names: Vec<String> = User::query()
    ///     .filter(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar))
    ///     .pluck(&db, User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Column::new("users", "username")).unwrap_or(tork_orm_core::Column::new("users", "username")))
    ///     .await?;
    /// # let _ = names; Ok(())
    /// # }
    /// ```
    pub async fn pluck<T: crate::value::FromValue, E: Executor>(
        mut self,
        executor: E,
        column: Column<M, T>,
    ) -> crate::Result<Vec<T>> {
        self.statement.projection = vec![SelectItem::Column {
            table: column.table(),
            column: column.name(),
        }];
        let (sql, params) = render_select(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        rows.iter().map(|row| row.get::<T>(column.name())).collect()
    }

    /// Applies column assignments to every row matching the query's filters.
    ///
    /// Build the assignments with [`Column::set`](crate::Column::set). Returns the
    /// number of rows changed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model};
    /// # async fn run<M: Model>(db: Database, col: tork_orm_core::Column<M, bool>) -> tork_orm_core::Result<()> {
    /// let changed = M::query().update(&db, [col.set(false)]).await?;
    /// # let _ = changed;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update<E: Executor>(
        self,
        executor: E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> crate::Result<u64> {
        let statement = UpdateStatement {
            table: self.statement.table,
            assignments: assignments.into_iter().collect(),
            filters: self.statement.filters,
            returning: Vec::new(),
        };
        let (sql, params) = render_update(executor.dialect(), &statement);
        Ok(executor.execute(sql, params).await?.rows_affected)
    }

    /// Updates every row matching the query's filters and returns the changed rows.
    ///
    /// Like [`update`](Self::update) but appends `RETURNING` to the statement,
    /// fetching the stored values after the update. All columns of `M` are
    /// returned and deserialized into `Vec<M>`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model};
    /// # async fn run<M: Model>(db: Database, col: tork_orm_core::Column<M, bool>) -> tork_orm_core::Result<()> {
    /// let updated = M::query()
    ///     .update_returning(&db, [col.set(false)])
    ///     .await?;
    /// # let _ = updated; Ok(())
    /// # }
    /// ```
    pub async fn update_returning<E: Executor>(
        self,
        executor: E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> crate::Result<Vec<M>> {
        let returning = M::COLUMNS.iter().map(|c| c.name).collect();
        let statement = UpdateStatement {
            table: self.statement.table,
            assignments: assignments.into_iter().collect(),
            filters: self.statement.filters,
            returning,
        };
        let (sql, params) = render_update(executor.dialect(), &statement);
        let rows = executor.fetch_all(sql, params).await?;
        rows.iter().map(M::from_row).collect()
    }

    /// Deletes every row matching the query's filters, returning the count removed.
    pub async fn delete<E: Executor>(self, executor: E) -> crate::Result<u64> {
        let statement = DeleteStatement {
            table: self.statement.table,
            filters: self.statement.filters,
            returning: Vec::new(),
        };
        let (sql, params) = render_delete(executor.dialect(), &statement);
        Ok(executor.execute(sql, params).await?.rows_affected)
    }

    /// Deletes every row matching the query's filters and returns the removed rows.
    ///
    /// Like [`delete`](Self::delete) but appends `RETURNING`, so the deleted
    /// rows are available after removal. Useful for soft-delete pipelines or
    /// audit logging.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model};
    /// # async fn run<M: Model>(db: Database) -> tork_orm_core::Result<()> {
    /// let removed = M::query()
    ///     .filter(M::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar))
    ///     .delete_returning(&db)
    ///     .await?;
    /// # let _ = removed; Ok(())
    /// # }
    /// ```
    pub async fn delete_returning<E: Executor>(self, executor: E) -> crate::Result<Vec<M>> {
        let returning = M::COLUMNS.iter().map(|c| c.name).collect();
        let statement = DeleteStatement {
            table: self.statement.table,
            filters: self.statement.filters,
            returning,
        };
        let (sql, params) = render_delete(executor.dialect(), &statement);
        let rows = executor.fetch_all(sql, params).await?;
        rows.iter().map(M::from_row).collect()
    }
}

impl<M: Model> Default for QuerySet<M> {
    fn default() -> Self {
        Self::new()
    }
}
