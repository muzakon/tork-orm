//! SQL-file migrations: a revision chain of plain `.sql` files.
//!
//! Each migration is a `.sql` file with a `-- revision:` id and a
//! `-- down_revision:` pointer to the one it follows, plus `-- migrate:up` and
//! optional `-- migrate:down` sections. Identity and order come from those
//! headers — the chain, not the filename — so files can be renamed freely.
//!
//! [`FileMigrator`] reads the directory, validates the chain, and applies or
//! reverts migrations, recording each in the shared `_tork_migrations` table with a
//! per-migration transaction. It needs no compilation, so a built binary plus a
//! migrations directory and a database URL is enough to run migrations anywhere.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::database::{Database, Pinned};
use crate::error::OrmError;
use crate::executor::Executor;

use super::checksum::checksum_of;
use super::store;
use super::OnMismatch;

/// A migration applied or reverted by a [`FileMigrator`].
#[derive(Debug, Clone)]
pub struct Applied {
    /// The migration's revision id.
    pub revision: String,
    /// The migration's display name.
    pub name: String,
    /// How long it took.
    pub elapsed: Duration,
}

/// The applied state of one SQL-file migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    /// The revision id.
    pub revision: String,
    /// The display name.
    pub name: String,
    /// Whether the migration is applied.
    pub applied: bool,
    /// For an applied migration, whether its stored checksum still matches the
    /// file; `None` when not applied.
    pub checksum_matches: Option<bool>,
}

/// A parsed migration file.
struct MigrationFile {
    revision: String,
    down_revision: Option<String>,
    name: String,
    up_sql: String,
    down_sql: Option<String>,
    transactional: bool,
    checksum: String,
}

/// Applies and reverts SQL-file migrations from a directory.
///
/// # Examples
///
/// ```no_run
/// # use tork_orm_core::{Database, migration::FileMigrator};
/// # async fn run(db: Database) -> tork_orm_core::Result<()> {
/// let applied = FileMigrator::new(db, "migrations").up().await?;
/// # let _ = applied;
/// # Ok(())
/// # }
/// ```
pub struct FileMigrator {
    db: Database,
    dir: PathBuf,
    table: String,
    on_mismatch: OnMismatch,
    allow_destructive: bool,
}

