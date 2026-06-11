//! The migration runner.
//!
//! [`Migrator`] applies pending migrations in revision order and records each in a
//! bookkeeping table, and rolls the most recent ones back. The bookkeeping table's
//! reads and writes are rendered through the dialect (via [`QueryWriter`]) so they
//! work on any backend.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::database::Database;
use crate::dialect::QueryWriter;
use crate::value::Value;

use super::ddl::{ColumnSpec, TableDef};
use super::registry::MigrationSet;
use super::render;
use super::schema::SchemaManager;
use crate::dialect::SqlType;

/// The default bookkeeping table name.
const DEFAULT_TABLE: &str = "_tork_migrations";

/// Applies and reverts migrations against a database.
///
/// # Examples
///
/// ```no_run
/// # use tork_orm_core::Database;
/// # use tork_orm_core::migration::{MigrationSet, Migrator};
/// # async fn run(db: Database, set: MigrationSet) -> tork_orm_core::Result<()> {
/// let applied = Migrator::new(&db, set).up().await?;
/// # let _ = applied;
/// # Ok(())
/// # }
/// ```
pub struct Migrator<'d> {
    db: &'d Database,
    set: MigrationSet,
    table: String,
}

impl<'d> Migrator<'d> {
    /// Builds a migrator over `db` for the migrations in `set`.
    pub fn new(db: &'d Database, set: MigrationSet) -> Self {
        Self {
            db,
            set,
            table: DEFAULT_TABLE.to_string(),
        }
    }

    /// Overrides the bookkeeping table name (default `_tork_migrations`).
    pub fn table(mut self, name: &str) -> Self {
        self.table = name.to_string();
        self
    }

    /// Applies every pending migration in revision order.
    ///
    /// Returns the number of migrations applied.
    pub async fn up(&self) -> crate::Result<usize> {
        self.ensure_table().await?;
        let applied = self.applied_revisions().await?;
        // Every migration applied in this run shares one batch number.
        let batch = self.next_batch().await?;
        let mut count = 0;

        for migration in self.set.sorted() {
            if applied.iter().any(|rev| rev == migration.revision()) {
                continue;
            }
            let mut schema = SchemaManager::executing(self.db);
            let start = std::time::Instant::now();
            migration.up(&mut schema).await?;
            let elapsed_ms = start.elapsed().as_millis() as i64;
            self.record(migration.revision(), migration.name(), "", batch, elapsed_ms)
                .await?;
            count += 1;
        }
        Ok(count)
    }

    /// Reverts the most recently applied `steps` migrations.
    ///
    /// Returns the number reverted.
    pub async fn down(&self, steps: usize) -> crate::Result<usize> {
        self.ensure_table().await?;
        let revisions = self.recent_revisions(steps).await?;
        let mut count = 0;

        for revision in revisions {
            let Some(migration) = self.set.find(&revision) else {
                return Err(crate::OrmError::configuration(format!(
                    "applied revision `{revision}` has no migration in the set"
                )));
            };
            let mut schema = SchemaManager::executing(self.db);
            migration.down(&mut schema).await?;
            self.delete_record(&revision).await?;
            count += 1;
        }
        Ok(count)
    }

    /// The schema of the bookkeeping table.
    fn table_def(&self) -> TableDef {
        let text = |name: &str| ColumnSpec::new(name, SqlType::Text);
        let mut id = ColumnSpec::new("id", SqlType::BigInt);
        id.primary_key = true;
        id.auto_increment = true;
        let mut revision = text("revision");
        revision.unique = true;

        TableDef {
            name: self.table.clone(),
            if_not_exists: true,
            columns: vec![
                id,
                revision,
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
    async fn ensure_table(&self) -> crate::Result<()> {
        let dialect = self.db.dialect().as_ref();
        for statement in render::create_table(dialect, &self.table_def()) {
            self.db.execute(statement, Vec::new()).await?;
        }
        Ok(())
    }

    /// Returns every recorded revision.
    async fn applied_revisions(&self) -> crate::Result<Vec<String>> {
        let mut writer = QueryWriter::new(self.db.dialect().as_ref());
        writer.push_sql("SELECT ");
        writer.push_identifier("revision");
        writer.push_sql(" FROM ");
        writer.push_identifier(&self.table);
        let (sql, params) = writer.finish();

        let rows = self.db.fetch_all(sql, params).await?;
        rows.iter().map(|row| row.get::<String>("revision")).collect()
    }

    /// Returns the recorded revisions most-recent first, capped at `limit`.
    async fn recent_revisions(&self, limit: usize) -> crate::Result<Vec<String>> {
        let mut writer = QueryWriter::new(self.db.dialect().as_ref());
        writer.push_sql("SELECT ");
        writer.push_identifier("revision");
        writer.push_sql(" FROM ");
        writer.push_identifier(&self.table);
        writer.push_sql(" ORDER BY ");
        writer.push_identifier("batch");
        writer.push_sql(" DESC, ");
        writer.push_identifier("revision");
        writer.push_sql(" DESC");
        let (sql, params) = writer.finish();

        let rows = self.db.fetch_all(sql, params).await?;
        rows.iter()
            .take(limit)
            .map(|row| row.get::<String>("revision"))
            .collect()
    }

    /// Returns the next batch number (`max(batch) + 1`, or `1`).
    async fn next_batch(&self) -> crate::Result<i64> {
        let mut writer = QueryWriter::new(self.db.dialect().as_ref());
        writer.push_sql("SELECT MAX(");
        writer.push_identifier("batch");
        writer.push_sql(") FROM ");
        writer.push_identifier(&self.table);
        let (sql, params) = writer.finish();

        let rows = self.db.fetch_all(sql, params).await?;
        let current = match rows.first() {
            Some(row) => row.get_index::<Option<i64>>(0)?.unwrap_or(0),
            None => 0,
        };
        Ok(current + 1)
    }

    /// Inserts a bookkeeping row for an applied migration.
    async fn record(
        &self,
        revision: &str,
        name: &str,
        checksum: &str,
        batch: i64,
        execution_time_ms: i64,
    ) -> crate::Result<()> {
        let applied_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let columns = [
            "revision",
            "name",
            "checksum",
            "batch",
            "applied_at",
            "execution_time_ms",
        ];
        let values = vec![
            Value::Text(revision.to_string()),
            Value::Text(name.to_string()),
            Value::Text(checksum.to_string()),
            Value::Int(batch),
            Value::Text(applied_at),
            Value::Int(execution_time_ms),
        ];

        let mut writer = QueryWriter::new(self.db.dialect().as_ref());
        writer.push_sql("INSERT INTO ");
        writer.push_identifier(&self.table);
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

        self.db.execute(sql, params).await?;
        Ok(())
    }

    /// Removes the bookkeeping row for a reverted migration.
    async fn delete_record(&self, revision: &str) -> crate::Result<()> {
        let mut writer = QueryWriter::new(self.db.dialect().as_ref());
        writer.push_sql("DELETE FROM ");
        writer.push_identifier(&self.table);
        writer.push_sql(" WHERE ");
        writer.push_identifier("revision");
        writer.push_sql(" = ");
        writer.push_bind(Value::Text(revision.to_string()));
        let (sql, params) = writer.finish();

        self.db.execute(sql, params).await?;
        Ok(())
    }
}
