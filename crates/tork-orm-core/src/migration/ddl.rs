//! The schema-change (DDL) AST.
//!
//! These backend-neutral structures describe a schema change. The
//! [`SchemaManager`](crate::migration::SchemaManager) builders assemble them, and
//! the [`render`](crate::migration::render) layer turns them into SQL for a given
//! dialect. Like the query AST, the DDL AST is independent of any one backend.

use crate::dialect::SqlType;

/// The action a foreign key takes when the referenced row changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKeyAction {
    /// `NO ACTION` (the default).
    NoAction,
    /// `RESTRICT`.
    Restrict,
    /// `CASCADE`.
    Cascade,
    /// `SET NULL`.
    SetNull,
    /// `SET DEFAULT`.
    SetDefault,
}

impl ForeignKeyAction {
    /// Returns the SQL keyword, or `None` for the default `NO ACTION`.
    pub fn as_sql(self) -> Option<&'static str> {
        match self {
            ForeignKeyAction::NoAction => None,
            ForeignKeyAction::Restrict => Some("RESTRICT"),
            ForeignKeyAction::Cascade => Some("CASCADE"),
            ForeignKeyAction::SetNull => Some("SET NULL"),
            ForeignKeyAction::SetDefault => Some("SET DEFAULT"),
        }
    }
}

/// A column default value.
///
/// Rendered as a SQL literal (defaults cannot be bound parameters).
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    /// A boolean, rendered as `0`/`1`.
    Bool(bool),
    /// An integer literal.
    Int(i64),
    /// A floating point literal.
    Real(f64),
    /// A text literal (single-quoted, with embedded quotes escaped).
    Text(String),
    /// The `CURRENT_TIMESTAMP` keyword.
    CurrentTimestamp,
    /// The `NULL` keyword.
    Null,
    /// Verbatim SQL, an escape hatch the caller is responsible for.
    Raw(String),
}

impl From<bool> for DefaultValue {
    fn from(value: bool) -> Self {
        DefaultValue::Bool(value)
    }
}

impl From<i64> for DefaultValue {
    fn from(value: i64) -> Self {
        DefaultValue::Int(value)
    }
}

impl From<i32> for DefaultValue {
    fn from(value: i32) -> Self {
        DefaultValue::Int(i64::from(value))
    }
}

impl From<f64> for DefaultValue {
    fn from(value: f64) -> Self {
        DefaultValue::Real(value)
    }
}

impl From<&str> for DefaultValue {
    fn from(value: &str) -> Self {
        DefaultValue::Text(value.to_string())
    }
}

impl From<String> for DefaultValue {
    fn from(value: String) -> Self {
        DefaultValue::Text(value)
    }
}

/// The specification of a single column in a DDL statement.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSpec {
    /// The column name.
    pub name: String,
    /// The abstract column type.
    pub ty: SqlType,
    /// Whether the column accepts `NULL`.
    pub nullable: bool,
    /// Whether the column is (part of) the primary key.
    pub primary_key: bool,
    /// Whether the database assigns the value automatically.
    pub auto_increment: bool,
    /// Whether the column has a `UNIQUE` constraint.
    pub unique: bool,
    /// An optional default value.
    pub default: Option<DefaultValue>,
}

impl ColumnSpec {
    /// Builds a non-null-by-default column spec of the given type.
    pub fn new(name: impl Into<String>, ty: SqlType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            default: None,
        }
    }
}

/// A foreign key constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignKeySpec {
    /// The local columns.
    pub columns: Vec<String>,
    /// The referenced table.
    pub ref_table: String,
    /// The referenced columns.
    pub ref_columns: Vec<String>,
    /// The `ON DELETE` action.
    pub on_delete: ForeignKeyAction,
    /// The `ON UPDATE` action.
    pub on_update: ForeignKeyAction,
}

/// An index definition.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexSpec {
    /// The index name.
    pub name: String,
    /// The table the index is on.
    pub table: String,
    /// The indexed columns.
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub unique: bool,
    /// Whether to use `IF NOT EXISTS`.
    pub if_not_exists: bool,
}

/// A `CREATE TABLE` definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TableDef {
    /// The table name.
    pub name: String,
    /// Whether to use `IF NOT EXISTS`.
    pub if_not_exists: bool,
    /// The columns, in declaration order.
    pub columns: Vec<ColumnSpec>,
    /// Composite primary key columns (when not declared inline on a column).
    pub primary_key: Vec<String>,
    /// Foreign key constraints.
    pub foreign_keys: Vec<ForeignKeySpec>,
    /// Indexes created alongside the table.
    pub indexes: Vec<IndexSpec>,
}

impl TableDef {
    /// Builds an empty table definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            if_not_exists: false,
            columns: Vec::new(),
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
        }
    }
}

/// One change within an `ALTER TABLE`.
#[derive(Debug, Clone, PartialEq)]
pub enum AlterAction {
    /// Add a column.
    AddColumn(ColumnSpec),
    /// Drop a column by name.
    DropColumn(String),
    /// Rename a column.
    RenameColumn {
        /// The current name.
        from: String,
        /// The new name.
        to: String,
    },
}

/// An `ALTER TABLE` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct AlterTable {
    /// The table being altered.
    pub table: String,
    /// The changes to apply, in order.
    pub actions: Vec<AlterAction>,
}
