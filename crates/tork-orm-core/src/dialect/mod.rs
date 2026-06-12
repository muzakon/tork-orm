//! The dialect abstraction: everything backend-specific about generating SQL.
//!
//! A [`Dialect`] knows how to quote identifiers, write parameter placeholders, map
//! abstract column types to concrete SQL types, and (in later phases) render the
//! query AST into a SQL string plus an ordered list of bound parameters. Keeping
//! these behind one trait is what makes adding a new backend a small, isolated
//! change: implement [`Dialect`] for it and wire up a driver.

pub mod writer;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteDialect;

pub mod postgres;

pub use postgres::PostgresDialect;

pub use writer::{
    QueryWriter, predicate_sql, quote_string_literal, render_count, render_delete, render_exists,
    render_expr, render_insert, render_select, render_union, render_update,
};

/// Identifies a database backend.
///
/// Used where rendering branches on the backend (notably DDL, where constructs
/// like auto-increment columns differ between databases).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialectKind {
    /// SQLite.
    Sqlite,
    /// PostgreSQL (reserved for a future backend).
    Postgres,
    /// MySQL (reserved for a future backend).
    Mysql,
}

/// An abstract column type, independent of any backend.
///
/// Models record one of these per column (derived from the field's Rust type and
/// `#[field(...)]` attributes). A dialect maps it to a concrete SQL type. The
/// mapping is unused by query execution today but is the foundation a later
/// migrations phase builds on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    /// A boolean, typically stored as a small integer.
    Boolean,
    /// A 32-bit signed integer.
    Integer,
    /// A 64-bit signed integer.
    BigInt,
    /// A double-precision floating point number.
    Real,
    /// Unbounded UTF-8 text.
    Text,
    /// Bounded UTF-8 text of at most the given length.
    Varchar(u32),
    /// A timestamp with time zone.
    Timestamp,
    /// Raw bytes.
    Blob,
    /// A JSON document (PostgreSQL `jsonb`).
    Json,
    /// A UUID (PostgreSQL `uuid`).
    Uuid,
    /// An array of the given element type (PostgreSQL `element[]`).
    ///
    /// A `&'static` reference rather than a `Box` so [`SqlType`] stays `Copy`; the
    /// derive const-promotes the inner type.
    Array(&'static SqlType),
}

/// Generates backend-specific SQL.
///
/// Implementors override the small set of primitives that differ between
/// databases. The query layer (added in a later commit of this phase) renders the
/// query AST through these primitives, so the AST itself stays backend-neutral.
pub trait Dialect: Send + Sync + 'static {
    /// Returns the dialect's stable name, for diagnostics.
    fn name(&self) -> &'static str;

    /// Returns which backend this dialect targets.
    ///
    /// Lets backend-neutral rendering branch on the few constructs that differ
    /// between databases without downcasting.
    fn kind(&self) -> DialectKind;

    /// Writes a quoted identifier (table or column name) into `out`.
    ///
    /// Implementations must escape the quote character to prevent identifiers
    /// from breaking out of their quoting.
    fn quote_identifier(&self, identifier: &str, out: &mut String);

    /// Writes the placeholder for the parameter at `index` (zero-based) into `out`.
    ///
    /// Backends differ here: some use a positional `?`, others a numbered `$N`.
    fn placeholder(&self, index: usize, out: &mut String);

    /// Returns `true` if the backend supports `INSERT ... RETURNING`.
    fn supports_returning(&self) -> bool;

    /// Writes the backend's concrete column type for an abstract [`SqlType`].
    ///
    /// This is the single source of truth for column type spelling in DDL. It
    /// writes into `out` rather than returning a `&'static str` so that
    /// parameterized types such as `VARCHAR(n)` can include their length.
    fn map_sql_type(&self, ty: SqlType, out: &mut String);

    /// Returns a quoted identifier as an owned `String`.
    ///
    /// A convenience wrapper over [`Dialect::quote_identifier`].
    fn quoted(&self, identifier: &str) -> String {
        let mut out = String::with_capacity(identifier.len() + 2);
        self.quote_identifier(identifier, &mut out);
        out
    }

    /// The statement that begins a transaction.
    fn begin_sql(&self) -> &'static str {
        "BEGIN"
    }

    /// The statement that begins a transaction with the given isolation level.
    ///
    /// Most backends map this to `BEGIN` (ignoring the level) unless they have a
    /// matching SQL form. SQLite overrides this to support `BEGIN DEFERRED`,
    /// `BEGIN IMMEDIATE`, and `BEGIN EXCLUSIVE`.
    fn begin_with_sql(&self, _level: crate::transaction::IsolationLevel) -> String {
        "BEGIN".to_string()
    }

    /// The statement that commits a transaction.
    fn commit_sql(&self) -> &'static str {
        "COMMIT"
    }

    /// The statement that rolls back a transaction.
    fn rollback_sql(&self) -> &'static str {
        "ROLLBACK"
    }

    /// The statement that creates a savepoint with the given name.
    fn savepoint_sql(&self, name: &str) -> String {
        format!("SAVEPOINT {name}")
    }

    /// The statement that releases (commits) a savepoint with the given name.
    fn release_sql(&self, name: &str) -> String {
        format!("RELEASE {name}")
    }

    /// The statement that rolls back to a savepoint without ending the transaction.
    fn rollback_to_sql(&self, name: &str) -> String {
        format!("ROLLBACK TO {name}")
    }

    /// Returns the literal for `value` as it appears inline in a boolean column.
    ///
    /// Used when rendering DDL (a partial index predicate, say), where a parameter
    /// cannot be bound and the value has to be written into the SQL directly. The
    /// default stores booleans as `1`/`0`, matching the integer encoding most
    /// backends use; a backend with a native boolean overrides this.
    fn bool_literal(&self, value: bool) -> &'static str {
        if value {
            "1"
        } else {
            "0"
        }
    }

    /// Returns `true` if the backend supports choosing an index method (`USING`).
    fn supports_index_method(&self) -> bool {
        false
    }

    /// Returns `true` if the backend supports covering columns on an index
    /// (`INCLUDE`).
    fn supports_index_include(&self) -> bool {
        false
    }

    /// Returns `true` if the backend supports a per-column operator class on an
    /// index.
    fn supports_index_opclass(&self) -> bool {
        false
    }
}
