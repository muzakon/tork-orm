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
}

impl JoinKind {
    /// Returns the SQL keyword for this join kind.
    pub fn as_sql(self) -> &'static str {
        match self {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left  => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
            JoinKind::Full  => "FULL OUTER JOIN",
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

/// A `SELECT` statement.
#[derive(Debug, Clone)]
pub struct SelectStatement {
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
}

impl SelectStatement {
    /// Builds a statement selecting the given columns from `table`.
    pub fn new(table: &'static str, projection: Vec<SelectItem>) -> Self {
        Self {
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
        }
    }
}
