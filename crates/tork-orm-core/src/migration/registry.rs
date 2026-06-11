//! The migration trait and the set of migrations to run.
//!
//! A migration is a revision (a sortable id), a name, and `up`/`down` schema
//! changes. Because Rust cannot discover migration files at run time, migrations
//! are collected explicitly into a [`MigrationSet`]. The async `up`/`down` use the
//! project's boxed-future pattern so the trait stays object-safe without an extra
//! dependency; the `#[migration]` macro writes that boilerplate.

use super::schema::SchemaManager;
use super::BoxFuture;

/// Whether a migration runs inside a transaction.
///
/// The default is [`MigrationTransaction::Enabled`]; a migration overrides it to
/// [`MigrationTransaction::Disabled`] for an operation that cannot run in a
/// transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationTransaction {
    /// Run the migration inside a transaction (the default).
    Enabled,
    /// Run the migration outside any transaction.
    Disabled,
}

/// A single migration.
///
/// Usually written with the `#[migration]` macro, which generates this
/// implementation from plain `async fn up`/`down`.
pub trait MigrationTrait: Send + Sync {
    /// The revision id, sortable so revisions apply in order (e.g. a timestamp).
    fn revision(&self) -> &'static str;

    /// A short human-readable name.
    fn name(&self) -> &'static str;

    /// Applies the schema change.
    fn up<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, crate::Result<()>>;

    /// Reverts the schema change.
    fn down<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, crate::Result<()>>;

    /// Whether this migration runs inside a transaction (default: yes).
    fn transaction(&self) -> MigrationTransaction {
        MigrationTransaction::Enabled
    }
}

/// Boxes a migration for inclusion in a [`MigrationSet`].
pub fn boxed<M: MigrationTrait + 'static>(migration: M) -> Box<dyn MigrationTrait> {
    Box::new(migration)
}

/// An ordered collection of migrations to apply.
///
/// # Examples
///
/// ```ignore
/// pub fn migrations() -> MigrationSet {
///     MigrationSet::new(vec![
///         boxed(m20260611_143000_create_users::Migration),
///         boxed(m20260611_143100_create_posts::Migration),
///     ])
/// }
/// ```
pub struct MigrationSet {
    migrations: Vec<Box<dyn MigrationTrait>>,
}

impl MigrationSet {
    /// Builds a set from boxed migrations.
    pub fn new(migrations: Vec<Box<dyn MigrationTrait>>) -> Self {
        Self { migrations }
    }

    /// Returns the number of migrations.
    pub fn len(&self) -> usize {
        self.migrations.len()
    }

    /// Returns `true` if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.migrations.is_empty()
    }

    /// Returns the migrations sorted by revision (ascending).
    pub(crate) fn sorted(&self) -> Vec<&dyn MigrationTrait> {
        let mut sorted: Vec<&dyn MigrationTrait> =
            self.migrations.iter().map(Box::as_ref).collect();
        sorted.sort_by(|a, b| a.revision().cmp(b.revision()));
        sorted
    }

    /// Finds a migration by revision.
    pub(crate) fn find(&self, revision: &str) -> Option<&dyn MigrationTrait> {
        self.migrations
            .iter()
            .map(Box::as_ref)
            .find(|migration| migration.revision() == revision)
    }
}
