//! The query builder.
//!
//! A [`QuerySet`] accumulates a [`SelectStatement`] through chainable builders and
//! runs it through an [`Executor`](crate::Executor) with a terminal method. The
//! builders mirror a readable, filter-first style: `filter` adds an `AND`
//! predicate, `filter_any` adds an `OR` group, and so on.

use std::marker::PhantomData;

use crate::dialect::{
    render_count, render_delete, render_exists, render_select, render_update, Dialect,
};
use crate::error::OrmError;
use crate::executor::Executor;
use crate::model::{FromRow, Model};
use crate::query::ast::{
    Cte, CteQuery, JoinKind, LockClause, LockStrength, LockWait, OrderItem, SelectItem,
    SelectStatement, UnionStatement, WithClause,
};
use crate::query::column::Column;
use crate::query::expr::{BinaryOp, Expr};
use crate::query::projection::ExprTuple;
use crate::query::write::{Assignment, DeleteStatement, UpdateStatement};
use crate::value::Value;

/// A typed query over a model `M`.
///
/// # Examples
///
/// ```no_run
/// use tork_orm_core::{Database, Model};
/// # use tork_orm_core::{Row, Value};
/// # #[derive(Clone)] struct User;
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
/// # impl tork_orm_core::ModelHooks for User {}
/// # async fn run(db: Database) -> tork_orm_core::Result<()> {
/// let users = User::query().limit(20).all(&db).await?;
/// # let _ = users;
/// # Ok(())
/// # }
/// ```
pub struct QuerySet<M: Model> {
    statement: SelectStatement,
    /// Whether the automatic soft-delete scope filter (`deleted_at IS NULL`) is
    /// currently present as the first filter, so [`with_deleted`](QuerySet::with_deleted)
    /// can drop it. Always `false` for models without a soft-delete column.
    scope_active: bool,
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
/// # #[derive(Clone)] struct User;
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
        let mut statement = SelectStatement::new(M::TABLE, projection);
        // Soft-delete models are scoped to non-deleted rows by default. The filter
        // goes in first so it is baked into every terminal, subquery, and union;
        // `with_deleted`/`only_deleted` adjust it.
        let scope_active = if let Some(column) = M::DELETED_AT {
            statement.filters.push(scope_filter(M::TABLE, column, false));
            true
        } else {
            false
        };
        Self {
            statement,
            scope_active,
            _marker: PhantomData,
        }
    }

    /// Includes soft-deleted rows in the results, dropping the default
    /// `deleted_at IS NULL` scope. A no-op for models without a soft-delete column.
    pub fn with_deleted(mut self) -> Self {
        if self.scope_active {
            self.statement.filters.remove(0);
            self.scope_active = false;
        }
        self
    }

    /// Restricts the results to only soft-deleted rows (`deleted_at IS NOT NULL`).
    /// A no-op for models without a soft-delete column.
    pub fn only_deleted(mut self) -> Self {
        if let Some(column) = M::DELETED_AT {
            if self.scope_active {
                self.statement.filters.remove(0);
                self.scope_active = false;
            }
            self.statement.filters.push(scope_filter(M::TABLE, column, true));
        }
        self
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
    /// # #[derive(Clone)] struct User;
    /// # impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } }
    /// # impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
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
            alias: None,
            left_table: "",
            left_column: "",
            right_table: "",
            right_column: "",
        });
        self
    }

    /// Joins this model's table to itself under `alias` with an `INNER JOIN`,
    /// matching `base_column` on the base table against `alias_column` on the
    /// aliased copy. Reference the aliased columns in filters and projections with
    /// [`Expr::column`](crate::Expr::column)`(alias, "...")`.
    ///
    /// ```ignore
    /// // Employees managed by "Alice".
    /// Employee::query()
    ///     .self_join("mgr", "manager_id", "id")
    ///     .filter(Expr::column("mgr", "name").eq("Alice"))
    /// ```
    pub fn self_join(
        self,
        alias: &'static str,
        base_column: &'static str,
        alias_column: &'static str,
    ) -> Self {
        self.self_join_with_kind(JoinKind::Inner, alias, base_column, alias_column)
    }

    /// Like [`self_join`](Self::self_join) but with a `LEFT JOIN`, so base rows
    /// without a matching aliased row are kept (the aliased columns read as `NULL`).
    pub fn self_left_join(
        self,
        alias: &'static str,
        base_column: &'static str,
        alias_column: &'static str,
    ) -> Self {
        self.self_join_with_kind(JoinKind::Left, alias, base_column, alias_column)
    }

    fn self_join_with_kind(
        mut self,
        kind: JoinKind,
        alias: &'static str,
        base_column: &'static str,
        alias_column: &'static str,
    ) -> Self {
        self.statement.joins.push(crate::query::ast::Join {
            kind,
            table: M::TABLE,
            alias: Some(alias),
            left_table: M::TABLE,
            left_column: base_column,
            right_table: alias,
            right_column: alias_column,
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

    /// Keeps only the first row of each group of the given expressions
    /// (`DISTINCT ON (...)`), ordered by the query's `ORDER BY`.
    ///
    /// PostgreSQL only; using it on another backend is rejected at the terminal
    /// with a clear error.
    pub fn distinct_on<G: ExprTuple>(mut self, group: G) -> Self {
        self.statement.distinct_on = group.into_exprs();
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
    /// A bare `FOR UPDATE` works on every backend with row-level locking
    /// (PostgreSQL, MySQL, SQLite 3.54+). The modifiers below
    /// ([`for_share`](Self::for_share), [`skip_locked`](Self::skip_locked),
    /// [`nowait`](Self::nowait), [`lock_of`](Self::lock_of)) require PostgreSQL or
    /// MySQL and are rejected elsewhere at the terminal.
    pub fn for_update(mut self) -> Self {
        self.statement.lock = Some(LockClause::new(LockStrength::Update));
        self
    }

    /// Locks the selected rows with `FOR SHARE` (a shared lock: concurrent reads
    /// are allowed, writes are blocked).
    pub fn for_share(mut self) -> Self {
        self.statement.lock = Some(LockClause::new(LockStrength::Share));
        self
    }

    /// Skips rows already locked by another transaction (`SKIP LOCKED`), instead
    /// of waiting. Implies `FOR UPDATE` when no lock was set yet.
    pub fn skip_locked(mut self) -> Self {
        self.lock_mut().wait = LockWait::SkipLocked;
        self
    }

    /// Fails immediately rather than waiting when a row is already locked
    /// (`NOWAIT`). Implies `FOR UPDATE` when no lock was set yet.
    pub fn nowait(mut self) -> Self {
        self.lock_mut().wait = LockWait::NoWait;
        self
    }

    /// Restricts the lock to rows of the given tables (`OF table, ...`). Implies
    /// `FOR UPDATE` when no lock was set yet.
    pub fn lock_of(mut self, tables: &[&'static str]) -> Self {
        self.lock_mut().of = tables.to_vec();
        self
    }

    /// Returns the current lock clause, inserting a default `FOR UPDATE` if none
    /// was set, so the modifier builders can be used on their own.
    fn lock_mut(&mut self) -> &mut LockClause {
        self.statement
            .lock
            .get_or_insert_with(|| LockClause::new(LockStrength::Update))
    }

    /// Keeps only rows that sort strictly after `cursor` under the current
    /// `ORDER BY` (keyset / "seek" pagination). Pair with `.limit(n)` to fetch
    /// the next page without the cost of a large `OFFSET`.
    ///
    /// `cursor` holds the ordering-key values of the last row of the previous
    /// page, one per `ORDER BY` term and in the same order. Build them from a
    /// row with [`BindValue::to_value`](crate::BindValue::to_value).
    ///
    /// # Panics
    ///
    /// Panics if `cursor` is empty or its length differs from the number of
    /// `ORDER BY` terms, since the comparison would be ill-defined.
    pub fn keyset_after(mut self, cursor: Vec<Value>) -> Self {
        let predicate = keyset_predicate(&self.statement.order_by, &cursor, true);
        self.statement.filters.push(predicate);
        self
    }

    /// Keeps only rows that sort strictly before `cursor` under the current
    /// `ORDER BY` (keyset pagination, walking backwards). See
    /// [`keyset_after`](Self::keyset_after) for the cursor shape and panics.
    pub fn keyset_before(mut self, cursor: Vec<Value>) -> Self {
        let predicate = keyset_predicate(&self.statement.order_by, &cursor, false);
        self.statement.filters.push(predicate);
        self
    }

    /// Attaches a `WITH` clause with one or more Common Table Expressions.
    ///
    /// Each CTE is given as `(name, query)`. The query can be built from a
    /// `QuerySet` via [`into_statement`](Self::into_statement) or as a
    /// [`UnionStatement`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let regional = User::query().filter(User::region.eq("EU"));
    /// let admins = User::query().filter(User::is_admin.eq(true));
    /// let qs = User::query()
    ///     .with([("regional", CteQuery::Select(regional.into_statement()))])
    ///     .filter_raw("id IN (SELECT id FROM regional)", []);
    /// ```
    pub fn with(mut self, ctes: impl IntoIterator<Item = (&'static str, CteQuery)>) -> Self {
        self.statement.with = Some(WithClause {
            recursive: false,
            ctes: ctes
                .into_iter()
                .map(|(name, query)| Cte { name, columns: None, query })
                .collect(),
        });
        self
    }

    /// Attaches a `WITH RECURSIVE` clause with one or more Common Table
    /// Expressions. Recursive CTEs reference themselves by name in the second
    /// branch of a `UNION ALL`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let cte = User::query()
    ///     .filter(User::parent_id.is_null())
    ///     .select((User::id, User::name, User::parent_id))
    ///     .union_all(
    ///         User::query()
    ///             .filter(User::parent_id.is_not_null())
    ///             .select((User::id, User::name, User::parent_id)),
    ///     );
    /// let qs = User::query()
    ///     .with_recursive([("ancestors", cte)]);
    /// ```
    pub fn with_recursive(
        mut self,
        ctes: impl IntoIterator<Item = (&'static str, crate::query::union::UnionQuery<M>)>,
    ) -> Self {
        self.statement.with = Some(WithClause {
            recursive: true,
            ctes: ctes
                .into_iter()
                .map(|(name, query)| Cte {
                    name,
                    columns: None,
                    query: CteQuery::Union(Box::new(query.into_statement())),
                })
                .collect(),
        });
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
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
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
        validate_for_dialect(executor.dialect(), &self.statement)?;
        let (sql, params) = render_select(executor.dialect(), &self.statement);
        let rows = executor.fetch_all(sql, params).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Runs the query and returns the first matching row, if any.
    pub async fn first<E: Executor>(mut self, executor: E) -> crate::Result<Option<M>> {
        self.statement.limit = Some(1);
        validate_for_dialect(executor.dialect(), &self.statement)?;
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
        validate_for_dialect(executor.dialect(), &self.statement)?;
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
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
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
        validate_for_dialect(executor.dialect(), &self.statement)?;
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
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
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
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
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
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
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
        validate_for_dialect(executor.dialect(), &self.statement)?;
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
    ///
    /// On a model with a `#[field(deleted_at)]` column this is a *soft* delete: the
    /// rows are stamped with the current time instead of being removed. Use
    /// [`hard_delete`](Self::hard_delete) to remove them permanently.
    pub async fn delete<E: Executor>(self, executor: E) -> crate::Result<u64> {
        if let Some(column) = M::DELETED_AT {
            let statement = UpdateStatement {
                table: self.statement.table,
                assignments: vec![Assignment::new(column, Expr::raw("CURRENT_TIMESTAMP"))],
                filters: self.statement.filters,
                returning: Vec::new(),
            };
            let (sql, params) = render_update(executor.dialect(), &statement);
            return Ok(executor.execute(sql, params).await?.rows_affected);
        }
        let statement = DeleteStatement {
            table: self.statement.table,
            filters: self.statement.filters,
            returning: Vec::new(),
        };
        let (sql, params) = render_delete(executor.dialect(), &statement);
        Ok(executor.execute(sql, params).await?.rows_affected)
    }

    /// Permanently removes every row matching the query's filters, bypassing
    /// soft-delete. Identical to [`delete`](Self::delete) for models without a
    /// soft-delete column.
    pub async fn hard_delete<E: Executor>(self, executor: E) -> crate::Result<u64> {
        let statement = DeleteStatement {
            table: self.statement.table,
            filters: self.statement.filters,
            returning: Vec::new(),
        };
        let (sql, params) = render_delete(executor.dialect(), &statement);
        Ok(executor.execute(sql, params).await?.rows_affected)
    }

    /// Clears the soft-delete mark (`deleted_at = NULL`) on every matching row,
    /// returning the count restored. Pair with [`only_deleted`](Self::only_deleted)
    /// or [`with_deleted`](Self::with_deleted), since the default scope hides
    /// soft-deleted rows. A no-op for models without a soft-delete column.
    pub async fn restore<E: Executor>(self, executor: E) -> crate::Result<u64> {
        let Some(column) = M::DELETED_AT else {
            return Ok(0);
        };
        let statement = UpdateStatement {
            table: self.statement.table,
            assignments: vec![Assignment::new(column, Expr::value(Value::Null))],
            filters: self.statement.filters,
            returning: Vec::new(),
        };
        let (sql, params) = render_update(executor.dialect(), &statement);
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

/// Rejects a statement that uses a feature the dialect cannot support before it is
/// rendered, so the caller gets a clear error rather than invalid SQL.
///
/// Currently this checks for `FULL OUTER JOIN` on dialects that lack it (MySQL),
/// recursing into CTE bodies.
pub(crate) fn validate_for_dialect(
    dialect: &dyn Dialect,
    statement: &SelectStatement,
) -> crate::Result<()> {
    if !dialect.supports_full_join()
        && statement.joins.iter().any(|join| join.kind == JoinKind::Full)
    {
        return Err(OrmError::query(format!(
            "FULL OUTER JOIN is not supported by the `{}` dialect",
            dialect.name()
        )));
    }
    if !dialect.supports_distinct_on() && !statement.distinct_on.is_empty() {
        return Err(OrmError::query(format!(
            "DISTINCT ON is not supported by the `{}` dialect",
            dialect.name()
        )));
    }
    if !dialect.supports_lock_modifiers()
        && statement.lock.as_ref().is_some_and(LockClause::uses_modifiers)
    {
        return Err(OrmError::query(format!(
            "FOR SHARE / SKIP LOCKED / NOWAIT / OF is not supported by the `{}` dialect",
            dialect.name()
        )));
    }
    if let Some(with) = &statement.with {
        for cte in &with.ctes {
            match &cte.query {
                CteQuery::Select(select) => validate_for_dialect(dialect, select)?,
                CteQuery::Union(union) => validate_union_for_dialect(dialect, union)?,
            }
        }
    }
    Ok(())
}

/// Builds a soft-delete scope predicate: `deleted_at IS NULL` (active rows) or
/// `deleted_at IS NOT NULL` (only deleted) when `deleted` is `true`.
fn scope_filter(table: &'static str, column: &'static str, deleted: bool) -> Expr {
    Expr::is_null(Expr::column(table, column), deleted)
}

/// Builds the keyset (seek) predicate for an `ORDER BY` and a boundary `cursor`.
///
/// For terms `t0..tn` with cursor values `v0..vn` it expands to the standard
/// lexicographic comparison as an `OR` of `AND` groups, which handles mixed
/// `ASC`/`DESC` directions that a row-value comparison cannot:
///
/// ```text
/// (t0 > v0)
///   OR (t0 = v0 AND t1 > v1)
///   OR (t0 = v0 AND t1 = v1 AND t2 > v2)
/// ```
///
/// Each `>` is flipped to `<` when the term is `DESC`, and again when walking
/// backwards (`after = false`).
fn keyset_predicate(order: &[OrderItem], cursor: &[Value], after: bool) -> Expr {
    assert!(
        !order.is_empty(),
        "keyset pagination requires at least one `order_by` term"
    );
    assert_eq!(
        order.len(),
        cursor.len(),
        "keyset cursor length ({}) must match the number of `order_by` terms ({})",
        cursor.len(),
        order.len(),
    );

    let mut disjuncts = Vec::with_capacity(order.len());
    for boundary in 0..order.len() {
        let mut conjuncts = Vec::with_capacity(boundary + 1);
        // Earlier keys must be equal to the cursor's values.
        for prior in 0..boundary {
            conjuncts.push(Expr::binary(
                order[prior].expr.clone(),
                BinaryOp::Eq,
                Expr::value(cursor[prior].clone()),
            ));
        }
        // The boundary key is strictly greater (or less, depending on direction).
        let ascending = !order[boundary].descending;
        let op = if ascending == after { BinaryOp::Gt } else { BinaryOp::Lt };
        conjuncts.push(Expr::binary(
            order[boundary].expr.clone(),
            op,
            Expr::value(cursor[boundary].clone()),
        ));
        disjuncts.push(Expr::all(conjuncts));
    }
    Expr::any(disjuncts)
}

/// Validates every branch of a union (used by CTE bodies and [`UnionQuery`]).
pub(crate) fn validate_union_for_dialect(
    dialect: &dyn Dialect,
    union: &UnionStatement,
) -> crate::Result<()> {
    validate_for_dialect(dialect, &union.first)?;
    for (_, branch) in &union.rest {
        validate_for_dialect(dialect, branch)?;
    }
    Ok(())
}
