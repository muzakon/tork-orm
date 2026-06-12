//! The write-statement AST: inserts, updates, and deletes.
//!
//! Like the [`SelectStatement`](crate::query::ast::SelectStatement), these are
//! backend-neutral and rendered to SQL plus bound parameters by a dialect. All
//! written values are bound parameters.

use crate::query::expr::Expr;
use crate::value::Value;

/// Controls what happens when an inserted row conflicts with an existing one.
///
/// Used in [`InsertStatement`] and produced by [`Model::upsert`](crate::Model::upsert).
/// The conflict clauses use portable `ON CONFLICT` syntax, accepted by both
/// PostgreSQL and SQLite (≥ 3.24, which the bundled driver provides).
#[derive(Debug, Clone, Default)]
pub enum OnConflict {
    /// Plain `INSERT INTO` — propagate the conflict as an error (default).
    #[default]
    None,
    /// `ON CONFLICT (constraint) DO UPDATE SET ...` — update the existing row.
    ///
    /// `constraint` names the conflict-target columns (a unique/primary key); the
    /// `updates` are applied to the existing row, typically setting each column to
    /// its [`EXCLUDED`](crate::Expr::excluded) (would-be-inserted) value.
    Update {
        /// The conflict-target columns.
        constraint: Vec<&'static str>,
        /// The column assignments to apply on conflict.
        updates: Vec<Assignment>,
    },
    /// `ON CONFLICT (constraint) DO NOTHING` — skip the row on conflict.
    ///
    /// An empty `constraint` renders `ON CONFLICT DO NOTHING` (any conflict).
    DoNothing {
        /// The conflict-target columns, or empty for any conflict.
        constraint: Vec<&'static str>,
    },
}

/// A column assignment in an `UPDATE` (`column = expression`).
///
/// Built by [`Column::set`](crate::Column::set). The right-hand side is an
/// [`Expr`], so it can be either a bound literal (`col.set("new")`) or an
/// arbitrary expression (`col.set(col.add(1))`).
#[derive(Debug, Clone)]
pub struct Assignment {
    /// The column being assigned.
    pub column: &'static str,
    /// The expression to assign to the column.
    pub value: Expr,
}

impl Assignment {
    /// Builds an assignment of `value` to `column`.
    pub fn new(column: &'static str, value: Expr) -> Self {
        Self { column, value }
    }
}

/// An `INSERT` statement, possibly inserting several rows.
#[derive(Debug, Clone)]
pub struct InsertStatement {
    /// The target table.
    pub table: &'static str,
    /// The columns written, in order.
    pub columns: Vec<&'static str>,
    /// One value list per inserted row, each aligned with `columns`.
    pub rows: Vec<Vec<Value>>,
    /// Columns to return from the inserted rows; empty means no `RETURNING`.
    pub returning: Vec<&'static str>,
    /// Conflict resolution strategy; default is [`OnConflict::None`].
    pub on_conflict: OnConflict,
}

/// An `UPDATE` statement.
#[derive(Debug, Clone)]
pub struct UpdateStatement {
    /// The target table.
    pub table: &'static str,
    /// The column assignments.
    pub assignments: Vec<Assignment>,
    /// The predicates restricting which rows change, joined by `AND`.
    pub filters: Vec<Expr>,
    /// Columns to return from the updated rows; empty means no `RETURNING`.
    pub returning: Vec<&'static str>,
}

/// A `DELETE` statement.
#[derive(Debug, Clone)]
pub struct DeleteStatement {
    /// The target table.
    pub table: &'static str,
    /// The predicates restricting which rows are removed, joined by `AND`.
    pub filters: Vec<Expr>,
    /// Columns to return from the deleted rows; empty means no `RETURNING`.
    pub returning: Vec<&'static str>,
}
