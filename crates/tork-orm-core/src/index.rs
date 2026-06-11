//! Index metadata.
//!
//! A model declares its indexes (via `#[derive(Model)]`), and the migration DDL
//! layer renders them. These types live in core — not in the feature-gated
//! migration module — because [`Model`](crate::Model) is always compiled and must
//! be able to describe its indexes. A partial index's predicate reuses the query
//! [`Expr`](crate::Expr).

use crate::query::expr::Expr;

/// Where `NULL`s sort relative to other values in an index column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullsOrder {
    /// `NULLS FIRST`.
    First,
    /// `NULLS LAST`.
    Last,
}

/// One entry of an index: either a plain column or an expression, with optional
/// ordering, null placement, collation, and operator class.
///
/// # Examples
///
/// ```
/// use tork_orm_core::IndexColumn;
///
/// let column = IndexColumn::new("created_at").desc().nulls_last();
/// assert!(column.descending);
/// ```
#[derive(Debug, Clone)]
pub struct IndexColumn {
    /// The column name. Empty when this entry is an expression.
    pub name: String,
    /// A functional-index expression; when set, it is indexed instead of `name`.
    pub expression: Option<Expr>,
    /// Whether the entry is indexed in descending order.
    pub descending: bool,
    /// Where `NULL`s sort, if specified.
    pub nulls: Option<NullsOrder>,
    /// A collation to index under (e.g. `NOCASE`).
    pub collation: Option<String>,
    /// A backend-specific operator class (e.g. `gin_trgm_ops`); Postgres-only.
    pub opclass: Option<String>,
}

impl IndexColumn {
    /// Builds an ascending index column over a named column.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            expression: None,
            descending: false,
            nulls: None,
            collation: None,
            opclass: None,
        }
    }

    /// Builds a functional index entry over an expression, such as `lower(email)`.
    pub fn expression(expression: Expr) -> Self {
        Self {
            name: String::new(),
            expression: Some(expression),
            descending: false,
            nulls: None,
            collation: None,
            opclass: None,
        }
    }

    /// Marks the entry ascending (the default).
    pub fn asc(mut self) -> Self {
        self.descending = false;
        self
    }

    /// Marks the entry descending.
    pub fn desc(mut self) -> Self {
        self.descending = true;
        self
    }

    /// Sorts `NULL`s first.
    pub fn nulls_first(mut self) -> Self {
        self.nulls = Some(NullsOrder::First);
        self
    }

    /// Sorts `NULL`s last.
    pub fn nulls_last(mut self) -> Self {
        self.nulls = Some(NullsOrder::Last);
        self
    }

    /// Sets the collation to index under.
    pub fn collate(mut self, collation: impl Into<String>) -> Self {
        self.collation = Some(collation.into());
        self
    }

    /// Sets the column's operator class (Postgres).
    pub fn opclass(mut self, opclass: impl Into<String>) -> Self {
        self.opclass = Some(opclass.into());
        self
    }
}

/// The definition of an index.
///
/// Covers single-column and compound indexes, unique indexes, per-column ordering,
/// and partial indexes (a `predicate`). The `method` (e.g. `gin`) and `include`
/// (covering columns) are stored for backends that support them; SQLite rejects
/// them at render time.
#[derive(Debug, Clone)]
pub struct IndexDef {
    /// The index name.
    pub name: String,
    /// The indexed columns, in order.
    pub columns: Vec<IndexColumn>,
    /// Whether the index is unique.
    pub unique: bool,
    /// A partial-index predicate (`WHERE ...`).
    pub predicate: Option<Expr>,
    /// The index method (`USING ...`); Postgres-only.
    pub method: Option<String>,
    /// Covering columns (`INCLUDE (...)`); Postgres-only.
    pub include: Vec<String>,
}

impl IndexDef {
    /// Builds an empty index definition with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: Vec::new(),
            unique: false,
            predicate: None,
            method: None,
            include: Vec::new(),
        }
    }
}
