//! Resolving the database URL, migrations directory, and table from flags and env.

use tork_orm::OrmError;

use crate::cli::GlobalArgs;

/// The resolved configuration for a command.
pub struct Config {
    /// The database URL, if one was provided.
    pub database_url: Option<String>,
    /// The migrations directory.
    pub dir: String,
    /// The bookkeeping table name.
    pub table: String,
}

impl Config {
    /// Resolves configuration from the global flags and the environment.
    ///
    /// The database URL comes from `--database-url`/`DATABASE_URL` (handled by
    /// clap), then `DB_URL`. The directory defaults to `migrations`, the table to
    /// `_tork_migrations`.
    pub fn resolve(global: &GlobalArgs) -> Self {
        let database_url = global
            .database_url
            .clone()
            .or_else(|| std::env::var("DB_URL").ok());
        Self {
            database_url,
            dir: global.dir.clone().unwrap_or_else(|| "migrations".to_string()),
            table: global
                .table
                .clone()
                .unwrap_or_else(|| "_tork_migrations".to_string()),
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
