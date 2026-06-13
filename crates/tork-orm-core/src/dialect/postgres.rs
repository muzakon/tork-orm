//! The PostgreSQL dialect.

use std::fmt::Write;

use super::{Dialect, DialectKind, SqlType};
use crate::transaction::IsolationLevel;

/// SQL generation for PostgreSQL.
///
/// PostgreSQL quotes identifiers with double quotes, uses numbered `$1`, `$2`
/// placeholders, has native `BOOLEAN`/`BYTEA`/timestamp types, supports
/// `RETURNING`, and offers the full set of index features (method, covering
/// columns, operator classes).
///
/// # Examples
///
/// ```
/// use tork_orm_core::dialect::{Dialect, PostgresDialect};
///
/// let dialect = PostgresDialect::new();
/// assert_eq!(dialect.quoted("user id"), "\"user id\"");
/// let mut placeholder = String::new();
/// dialect.placeholder(0, &mut placeholder);
/// assert_eq!(placeholder, "$1");
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDialect;

impl PostgresDialect {
    /// Creates a PostgreSQL dialect.
    pub const fn new() -> Self {
        Self
    }
}

impl Dialect for PostgresDialect {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn kind(&self) -> DialectKind {
        DialectKind::Postgres
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

    fn placeholder(&self, index: usize, out: &mut String) {
        // PostgreSQL placeholders are 1-based; the writer passes a 0-based index.
        let _ = write!(out, "${}", index + 1);
    }

    fn supports_returning(&self) -> bool {
        true
    }

    fn max_bind_params(&self) -> usize {
        // The PostgreSQL wire protocol encodes the parameter count as an
        // unsigned 16-bit integer, capping a single statement at 65535 binds.
        65535
    }

    fn supports_distinct_on(&self) -> bool {
        true
    }

    fn supports_lock_modifiers(&self) -> bool {
        true
    }

    fn map_sql_type(&self, ty: SqlType, out: &mut String) {
        match ty {
            SqlType::Boolean => out.push_str("BOOLEAN"),
            SqlType::Integer => out.push_str("INTEGER"),
            SqlType::BigInt => out.push_str("BIGINT"),
            SqlType::Real => out.push_str("DOUBLE PRECISION"),
            SqlType::Text => out.push_str("TEXT"),
            SqlType::Varchar(length) => {
                out.push_str("VARCHAR(");
                out.push_str(&length.to_string());
                out.push(')');
            }
            SqlType::Timestamp => out.push_str("TIMESTAMP WITH TIME ZONE"),
            SqlType::Blob => out.push_str("BYTEA"),
            SqlType::Json => out.push_str("JSONB"),
            SqlType::Uuid => out.push_str("UUID"),
            SqlType::Array(inner) => {
                self.map_sql_type(*inner, out);
                out.push_str("[]");
            }
            // Portable enum: a text column constrained by a CHECK appended by the DDL
            // renderer (rather than a native `CREATE TYPE`).
            SqlType::Enum { .. } => out.push_str("VARCHAR(255)"),
        }
    }

    fn begin_with_sql(&self, level: IsolationLevel) -> String {
        // PostgreSQL has no lock-mode BEGIN variants; map the abstract levels to
        // the nearest standard isolation level.
        let isolation = match level {
            IsolationLevel::Deferred => "READ COMMITTED",
            IsolationLevel::Immediate => "REPEATABLE READ",
            IsolationLevel::Exclusive => "SERIALIZABLE",
        };
        format!("BEGIN ISOLATION LEVEL {isolation}")
    }

    fn bool_literal(&self, value: bool) -> &'static str {
        if value {
            "true"
        } else {
            "false"
        }
    }

    fn supports_index_method(&self) -> bool {
        true
    }

    fn supports_index_include(&self) -> bool {
        true
    }

    fn supports_index_opclass(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::writer::QueryWriter;
    use crate::value::Value;

    fn type_str(ty: SqlType) -> String {
        let mut out = String::new();
        PostgresDialect::new().map_sql_type(ty, &mut out);
        out
    }

    #[test]
    fn quotes_identifiers_and_escapes_embedded_quotes() {
        let dialect = PostgresDialect::new();
        assert_eq!(dialect.quoted("users"), "\"users\"");
        assert_eq!(dialect.quoted("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn placeholders_are_numbered_from_one() {
        let dialect = PostgresDialect::new();
        let mut out = String::new();
        dialect.placeholder(0, &mut out);
        dialect.placeholder(1, &mut out);
        dialect.placeholder(9, &mut out);
        assert_eq!(out, "$1$2$10");
    }

    #[test]
    fn writer_numbers_bound_params_sequentially() {
        let dialect = PostgresDialect::new();
        let mut writer = QueryWriter::new(&dialect);
        writer.push_sql("VALUES (");
        writer.push_bind(Value::Int(1));
        writer.push_sql(", ");
        writer.push_bind(Value::Text("x".into()));
        writer.push_sql(", ");
        writer.push_bind(Value::Bool(true));
        writer.push_sql(")");
        let (sql, params) = writer.finish();
        assert_eq!(sql, "VALUES ($1, $2, $3)");
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn maps_types_to_postgres_spellings() {
        assert_eq!(type_str(SqlType::Boolean), "BOOLEAN");
        assert_eq!(type_str(SqlType::Integer), "INTEGER");
        assert_eq!(type_str(SqlType::BigInt), "BIGINT");
        assert_eq!(type_str(SqlType::Real), "DOUBLE PRECISION");
        assert_eq!(type_str(SqlType::Text), "TEXT");
        assert_eq!(type_str(SqlType::Varchar(50)), "VARCHAR(50)");
        assert_eq!(type_str(SqlType::Timestamp), "TIMESTAMP WITH TIME ZONE");
        assert_eq!(type_str(SqlType::Blob), "BYTEA");
    }

    #[test]
    fn maps_isolation_levels_to_standard_sql() {
        let dialect = PostgresDialect::new();
        assert_eq!(
            dialect.begin_with_sql(IsolationLevel::Deferred),
            "BEGIN ISOLATION LEVEL READ COMMITTED"
        );
        assert_eq!(
            dialect.begin_with_sql(IsolationLevel::Immediate),
            "BEGIN ISOLATION LEVEL REPEATABLE READ"
        );
        assert_eq!(
            dialect.begin_with_sql(IsolationLevel::Exclusive),
            "BEGIN ISOLATION LEVEL SERIALIZABLE"
        );
    }

    #[test]
    fn renders_native_boolean_literals() {
        let dialect = PostgresDialect::new();
        assert_eq!(dialect.bool_literal(true), "true");
        assert_eq!(dialect.bool_literal(false), "false");
    }

    #[test]
    fn advertises_full_index_capabilities() {
        let dialect = PostgresDialect::new();
        assert!(dialect.supports_index_method());
        assert!(dialect.supports_index_include());
        assert!(dialect.supports_index_opclass());
        assert!(dialect.supports_returning());
    }
}
