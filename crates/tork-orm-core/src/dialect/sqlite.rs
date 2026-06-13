//! The SQLite dialect.

use super::{Dialect, DialectKind, SqlType};
use crate::transaction::IsolationLevel;

/// SQL generation for SQLite.
///
/// SQLite quotes identifiers with double quotes, uses positional `?`
/// placeholders, and supports `RETURNING`. Its dynamic typing means the abstract
/// types map onto a small set of storage classes.
///
/// # Examples
///
/// ```
/// use tork_orm_core::dialect::{Dialect, SqliteDialect};
///
/// let dialect = SqliteDialect::new();
/// assert_eq!(dialect.quoted("user id"), "\"user id\"");
/// let mut placeholder = String::new();
/// dialect.placeholder(0, &mut placeholder);
/// assert_eq!(placeholder, "?");
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteDialect;

impl SqliteDialect {
    /// Creates a SQLite dialect.
    pub const fn new() -> Self {
        Self
    }
}

impl Dialect for SqliteDialect {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn kind(&self) -> DialectKind {
        DialectKind::Sqlite
    }

    fn quote_identifier(&self, identifier: &str, out: &mut String) {
        out.push('"');
        for ch in identifier.chars() {
            if ch == '"' {
                // Double an embedded quote so it cannot terminate the identifier.
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    }

    fn placeholder(&self, _index: usize, out: &mut String) {
        out.push('?');
    }

    fn supports_returning(&self) -> bool {
        // `RETURNING` was added in SQLite 3.35.0. The bundled library is newer, but
        // a build linking an older system SQLite must fall back to a re-select, or
        // every insert would fail with a syntax error.
        rusqlite::version_number() >= 3_035_000
    }

    fn map_sql_type(&self, ty: SqlType, out: &mut String) {
        // SQLite uses type affinity, so the declared type can keep its readable
        // spelling (e.g. `BOOLEAN`, `TIMESTAMP`, `VARCHAR(n)`) and still resolve
        // to the right storage class.
        match ty {
            SqlType::Boolean => out.push_str("BOOLEAN"),
            SqlType::Integer => out.push_str("INTEGER"),
            SqlType::BigInt => out.push_str("BIGINT"),
            SqlType::Real => out.push_str("REAL"),
            SqlType::Text => out.push_str("TEXT"),
            SqlType::Varchar(length) => {
                out.push_str("VARCHAR(");
                out.push_str(&length.to_string());
                out.push(')');
            }
            SqlType::Timestamp => out.push_str("TIMESTAMP"),
            SqlType::Blob => out.push_str("BLOB"),
            // SQLite has no native JSON/UUID/array types; a `sqlite`-declared project
            // is rejected at compile time before reaching here. Map to TEXT defensively.
            SqlType::Json | SqlType::Uuid | SqlType::Array(_) => out.push_str("TEXT"),
            // SQLite has no native enum; stored as TEXT with a CHECK appended by the
            // DDL renderer.
            SqlType::Enum { .. } => out.push_str("TEXT"),
        }
    }

    fn begin_with_sql(&self, level: IsolationLevel) -> String {
        match level {
            IsolationLevel::Deferred => "BEGIN DEFERRED".to_string(),
            IsolationLevel::Immediate => "BEGIN IMMEDIATE".to_string(),
            IsolationLevel::Exclusive => "BEGIN EXCLUSIVE".to_string(),
            // SQLite is effectively serializable through its locking, so the
            // standard levels map to a plain BEGIN (the closest it offers).
            _ => "BEGIN".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returning_support_follows_the_runtime_sqlite_version() {
        // RETURNING needs SQLite 3.35.0+; the bundled library is well past that, so
        // it is reported as supported, matching the runtime version.
        let supported = SqliteDialect::new().supports_returning();
        assert_eq!(supported, rusqlite::version_number() >= 3_035_000);
        assert!(supported, "the bundled SQLite should support RETURNING");
    }
}
