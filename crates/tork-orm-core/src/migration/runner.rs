//! The migration runner.
//!
//! [`Migrator`] applies pending migrations in revision order and records each in a
//! bookkeeping table, and rolls the most recent ones back. The bookkeeping table's
//! reads and writes are rendered through the dialect (via [`QueryWriter`]) so they
//! work on any backend.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::database::Database;
use crate::dialect::{Dialect, QueryWriter};
use crate::executor::Executor;
use crate::value::Value;

use super::checksum::checksum_of;
use super::ddl::{ColumnSpec, TableDef};
use super::registry::{MigrationSet, MigrationTrait, MigrationTransaction};
use super::render;
use super::schema::SchemaManager;
use crate::dialect::SqlType;

/// The default bookkeeping table name.
const DEFAULT_TABLE: &str = "_tork_migrations";

/// What to do when an already-applied migration's checksum no longer matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnMismatch {
    /// Print a warning and continue (the default).
    Warn,
    /// Fail with an error.
    Error,
}

/// The applied state of one migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    /// The revision id.
    pub revision: String,
    /// The migration name.
    pub name: String,
    /// Whether the migration has been applied.
    pub applied: bool,
    /// For an applied migration, whether its stored checksum still matches what it
    /// renders to now; `None` when not applied.
    pub checksum_matches: Option<bool>,
}

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
    on_mismatch: OnMismatch,
}

impl<'d> Migrator<'d> {
    /// Builds a migrator over `db` for the migrations in `set`.
    pub fn new(db: &'d Database, set: MigrationSet) -> Self {
        Self {
            db,
            set,
            table: DEFAULT_TABLE.to_string(),
            on_mismatch: OnMismatch::Warn,
        }
    }

    /// Overrides the bookkeeping table name (default `_tork_migrations`).
    pub fn table(mut self, name: &str) -> Self {
        self.table = name.to_string();
        self
    }

    /// Sets how a changed-since-applied checksum is handled (default
    /// [`OnMismatch::Warn`]).
    pub fn on_checksum_mismatch(mut self, on_mismatch: OnMismatch) -> Self {
        self.on_mismatch = on_mismatch;
        self
    }

    /// Applies every pending migration in revision order.
    ///
    /// Already-applied migrations are checked for a checksum match; a mismatch is
    /// handled per [`on_checksum_mismatch`](Self::on_checksum_mismatch). Returns the
    /// number of migrations applied.
    pub async fn up(&self) -> crate::Result<usize> {
        // Pin one connection so each migration's statements (including BEGIN/COMMIT)
        // run on the same connection regardless of the pool size.
        let executor = self.db.pinned().await?;
        self.ensure_table(&executor).await?;
        let records = self.applied_records(&executor).await?;
        // Every migration applied in this run shares one batch number.
        let batch = self.next_batch(&executor).await?;
        let mut count = 0;

        for migration in self.set.sorted() {
            let checksum = self.checksum_for(migration).await?;
            if let Some((_, stored)) = records
                .iter()
                .find(|(revision, _)| revision == migration.revision())
            {
                if stored != &checksum {
                    self.report_mismatch(migration.revision(), stored, &checksum)?;
                }
                continue;
            }

            let transactional = migration.transaction() == MigrationTransaction::Enabled;
            self.begin(&executor, transactional).await?;
            match self.apply_up(&executor, migration, &checksum, batch).await {
                Ok(()) => {
                    self.commit(&executor, transactional).await?;
                    count += 1;
                }
                Err(error) => {
                    self.rollback(&executor, transactional).await;
                    return Err(error);
                }
            }
        }
        Ok(count)
    }

    /// Applies one migration's `up` and records it.
    async fn apply_up<E: Executor + Sync>(
        &self,
        executor: &E,
        migration: &dyn MigrationTrait,
        checksum: &str,
        batch: i64,
    ) -> crate::Result<()> {
        let mut schema = SchemaManager::executing(executor);
        let start = std::time::Instant::now();
        migration.up(&mut schema).await?;
        let elapsed_ms = start.elapsed().as_millis() as i64;
        self.record(
            executor,
            migration.revision(),
            migration.name(),
            checksum,
            batch,
            elapsed_ms,
        )
        .await
    }

    /// Returns the dialect used to render bookkeeping SQL and transaction control.
    fn dialect(&self) -> &dyn Dialect {
        self.db.dialect().as_ref()
    }

    /// Begins a transaction when `enabled`.
    async fn begin<E: Executor + Sync>(&self, executor: &E, enabled: bool) -> crate::Result<()> {
        if enabled {
            executor
                .execute(self.dialect().begin_sql().to_string(), Vec::new())
                .await?;
        }
        Ok(())
    }

    /// Commits a transaction when `enabled`.
    async fn commit<E: Executor + Sync>(&self, executor: &E, enabled: bool) -> crate::Result<()> {
        if enabled {
            executor
                .execute(self.dialect().commit_sql().to_string(), Vec::new())
                .await?;
        }
        Ok(())
    }

    /// Rolls a transaction back when `enabled` (best effort).
    async fn rollback<E: Executor + Sync>(&self, executor: &E, enabled: bool) {
        if enabled {
            let _ = executor
                .execute(self.dialect().rollback_sql().to_string(), Vec::new())
                .await;
        }
    }

    /// Reports the applied state of every migration in the set.
    pub async fn status(&self) -> crate::Result<Vec<MigrationStatus>> {
        self.ensure_table(self.db).await?;
        let records = self.applied_records(self.db).await?;
        let mut statuses = Vec::new();

        for migration in self.set.sorted() {
            let checksum = self.checksum_for(migration).await?;
            let checksum_matches = records
                .iter()
                .find(|(revision, _)| revision == migration.revision())
                .map(|(_, stored)| stored == &checksum);
            statuses.push(MigrationStatus {
                revision: migration.revision().to_string(),
                name: migration.name().to_string(),
                applied: checksum_matches.is_some(),
                checksum_matches,
            });
        }
        Ok(statuses)
    }

