//! The statement AST built by the query builder.
//!
//! A [`SelectStatement`] is the backend-neutral description of a query. The
//! builder ([`QuerySet`](crate::query::QuerySet)) assembles it, and a
//! [`Dialect`](crate::dialect::Dialect) renders it to SQL plus bound parameters.

use crate::query::expr::Expr;

/// One item in a `SELECT` projection.
#[derive(Debug, Clone)]
pub enum SelectItem {
    /// A qualified column, `"table"."column"`.
    Column {
        /// The owning table.
        table: &'static str,
        /// The column name.
        column: &'static str,
    },
    /// An arbitrary expression, such as an aggregate or an aliased column.
    Expression(Expr),
}

/// The kind of join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    /// `INNER JOIN` — only rows that match on both sides.
    Inner,
    /// `LEFT JOIN` — all left rows; NULLs on the right when no match.
    Left,
    /// `RIGHT JOIN` — all right rows; NULLs on the left when no match.
    ///
    /// Not supported by SQLite; available in the AST for future backends.
    Right,
    /// `FULL OUTER JOIN` — all rows from both sides with NULLs for mismatches.
    ///
    /// Not supported by SQLite; available in the AST for future backends.
    Full,
    /// `CROSS JOIN` — cartesian product; every left row paired with every right row.
    ///
    /// Has no `ON` condition. Use with care on large tables.
    Cross,
}

impl JoinKind {
    /// Returns the SQL keyword for this join kind.
    pub fn as_sql(self) -> &'static str {
        match self {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left  => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
            JoinKind::Full  => "FULL OUTER JOIN",
            JoinKind::Cross => "CROSS JOIN",
        }
    }
}

/// A join onto another table.
///
/// The condition equates two qualified columns; the join kind controls which
/// rows from each side are included.
#[derive(Debug, Clone)]
pub struct Join {
    /// The kind of join.
    pub kind: JoinKind,
    /// The table brought into the query.
    pub table: &'static str,
    /// An optional alias for the joined table (`JOIN table AS alias`), used to join
    /// a table to itself. `None` renders the table name directly.
    pub alias: Option<&'static str>,
    /// The left side table of the `ON` condition.
    pub left_table: &'static str,
    /// The left side column of the `ON` condition.
    pub left_column: &'static str,
    /// The right side table of the `ON` condition.
    pub right_table: &'static str,
    /// The right side column of the `ON` condition.
    pub right_column: &'static str,
}

/// A single `ORDER BY` term.
#[derive(Debug, Clone)]
pub struct OrderItem {
    /// The expression to order by.
    pub expr: Expr,
    /// Whether to sort descending.
    pub descending: bool,
    /// Where to place `NULL` values: `Some(true)` = `NULLS FIRST`,
    /// `Some(false)` = `NULLS LAST`, `None` = database default.
    pub nulls: Option<bool>,
}

impl OrderItem {
    /// Builds an order term with the database's default NULL placement.
    pub fn new(expr: Expr, descending: bool) -> Self {
        Self { expr, descending, nulls: None }
    }

    /// Places `NULL` values before non-null values (`NULLS FIRST`).
    pub fn nulls_first(mut self) -> Self {
        self.nulls = Some(true);
        self
    }

    /// Places `NULL` values after non-null values (`NULLS LAST`).
    pub fn nulls_last(mut self) -> Self {
        self.nulls = Some(false);
        self
    }
}

/// A Common Table Expression: `name [(columns)] AS (query)`.
#[derive(Debug, Clone)]
pub struct Cte {
    /// The CTE name.
    pub name: &'static str,
    /// Optional output column names (the parenthesised list after the name).
    pub columns: Option<Vec<&'static str>>,
    /// The CTE body — a `SELECT` or `UNION` statement.
    pub query: CteQuery,
}

/// The body of a Common Table Expression.
#[derive(Debug, Clone)]
pub enum CteQuery {
    /// A plain `SELECT` statement.
    Select(SelectStatement),
    /// A `UNION` / `UNION ALL` of several `SELECT` statements.
    Union(Box<UnionStatement>),
}

/// The `WITH [RECURSIVE]` clause at the head of a `SELECT` statement.
#[derive(Debug, Clone)]
pub struct WithClause {
    /// Whether this is `WITH RECURSIVE`.
    pub recursive: bool,
    /// The list of Common Table Expressions.
    pub ctes: Vec<Cte>,
}

