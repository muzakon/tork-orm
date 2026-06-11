//! Reading the live database schema.
//!
//! `migrate generate` needs to know what already exists in the database to diff it
//! against the models. These helpers read that state. They are written for SQLite
//! (the only backend in this phase) via its schema table and pragma functions; a
//! future backend would add its own introspection.

use crate::dialect::quote_string_literal;
use crate::executor::Executor;

/// An index that currently exists on a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingIndex {
    /// The index name.
    pub name: String,
    /// Whether the index is unique.
    pub unique: bool,
}

/// Returns whether a table exists in the database.
pub async fn table_exists<E: Executor + Sync>(executor: &E, table: &str) -> crate::Result<bool> {
    let rows = executor
        .fetch_all(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1".to_string(),
            vec![crate::Value::Text(table.to_string())],
        )
        .await?;
    Ok(!rows.is_empty())
}

/// Returns the explicitly created indexes on a table.
///
/// Only indexes created by `CREATE INDEX` are returned (SQLite `origin = 'c'`); the
/// implicit indexes backing `UNIQUE`/`PRIMARY KEY` constraints are excluded, since
/// they are not separately managed.
pub async fn existing_indexes<E: Executor + Sync>(
    executor: &E,
    table: &str,
) -> crate::Result<Vec<ExistingIndex>> {
    let mut sql =
        String::from("SELECT name AS idx_name, \"unique\" AS is_unique FROM pragma_index_list(");
    quote_string_literal(table, &mut sql);
    sql.push_str(") WHERE origin = 'c'");

    let rows = executor.fetch_all(sql, Vec::new()).await?;
    let mut indexes = Vec::with_capacity(rows.len());
    for row in &rows {
        let name: String = row.get("idx_name")?;
        let unique: i64 = row.get("is_unique")?;
        indexes.push(ExistingIndex {
            name,
            unique: unique != 0,
        });
    }
    Ok(indexes)
}