    /// Computes a migration's checksum by rendering its `up` in collect mode.
    async fn checksum_for(&self, migration: &dyn MigrationTrait) -> crate::Result<String> {
        let dialect = self.db.dialect().as_ref();
        let mut schema = SchemaManager::collect(dialect);
        migration.up(&mut schema).await?;
        Ok(checksum_of(&schema.into_collected()))
    }

    /// Handles a checksum mismatch per the configured policy.
    fn report_mismatch(
        &self,
        revision: &str,
        stored: &str,
        computed: &str,
    ) -> crate::Result<()> {
        let message = format!(
            "migration checksum mismatch: `{revision}` was applied with checksum \
             {stored} but now renders to {computed}"
        );
        match self.on_mismatch {
            OnMismatch::Error => Err(crate::OrmError::configuration(message)),
            OnMismatch::Warn => {
                eprintln!("tork-orm: {message}");
                Ok(())
            }
        }
    }

    /// Reverts the most recently applied `steps` migrations.
    ///
    /// Returns the number reverted.
    pub async fn down(&self, steps: usize) -> crate::Result<usize> {
        let executor = self.db.pinned().await?;
        self.ensure_table(&executor).await?;
        let revisions = self.recent_revisions(&executor, steps).await?;
        let mut count = 0;

        for revision in revisions {
            let Some(migration) = self.set.find(&revision) else {
                return Err(crate::OrmError::configuration(format!(
                    "applied revision `{revision}` has no migration in the set"
                )));
            };
            let transactional = migration.transaction() == MigrationTransaction::Enabled;
            self.begin(&executor, transactional).await?;
            match self.apply_down(&executor, migration, &revision).await {
                Ok(()) => {
                    self.commit(&executor, transactional).await?;
                    count += 1;
                }
                Err(error) => {
                    self.rollback(&executor, transactional).await;
                    return Err(error);
                }
            }
        }
        Ok(count)
    }

    /// Reverts one migration's `down` and removes its bookkeeping row.
    async fn apply_down<E: Executor + Sync>(
        &self,
        executor: &E,
        migration: &dyn MigrationTrait,
        revision: &str,
    ) -> crate::Result<()> {
        let mut schema = SchemaManager::executing(executor);
        migration.down(&mut schema).await?;
        self.delete_record(executor, revision).await
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
    async fn ensure_table<E: Executor + Sync>(&self, executor: &E) -> crate::Result<()> {
        for statement in render::create_table(executor.dialect(), &self.table_def()) {
            executor.execute(statement, Vec::new()).await?;
        }
        Ok(())
    }

    /// Returns every recorded `(revision, checksum)` pair.
    async fn applied_records<E: Executor + Sync>(
        &self,
        executor: &E,
    ) -> crate::Result<Vec<(String, String)>> {
        let mut writer = QueryWriter::new(executor.dialect());
        writer.push_sql("SELECT ");
        writer.push_identifier("revision");
        writer.push_sql(", ");
        writer.push_identifier("checksum");
        writer.push_sql(" FROM ");
        writer.push_identifier(&self.table);
        let (sql, params) = writer.finish();

        let rows = executor.fetch_all(sql, params).await?;
        rows.iter()
            .map(|row| Ok((row.get::<String>("revision")?, row.get::<String>("checksum")?)))
            .collect()
    }

    /// Returns the recorded revisions most-recent first, capped at `limit`.
    async fn recent_revisions<E: Executor + Sync>(
        &self,
        executor: &E,
        limit: usize,
    ) -> crate::Result<Vec<String>> {
        let mut writer = QueryWriter::new(executor.dialect());
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

        let rows = executor.fetch_all(sql, params).await?;
        rows.iter()
            .take(limit)
            .map(|row| row.get::<String>("revision"))
            .collect()
    }

    /// Returns the next batch number (`max(batch) + 1`, or `1`).
    async fn next_batch<E: Executor + Sync>(&self, executor: &E) -> crate::Result<i64> {
        let mut writer = QueryWriter::new(executor.dialect());
        writer.push_sql("SELECT MAX(");
        writer.push_identifier("batch");
        writer.push_sql(") FROM ");
        writer.push_identifier(&self.table);
        let (sql, params) = writer.finish();

        let rows = executor.fetch_all(sql, params).await?;
        let current = match rows.first() {
            Some(row) => row.get_index::<Option<i64>>(0)?.unwrap_or(0),
            None => 0,
        };
        Ok(current + 1)
    }

    /// Inserts a bookkeeping row for an applied migration.
    async fn record<E: Executor + Sync>(
        &self,
        executor: &E,
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

        let mut writer = QueryWriter::new(executor.dialect());
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

        executor.execute(sql, params).await?;
        Ok(())
    }

    /// Removes the bookkeeping row for a reverted migration.
    async fn delete_record<E: Executor + Sync>(
        &self,
        executor: &E,
        revision: &str,
    ) -> crate::Result<()> {
        let mut writer = QueryWriter::new(executor.dialect());
        writer.push_sql("DELETE FROM ");
        writer.push_identifier(&self.table);
        writer.push_sql(" WHERE ");
        writer.push_identifier("revision");
        writer.push_sql(" = ");
        writer.push_bind(Value::Text(revision.to_string()));
        let (sql, params) = writer.finish();

        executor.execute(sql, params).await?;
        Ok(())
    }
}