impl FileMigrator {
    /// Builds a migrator over the migration files in `dir`.
    pub fn new(db: Database, dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            dir: dir.into(),
            table: "_tork_migrations".to_string(),
            on_mismatch: OnMismatch::Error,
            allow_destructive: false,
        }
    }

    /// Overrides the bookkeeping table name (default `_tork_migrations`).
    pub fn table(mut self, name: &str) -> Self {
        self.table = name.to_string();
        self
    }

    /// Sets how a changed-since-applied checksum is handled (default
    /// [`OnMismatch::Error`], which aborts so an edited applied migration cannot
    /// silently drift in production).
    pub fn on_checksum_mismatch(mut self, on_mismatch: OnMismatch) -> Self {
        self.on_mismatch = on_mismatch;
        self
    }

    /// Allows destructive statements (`DROP TABLE`, `DROP COLUMN`) in
    /// migrations (default `false`). Migrations with destructive SQL abort
    /// at run time when this is `false`; pass `true` to opt in.
    pub fn allow_destructive(mut self, allow: bool) -> Self {
        self.allow_destructive = allow;
        self
    }

    /// Applies every pending migration in chain order.
    pub async fn up(&self) -> crate::Result<Vec<Applied>> {
        self.up_through(None).await
    }

    /// Applies pending migrations up to and including `revision` (a unique prefix).
    pub async fn up_to(&self, revision: &str) -> crate::Result<Vec<Applied>> {
        self.up_through(Some(revision)).await
    }

    async fn up_through(&self, target: Option<&str>) -> crate::Result<Vec<Applied>> {
        let chain = self.load_chain()?;
        let target = match target {
            Some(prefix) => Some(resolve(&chain, prefix)?),
            None => None,
        };

        let pinned = self.db.pinned().await?;
        store::ensure_table(&pinned, &self.table).await?;

        // Serialize concurrent migrators (for example several instances booting at
        // once during an autoscaling deploy) so they cannot race on the chain.
        self.acquire_lock(&pinned).await?;
        let result = async {
            let records = store::applied_records(&pinned, &self.table).await?;
            let applied: HashMap<&str, &str> = records
                .iter()
                .map(|record| (record.revision.as_str(), record.checksum.as_str()))
                .collect();
            let batch = store::next_batch(&pinned, &self.table).await?;

            let mut result = Vec::new();
            for file in &chain {
                let is_target = target.as_deref() == Some(file.revision.as_str());
                if let Some(stored) = applied.get(file.revision.as_str()) {
                    if *stored != file.checksum {
                        self.report_mismatch(&file.revision, stored, &file.checksum)?;
                    }
                    if is_target {
                        break;
                    }
                    continue;
                }
                let elapsed = self.apply_up(&pinned, file, batch).await?;
                result.push(Applied {
                    revision: file.revision.clone(),
                    name: file.name.clone(),
                    elapsed,
                });
                if is_target {
                    break;
                }
            }
            Ok(result)
        }
        .await;
        self.release_lock(&pinned).await;
        result
    }

    /// Reverts the most recently applied `steps` migrations.
    pub async fn down(&self, steps: usize) -> crate::Result<Vec<Applied>> {
        let chain = self.load_chain()?;
        let pinned = self.db.pinned().await?;
        store::ensure_table(&pinned, &self.table).await?;

        self.acquire_lock(&pinned).await?;
        let result = async {
            let records = store::applied_records(&pinned, &self.table).await?;
            let applied: HashSet<&str> = records.iter().map(|r| r.revision.as_str()).collect();

            let to_revert: Vec<&MigrationFile> = chain
                .iter()
                .filter(|file| applied.contains(file.revision.as_str()))
                .rev()
                .take(steps)
                .collect();

            let mut result = Vec::new();
            for file in to_revert {
                let elapsed = self.apply_down(&pinned, file).await?;
                result.push(Applied {
                    revision: file.revision.clone(),
                    name: file.name.clone(),
                    elapsed,
                });
            }
            Ok(result)
        }
        .await;
        self.release_lock(&pinned).await;
        result
    }

    /// Reverts every applied migration.
    pub async fn down_all(&self) -> crate::Result<Vec<Applied>> {
        self.down(usize::MAX).await
    }

    /// Reverts every migration applied after `revision` (a unique prefix);
    /// `revision` itself stays applied.
    pub async fn down_to(&self, revision: &str) -> crate::Result<Vec<Applied>> {
        let chain = self.load_chain()?;
        let target = resolve(&chain, revision)?;
        let position = chain
            .iter()
            .position(|file| file.revision == target)
            .expect("resolved revision is in the chain");

        // Count only the migrations strictly after the target that are actually
        // applied. Using the chain length here would over-count when the local
        // chain has unapplied migrations past the target (for example after
        // pulling a branch), making `down` revert the target itself and earlier
        // migrations the caller meant to keep.
        store::ensure_table(&self.db, &self.table).await?;
        let records = store::applied_records(&self.db, &self.table).await?;
        let applied: HashSet<&str> = records.iter().map(|r| r.revision.as_str()).collect();
        let after = chain[position + 1..]
            .iter()
            .filter(|file| applied.contains(file.revision.as_str()))
            .count();
        self.down(after).await
    }

    /// Reverts the most recent migration and re-applies all pending.
    pub async fn redo(&self) -> crate::Result<(usize, usize)> {
        let reverted = self.down(1).await?.len();
        let applied = self.up().await?.len();
        Ok((reverted, applied))
    }

    /// Reports the applied state of every migration in the chain.
    pub async fn status(&self) -> crate::Result<Vec<FileStatus>> {
        let chain = self.load_chain()?;
        store::ensure_table(&self.db, &self.table).await?;
        let records = store::applied_records(&self.db, &self.table).await?;
        let applied: HashMap<&str, &str> = records
            .iter()
            .map(|record| (record.revision.as_str(), record.checksum.as_str()))
            .collect();

        Ok(chain
            .iter()
            .map(|file| {
                let checksum_matches = applied
                    .get(file.revision.as_str())
                    .map(|stored| *stored == file.checksum);
                FileStatus {
                    revision: file.revision.clone(),
                    name: file.name.clone(),
                    applied: checksum_matches.is_some(),
                    checksum_matches,
                }
            })
            .collect())
    }

    /// Applies one migration's up SQL inside a transaction, recording it.
    async fn apply_up(
        &self,
        pinned: &Pinned,
        file: &MigrationFile,
        batch: i64,
    ) -> crate::Result<Duration> {
        if !self.allow_destructive && is_destructive(&file.up_sql) {
            return Err(OrmError::configuration(
                "cannot run destructive migration without --allow-destructive",
            ));
        }
        if file.transactional {
            pinned.execute(self.begin_sql(), Vec::new()).await?;
        }
        let start = Instant::now();

        if let Err(error) = pinned.execute_batch(file.up_sql.clone()).await {
            self.rollback_if(pinned, file.transactional).await;
            return Err(error);
        }
        let elapsed = start.elapsed();
        let record = store::record(
            pinned,
            &self.table,
            &file.revision,
            file.down_revision.as_deref(),
            &file.name,
            &file.checksum,
            batch,
            elapsed.as_millis() as i64,
        )
        .await;
        if let Err(error) = record {
            self.rollback_if(pinned, file.transactional).await;
            return Err(error);
        }

        if file.transactional {
            pinned.execute(self.commit_sql(), Vec::new()).await?;
        }
        Ok(elapsed)
    }

    /// Reverts one migration's down SQL inside a transaction, deleting its record.
    async fn apply_down(&self, pinned: &Pinned, file: &MigrationFile) -> crate::Result<Duration> {
        let down_sql = file.down_sql.as_ref().ok_or_else(|| {
            OrmError::configuration(format!(
                "migration `{}` has no `-- migrate:down` section to revert",
                file.name
            ))
        })?;

        if file.transactional {
            pinned.execute(self.begin_sql(), Vec::new()).await?;
        }
        let start = Instant::now();

        if let Err(error) = pinned.execute_batch(down_sql.clone()).await {
            self.rollback_if(pinned, file.transactional).await;
            return Err(error);
        }
        let elapsed = start.elapsed();
        if let Err(error) = store::delete_record(pinned, &self.table, &file.revision).await {
            self.rollback_if(pinned, file.transactional).await;
            return Err(error);
        }

        if file.transactional {
            pinned.execute(self.commit_sql(), Vec::new()).await?;
        }
        Ok(elapsed)
    }

    /// Rolls back when `transactional` (best effort).
    async fn rollback_if(&self, pinned: &Pinned, transactional: bool) {
        if transactional {
            let _ = pinned.execute(self.rollback_sql(), Vec::new()).await;
        }
    }

    /// A stable advisory-lock key derived from the bookkeeping table name, so
    /// every instance contends for the same lock.
    fn lock_key(&self) -> i64 {
        // FNV-1a over the table name.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in self.table.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash as i64
    }

    /// Takes the dialect's migration advisory lock, if it has one. Blocks until
    /// granted, so concurrent migrators run one at a time.
    async fn acquire_lock(&self, pinned: &Pinned) -> crate::Result<()> {
        if let Some(sql) = self.db.dialect().acquire_migration_lock_sql(self.lock_key()) {
            pinned.fetch_all(sql, Vec::new()).await?;
        }
        Ok(())
    }

    /// Releases the dialect's migration advisory lock (best effort; a session
    /// lock is also dropped automatically when the connection ends).
    async fn release_lock(&self, pinned: &Pinned) {
        if let Some(sql) = self.db.dialect().release_migration_lock_sql(self.lock_key()) {
            let _ = pinned.fetch_all(sql, Vec::new()).await;
        }
    }

    fn begin_sql(&self) -> String {
        self.db.dialect().begin_sql().to_string()
    }

    fn commit_sql(&self) -> String {
        self.db.dialect().commit_sql().to_string()
    }

    fn rollback_sql(&self) -> String {
        self.db.dialect().rollback_sql().to_string()
    }

    /// Handles a checksum mismatch per the configured policy.
    fn report_mismatch(&self, revision: &str, stored: &str, computed: &str) -> crate::Result<()> {
        let message = format!(
            "migration checksum mismatch: `{revision}` was applied with checksum \
             {stored} but its file now hashes to {computed}"
        );
        match self.on_mismatch {
            OnMismatch::Error => Err(OrmError::configuration(message)),
            OnMismatch::Warn => {
                eprintln!("tork-orm: {message}");
                Ok(())
            }
        }
    }

    /// Reads and validates the migration chain from the directory.
    fn load_chain(&self) -> crate::Result<Vec<MigrationFile>> {
        build_chain(load_dir(&self.dir)?)
    }
}

