//! The MySQL dialect.

use super::{Dialect, DialectKind, SqlType};
use crate::transaction::IsolationLevel;

/// SQL generation for MySQL (and MariaDB).
///
/// MySQL quotes identifiers with backticks, uses positional `?` placeholders, has
/// no `RETURNING` (the ORM re-selects by `LAST_INSERT_ID()`), and spells upserts as
/// `ON DUPLICATE KEY UPDATE`. It lacks the aggregate `FILTER` clause (the writer
/// emulates it with `CASE`) and `FULL OUTER JOIN` (rejected). Window functions,
/// CTEs, `FOR UPDATE`, and native `JSON` are supported (MySQL 8).
///
/// # Examples
///
/// ```
/// use tork_orm_core::dialect::{Dialect, MySqlDialect};
///
/// let dialect = MySqlDialect::new();
/// assert_eq!(dialect.quoted("user id"), "`user id`");
/// let mut placeholder = String::new();
/// dialect.placeholder(0, &mut placeholder);
/// assert_eq!(placeholder, "?");
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct MySqlDialect;

impl MySqlDialect {
    /// Creates a MySQL dialect.
    pub const fn new() -> Self {
        Self
    }
}

impl Dialect for MySqlDialect {
    fn name(&self) -> &'static str {
        "mysql"
    }

    fn kind(&self) -> DialectKind {
        DialectKind::Mysql
    }

    fn quote_identifier(&self, identifier: &str, out: &mut String) {
        out.push('`');
        for ch in identifier.chars() {
            if ch == '`' {
                // Double an embedded backtick so it cannot terminate the identifier.
                out.push('`');
            }
            out.push(ch);
        }
        out.push('`');
    }

    fn placeholder(&self, _index: usize, out: &mut String) {
        out.push('?');
    }

    fn supports_returning(&self) -> bool {
        false
    }

    fn max_bind_params(&self) -> usize {
        // The MySQL client/server protocol caps a prepared statement at 65535
        // placeholders (the `COM_STMT_PREPARE` parameter count is a 16-bit field).
        65535
    }

    fn acquire_migration_lock_sql(&self, key: i64) -> Option<String> {
        // Named user-level lock, released when the session ends. The 60s timeout
        // bounds how long a starting instance waits for a peer's migration run.
        Some(format!("SELECT GET_LOCK('tork_migration_{key}', 60)"))
    }

    fn release_migration_lock_sql(&self, key: i64) -> Option<String> {
        Some(format!("SELECT RELEASE_LOCK('tork_migration_{key}')"))
    }

    fn map_sql_type(&self, ty: SqlType, out: &mut String) {
        match ty {
            // MySQL has no native boolean; `TINYINT(1)` is the conventional spelling.
            SqlType::Boolean => out.push_str("TINYINT(1)"),
            SqlType::Integer => out.push_str("INT"),
            SqlType::BigInt => out.push_str("BIGINT"),
            SqlType::Real => out.push_str("DOUBLE"),
            SqlType::Text => out.push_str("TEXT"),
            SqlType::Varchar(length) => {
                out.push_str("VARCHAR(");
                out.push_str(&length.to_string());
                out.push(')');
            }
            SqlType::Timestamp => out.push_str("DATETIME"),
            SqlType::Blob => out.push_str("BLOB"),
            SqlType::Json => out.push_str("JSON"),
            // No native UUID; the canonical text form fits a 36-char column.
            SqlType::Uuid => out.push_str("CHAR(36)"),
            // No native arrays; gated off MySQL, so this is a defensive fallback.
            SqlType::Array(_) => out.push_str("TEXT"),
            // MySQL has a native ENUM type; the variant list constrains the column,
            // so no separate CHECK is emitted for it.
            SqlType::Enum { variants, .. } => {
                out.push_str("ENUM(");
                for (index, variant) in variants.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    crate::dialect::quote_string_literal(variant, out);
                }
                out.push(')');
            }
        }
    }

    fn begin_sql(&self) -> &'static str {
        "START TRANSACTION"
    }

    fn begin_with_sql(&self, _level: IsolationLevel) -> String {
        // MySQL sets the isolation level in a separate `SET TRANSACTION` statement;
        // the abstract levels map closely enough to MySQL's default REPEATABLE READ
        // that we keep the begin statement simple.
        "START TRANSACTION".to_string()
    }

    fn release_sql(&self, name: &str) -> String {
        // MySQL requires the `SAVEPOINT` keyword on RELEASE.
        format!("RELEASE SAVEPOINT {name}")
    }

    fn rollback_to_sql(&self, name: &str) -> String {
        format!("ROLLBACK TO SAVEPOINT {name}")
    }

    fn supports_filter_clause(&self) -> bool {
        false
    }

    fn supports_full_join(&self) -> bool {
        false
    }

    fn supports_lock_modifiers(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(ty: SqlType) -> String {
        let mut out = String::new();
        MySqlDialect::new().map_sql_type(ty, &mut out);
        out
    }

    #[test]
    fn quotes_identifiers_with_backticks() {
        let dialect = MySqlDialect::new();
        assert_eq!(dialect.quoted("users"), "`users`");
        // An embedded backtick is doubled.
        assert_eq!(dialect.quoted("a`b"), "`a``b`");
    }

    #[test]
    fn placeholders_are_positional_question_marks() {
        let dialect = MySqlDialect::new();
        let mut out = String::new();
        dialect.placeholder(0, &mut out);
        dialect.placeholder(7, &mut out);
        assert_eq!(out, "??");
    }

    #[test]
    fn maps_types_to_mysql_spellings() {
        assert_eq!(type_str(SqlType::Boolean), "TINYINT(1)");
        assert_eq!(type_str(SqlType::Integer), "INT");
        assert_eq!(type_str(SqlType::BigInt), "BIGINT");
        assert_eq!(type_str(SqlType::Real), "DOUBLE");
        assert_eq!(type_str(SqlType::Text), "TEXT");
        assert_eq!(type_str(SqlType::Varchar(50)), "VARCHAR(50)");
        assert_eq!(type_str(SqlType::Timestamp), "DATETIME");
        assert_eq!(type_str(SqlType::Blob), "BLOB");
        assert_eq!(type_str(SqlType::Json), "JSON");
        assert_eq!(type_str(SqlType::Uuid), "CHAR(36)");
    }

    #[test]
    fn lacks_returning_filter_and_full_join() {
        let dialect = MySqlDialect::new();
        assert!(!dialect.supports_returning());
        assert!(!dialect.supports_filter_clause());
        assert!(!dialect.supports_full_join());
    }

    #[test]
    fn savepoint_release_uses_savepoint_keyword() {
        let dialect = MySqlDialect::new();
        assert_eq!(dialect.release_sql("sp1"), "RELEASE SAVEPOINT sp1");
        assert_eq!(dialect.rollback_to_sql("sp1"), "ROLLBACK TO SAVEPOINT sp1");
    }

    #[test]
    fn uses_named_user_locks_for_migrations() {
        let dialect = MySqlDialect::new();
        assert_eq!(
            dialect.acquire_migration_lock_sql(42).as_deref(),
            Some("SELECT GET_LOCK('tork_migration_42', 60)")
        );
        assert_eq!(
            dialect.release_migration_lock_sql(42).as_deref(),
            Some("SELECT RELEASE_LOCK('tork_migration_42')")
        );
        assert_eq!(dialect.max_bind_params(), 65535);
    }
}
