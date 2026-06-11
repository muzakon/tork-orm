//! Index metadata.
//!
//! A model declares its indexes (via `#[derive(Model)]`), and the migration DDL
//! layer renders them. These types live in core — not in the feature-gated
//! migration module — because [`Model`](crate::Model) is always compiled and must
//! be able to describe its indexes. A partial index's predicate reuses the query
//! [`Expr`](crate::Expr).

use crate::query::expr::Expr;

/// One column of an index, with optional ordering and operator class.
///
/// # Examples
///
/// ```
/// use tork_orm_core::IndexColumn;
///
/// let column = IndexColumn::new("created_at").desc();
/// assert!(column.descending);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexColumn {
    /// The column name.
    pub name: String,
    /// Whether the column is indexed in descending order.
    pub descending: bool,
    /// A backend-specific operator class (e.g. `gin_trgm_ops`); Postgres-only.
    pub opclass: Option<String>,
}

impl IndexColumn {
    /// Builds an ascending index column.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            descending: false,
            opclass: None,
        }
    }

    /// Marks the column ascending (the default).
    pub fn asc(mut self) -> Self {
        self.descending = false;
        self
    }

    /// Marks the column descending.
    pub fn desc(mut self) -> Self {
        self.descending = true;
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