/// Returns `true` if `sql` contains a destructive statement
/// (`DROP TABLE` or `DROP COLUMN`). The check is intentionally simple: a
/// case-insensitive substring match against the unparameterized raw text
/// of the migration. It cannot catch every possible data-destroying
/// statement (for example a `TRUNCATE` or a hand-written `DELETE`), but
/// it covers the two the production-readiness checklist calls out.
fn is_destructive(sql: &str) -> bool {
    let upper = sql.to_uppercase();
    upper.contains("DROP TABLE") || upper.contains("DROP COLUMN")
}

/// Returns the head revision of the chain in `dir` — the last migration — or
/// `None` if the directory is empty or absent.
///
/// Used to chain a newly created migration onto the most recent one without a
/// database connection.
pub fn head_revision(dir: &Path) -> crate::Result<Option<String>> {
    if !dir.exists() {
        return Ok(None);
    }
    let chain = build_chain(load_dir(dir)?)?;
    Ok(chain.last().map(|file| file.revision.clone()))
}

/// Reads every `*.sql` migration file in `dir`.
fn load_dir(dir: &Path) -> crate::Result<Vec<MigrationFile>> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        OrmError::configuration(format!("cannot read migrations directory `{}`: {e}", dir.display()))
    })?;

    let mut files = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| OrmError::configuration(format!("cannot read a directory entry: {e}")))?
            .path();
        if path.extension().and_then(|e| e.to_str()) != Some("sql") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let content = std::fs::read_to_string(&path).map_err(|e| {
            OrmError::configuration(format!("cannot read `{}`: {e}", path.display()))
        })?;
        files.push(parse(&content, &stem)?);
    }
    Ok(files)
}

