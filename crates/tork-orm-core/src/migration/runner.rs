//! The migration runner.
//!
//! [`Migrator`] applies pending migrations in revision order and records each in a
//! bookkeeping table, and rolls the most recent ones back. The bookkeeping table's
//! reads and writes are rendered through the dialect (via [`QueryWriter`]) so they
//! work on any backend.

use crate::database::Database;
use crate::dialect::Dialect;
use crate::executor::Executor;

use super::checksum::checksum_of;
use super::registry::{MigrationSet, MigrationTrait, MigrationTransaction};
use super::schema::SchemaManager;
use super::store;

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
        store::ensure_table(&executor, &self.table).await?;
        let records = store::applied_records(&executor, &self.table).await?;
        // Every migration applied in this run shares one batch number.
        let batch = store::next_batch(&executor, &self.table).await?;
        let mut count = 0;

        for migration in self.set.sorted() {
            let checksum = self.checksum_for(migration).await?;
            if let Some(record) = records
                .iter()
                .find(|record| record.revision == migration.revision())
            {
                if record.checksum != checksum {
                    self.report_mismatch(migration.revision(), &record.checksum, &checksum)?;
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
        store::record(
            executor,
            &self.table,
            migration.revision(),
            None,
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
        store::ensure_table(self.db, &self.table).await?;
        let records = store::applied_records(self.db, &self.table).await?;
        let mut statuses = Vec::new();

        for migration in self.set.sorted() {
            let checksum = self.checksum_for(migration).await?;
            let checksum_matches = records
                .iter()
                .find(|record| record.revision == migration.revision())
                .map(|record| record.checksum == checksum);
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
        store::ensure_table(&executor, &self.table).await?;
        let revisions = store::recent_revisions(&executor, &self.table, steps).await?;
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
        store::delete_record(executor, &self.table, revision).await
    }
}