/// A `SELECT` statement.
#[derive(Debug, Clone)]
pub struct SelectStatement {
    /// An optional `WITH` clause at the head of the statement.
    pub with: Option<WithClause>,
    /// The table being queried.
    pub table: &'static str,
    /// The projected items.
    pub projection: Vec<SelectItem>,
    /// The joined tables.
    pub joins: Vec<Join>,
    /// The top-level predicates, joined by `AND`.
    pub filters: Vec<Expr>,
    /// The `GROUP BY` expressions.
    pub group_by: Vec<Expr>,
    /// The `HAVING` predicate.
    pub having: Option<Expr>,
    /// The ordering terms.
    pub order_by: Vec<OrderItem>,
    /// An optional row limit.
    pub limit: Option<u64>,
    /// An optional row offset.
    pub offset: Option<u64>,
    /// Whether to return distinct rows.
    pub distinct: bool,
    /// `DISTINCT ON (...)` expressions (PostgreSQL). Empty means no `DISTINCT ON`.
    pub distinct_on: Vec<Expr>,
    /// An optional row-level locking clause (`FOR UPDATE`/`FOR SHARE`).
    pub lock: Option<LockClause>,
}

/// The strength of a row-level lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStrength {
    /// `FOR UPDATE` — blocks other writers and lockers of the rows.
    Update,
    /// `FOR SHARE` — allows concurrent readers but blocks writers.
    Share,
}

/// How a locking read behaves when a target row is already locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockWait {
    /// Wait for the lock to be released (the default).
    Wait,
    /// Fail immediately rather than wait (`NOWAIT`).
    NoWait,
    /// Skip rows that are already locked (`SKIP LOCKED`).
    SkipLocked,
}

/// A row-level locking clause: a strength plus optional `OF` tables and a
/// wait policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockClause {
    /// `FOR UPDATE` or `FOR SHARE`.
    pub strength: LockStrength,
    /// What to do when a row is already locked.
    pub wait: LockWait,
    /// Tables named in an `OF ...` restriction; empty locks every table.
    pub of: Vec<&'static str>,
}

impl LockClause {
    /// A plain `FOR <strength>` with no `OF` and the default wait policy.
    pub fn new(strength: LockStrength) -> Self {
        Self { strength, wait: LockWait::Wait, of: Vec::new() }
    }

    /// Returns `true` if the clause uses any feature beyond a bare `FOR UPDATE`
    /// (a share lock, a wait policy, or an `OF` restriction). These require
    /// dialect support; a bare `FOR UPDATE` works everywhere with row locking.
    pub fn uses_modifiers(&self) -> bool {
        self.strength != LockStrength::Update
            || self.wait != LockWait::Wait
            || !self.of.is_empty()
    }
}

/// A `UNION` or `UNION ALL` combining two or more `SELECT` statements.
///
/// Built by [`QuerySet::union`](crate::query::QuerySet::union) and
/// [`QuerySet::union_all`](crate::query::QuerySet::union_all). The
/// `order_by`, `limit`, and `offset` fields apply to the whole combined
/// result, not to any individual branch.
#[derive(Debug, Clone)]
pub struct UnionStatement {
    /// The first `SELECT`.
    pub first: SelectStatement,
    /// Subsequent branches; the `bool` flag is `true` for `UNION ALL`,
    /// `false` for `UNION` (distinct).
    pub rest: Vec<(bool, SelectStatement)>,
    /// `ORDER BY` terms applied after all branches are combined.
    pub order_by: Vec<OrderItem>,
    /// Optional row limit applied to the combined result.
    pub limit: Option<u64>,
    /// Optional row offset applied to the combined result.
    pub offset: Option<u64>,
    /// An optional row-level locking clause applied to the combined result.
    pub lock: Option<LockClause>,
}

impl SelectStatement {
    /// Builds a statement selecting the given columns from `table`.
    pub fn new(table: &'static str, projection: Vec<SelectItem>) -> Self {
        Self {
            with: None,
            table,
            projection,
            joins: Vec::new(),
            filters: Vec::new(),
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
            distinct: false,
            distinct_on: Vec::new(),
            lock: None,
        }
    }
}