/// Parses a migration file's headers and up/down sections.
fn parse(content: &str, stem: &str) -> crate::Result<MigrationFile> {
    #[derive(Clone, Copy)]
    enum Section {
        None,
        Up,
        Down,
    }

    let mut revision = None;
    let mut down_revision = None;
    let mut transactional = true;
    let mut up = String::new();
    let mut down = String::new();
    let mut section = Section::None;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("-- revision:") {
            revision = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("-- down_revision:") {
            let value = rest.trim();
            down_revision = (!value.is_empty() && value != "none").then(|| value.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("-- migrate:up") {
            section = Section::Up;
            if rest.contains("transaction:false") {
                transactional = false;
            }
        } else if trimmed.starts_with("-- migrate:down") {
            section = Section::Down;
        } else {
            match section {
                Section::Up => {
                    up.push_str(line);
                    up.push('\n');
                }
                Section::Down => {
                    down.push_str(line);
                    down.push('\n');
                }
                Section::None => {}
            }
        }
    }

    let revision = revision.ok_or_else(|| {
        OrmError::configuration(format!("migration `{stem}` is missing a `-- revision:` header"))
    })?;
    if up.trim().is_empty() {
        return Err(OrmError::configuration(format!(
            "migration `{stem}` is missing a `-- migrate:up` section"
        )));
    }
    let name = stem
        .strip_prefix(&format!("{revision}_"))
        .unwrap_or(stem)
        .to_string();
    let checksum = checksum_of(&[up.clone()]);

    Ok(MigrationFile {
        revision,
        down_revision,
        name,
        up_sql: up,
        down_sql: (!down.trim().is_empty()).then_some(down),
        transactional,
        checksum,
    })
}

/// Validates the migrations form a single linear chain and returns them ordered
/// base to head.
fn build_chain(files: Vec<MigrationFile>) -> crate::Result<Vec<MigrationFile>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_revision: HashMap<String, MigrationFile> = HashMap::new();
    for file in files {
        if by_revision.contains_key(&file.revision) {
            return Err(OrmError::configuration(format!(
                "two migrations share the revision `{}`",
                file.revision
            )));
        }
        by_revision.insert(file.revision.clone(), file);
    }

    // Find the single base (no down_revision) and each parent's single child.
    let mut base = None;
    let mut child_of: HashMap<String, String> = HashMap::new();
    for file in by_revision.values() {
        match &file.down_revision {
            None => {
                if base.is_some() {
                    return Err(OrmError::configuration(
                        "more than one base migration (with no `down_revision`)",
                    ));
                }
                base = Some(file.revision.clone());
            }
            Some(parent) => {
                if !by_revision.contains_key(parent) {
                    return Err(OrmError::configuration(format!(
                        "migration `{}` has down_revision `{parent}`, which does not exist",
                        file.revision
                    )));
                }
                if child_of.contains_key(parent) {
                    return Err(OrmError::configuration(format!(
                        "branching not supported yet: two migrations follow `{parent}`"
                    )));
                }
                child_of.insert(parent.clone(), file.revision.clone());
            }
        }
    }

    let base = base.ok_or_else(|| {
        OrmError::configuration("no base migration (every migration has a `down_revision`)")
    })?;

    // Walk the chain from the base.
    let mut ordered = Vec::with_capacity(by_revision.len());
    let mut seen = HashSet::new();
    let mut current = base;
    loop {
        if !seen.insert(current.clone()) {
            return Err(OrmError::configuration("the migration chain has a cycle"));
        }
        ordered.push(current.clone());
        match child_of.get(&current) {
            Some(next) => current = next.clone(),
            None => break,
        }
    }
    if ordered.len() != by_revision.len() {
        return Err(OrmError::configuration(
            "the migrations do not form a single connected chain",
        ));
    }

    Ok(ordered
        .into_iter()
        .map(|revision| by_revision.remove(&revision).expect("revision in chain"))
        .collect())
}

/// Resolves a revision prefix to a single full revision within the chain.
fn resolve(chain: &[MigrationFile], prefix: &str) -> crate::Result<String> {
    let matches: Vec<&str> = chain
        .iter()
        .map(|file| file.revision.as_str())
        .filter(|revision| revision.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [single] => Ok(single.to_string()),
        [] => Err(OrmError::configuration(format!(
            "no migration matches revision `{prefix}`"
        ))),
        _ => Err(OrmError::configuration(format!(
            "revision `{prefix}` is ambiguous; give more characters"
        ))),
    }
}
