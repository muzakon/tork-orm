//! Resolving the database URL, migrations directory, and table from flags, the
//! environment, and the project's `[package.metadata.tork]` configuration.

use tork_orm::OrmError;
use tork_orm_config::{Migrations, TorkConfig};

use crate::cli::GlobalArgs;

/// The resolved configuration for a command.
pub struct Config {
    /// The database URL, if one was provided.
    pub database_url: Option<String>,
    /// The migrations directory.
    pub dir: String,
    /// The bookkeeping table name.
    pub table: String,
    /// Migration file-naming settings from `[package.metadata.tork.migrations]`.
    pub migrations: Migrations,
}

impl Config {
    /// Resolves configuration from the global flags, the environment, and the
    /// `[package.metadata.tork]` table of the `Cargo.toml` in the current directory.
    ///
    /// The database URL comes from `--database-url`/`DATABASE_URL` (handled by
    /// clap), then `DB_URL`. The directory is the `--dir` flag, then the configured
    /// `migrations.dir`, then `migrations`. The table defaults to `_tork_migrations`.
    pub fn resolve(global: &GlobalArgs) -> Self {
        let project = std::env::current_dir()
            .map(|dir| TorkConfig::load(&dir))
            .unwrap_or_default();

        let database_url = global
            .database_url
            .clone()
            .or_else(|| std::env::var("DB_URL").ok());
        let dir = global
            .dir
            .clone()
            .unwrap_or_else(|| project.migrations.dir.clone());
        Self {
            database_url,
            dir,
            table: global
                .table
                .clone()
                .unwrap_or_else(|| "_tork_migrations".to_string()),
            migrations: project.migrations,
        }
    }

    /// Returns the database URL, or an error explaining how to provide one.
    pub fn require_database_url(&self) -> Result<&str, OrmError> {
        self.database_url.as_deref().ok_or_else(|| {
            OrmError::configuration(
                "no database URL; pass --database-url or set DATABASE_URL",
            )
        })
    }
}
