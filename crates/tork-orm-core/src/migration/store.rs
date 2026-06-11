//! The migration bookkeeping table, shared by both migrators.
//!
//! Both the Rust [`Migrator`](super::Migrator) and the SQL-file
//! [`FileMigrator`](super::FileMigrator) record applied migrations in the same
//! `_tork_migrations` table. These free functions own its schema and its reads and
//! writes, rendered through the dialect so they stay portable.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::dialect::{QueryWriter, SqlType};
use crate::executor::Executor;
use crate::value::Value;

use super::ddl::{ColumnSpec, TableDef};
use super::render;

/// A recorded migration, read back from the bookkeeping table.
pub(crate) struct AppliedRecord {
    pub revision: String,
    #[allow(dead_code)]
    pub down_revision: Option<String>,
    pub checksum: String,
}

/// The schema of the bookkeeping table.
pub(crate) fn table_def(table: &str) -> TableDef {
    let text = |name: &str| ColumnSpec::new(name, SqlType::Text);
    let mut id = ColumnSpec::new("id", SqlType::BigInt);
    id.primary_key = true;
    id.auto_increment = true;
    let mut revision = text("revision");
    revision.unique = true;
    let mut down_revision = text("down_revision");
    down_revision.nullable = true;

    TableDef {
        name: table.to_string(),
        if_not_exists: true,
        columns: vec![
            id,
            revision,
            down_revision,
            text("name"),
            text("checksum"),
            ColumnSpec::new("batch", SqlType::Integer),
            text("applied_at"),
            ColumnSpec::new("execution_time_ms", SqlType::BigInt),
        ],
        primary_key: Vec::new(),
        foreign_keys: Vec::new(),
        indexes: Vec::new(),
    }
}

/// Creates the bookkeeping table if it does not already exist.
pub(crate) async fn ensure_table<E: Executor + Sync>(executor: &E, table: &str) -> crate::Result<()> {
    for statement in render::create_table(executor.dialect(), &table_def(table)) {
        executor.execute(statement, Vec::new()).await?;
    }
    Ok(())
}

/// Returns every recorded migration.
pub(crate) async fn applied_records<E: Executor + Sync>(
    executor: &E,
    table: &str,
) -> crate::Result<Vec<AppliedRecord>> {
    let mut writer = QueryWriter::new(executor.dialect());
    writer.push_sql("SELECT ");
    writer.push_identifier("revision");
    writer.push_sql(", ");
    writer.push_identifier("down_revision");
    writer.push_sql(", ");
    writer.push_identifier("checksum");
    writer.push_sql(" FROM ");
    writer.push_identifier(table);
    let (sql, params) = writer.finish();

    let rows = executor.fetch_all(sql, params).await?;
    rows.iter()
        .map(|row| {
            Ok(AppliedRecord {
                revision: row.get::<String>("revision")?,
                down_revision: row.get::<Option<String>>("down_revision")?,
                checksum: row.get::<String>("checksum")?,
            })
        })
        .collect()
}

/// Returns the recorded revisions most-recent first, capped at `limit`.
pub(crate) async fn recent_revisions<E: Executor + Sync>(
    executor: &E,
    table: &str,
    limit: usize,
) -> crate::Result<Vec<String>> {
    let mut writer = QueryWriter::new(executor.dialect());
    writer.push_sql("SELECT ");
    writer.push_identifier("revision");
    writer.push_sql(" FROM ");
    writer.push_identifier(table);
    writer.push_sql(" ORDER BY ");
    writer.push_identifier("batch");
    writer.push_sql(" DESC, ");
    writer.push_identifier("id");
    writer.push_sql(" DESC");
    let (sql, params) = writer.finish();

    let rows = executor.fetch_all(sql, params).await?;
    rows.iter()
        .take(limit)
        .map(|row| row.get::<String>("revision"))
        .collect()
}

/// Returns the next batch number (`max(batch) + 1`, or `1`).
pub(crate) async fn next_batch<E: Executor + Sync>(
    executor: &E,
    table: &str,
) -> crate::Result<i64> {
    let mut writer = QueryWriter::new(executor.dialect());
    writer.push_sql("SELECT MAX(");
    writer.push_identifier("batch");
    writer.push_sql(") FROM ");
    writer.push_identifier(table);
    let (sql, params) = writer.finish();

    let rows = executor.fetch_all(sql, params).await?;
    let current = match rows.first() {
        Some(row) => row.get_index::<Option<i64>>(0)?.unwrap_or(0),
        None => 0,
    };
    Ok(current + 1)
}

/// Inserts a bookkeeping row for an applied migration.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn record<E: Executor + Sync>(
    executor: &E,
    table: &str,
    revision: &str,
    down_revision: Option<&str>,
    name: &str,
    checksum: &str,
    batch: i64,
    execution_time_ms: i64,
) -> crate::Result<()> {
    let applied_at = OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default();
    let columns = [
        "revision",
        "down_revision",
        "name",
        "checksum",
        "batch",
        "applied_at",
        "execution_time_ms",
    ];
    let values = vec![
        Value::Text(revision.to_string()),
        down_revision.map_or(Value::Null, |d| Value::Text(d.to_string())),
        Value::Text(name.to_string()),
        Value::Text(checksum.to_string()),
        Value::Int(batch),
        Value::Text(applied_at),
        Value::Int(execution_time_ms),
    ];

    let mut writer = QueryWriter::new(executor.dialect());
    writer.push_sql("INSERT INTO ");
    writer.push_identifier(table);
    writer.push_sql(" (");
    for (index, column) in columns.iter().enumerate() {
        if index != 0 {
            writer.push_sql(", ");
        }
        writer.push_identifier(column);
    }
    writer.push_sql(") VALUES (");
    for (index, value) in values.into_iter().enumerate() {
        if index != 0 {
            writer.push_sql(", ");
        }
        writer.push_bind(value);
    }
    writer.push_sql(")");
    let (sql, params) = writer.finish();

    executor.execute(sql, params).await?;
    Ok(())
}

/// Removes the bookkeeping row for a reverted migration.
pub(crate) async fn delete_record<E: Executor + Sync>(
    executor: &E,
    table: &str,
    revision: &str,
) -> crate::Result<()> {
    let mut writer = QueryWriter::new(executor.dialect());
    writer.push_sql("DELETE FROM ");
    writer.push_identifier(table);
    writer.push_sql(" WHERE ");
    writer.push_identifier("revision");
    writer.push_sql(" = ");
    writer.push_bind(Value::Text(revision.to_string()));
    let (sql, params) = writer.finish();

    executor.execute(sql, params).await?;
    Ok(())
}
