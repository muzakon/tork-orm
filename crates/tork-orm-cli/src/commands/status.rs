//! The `status` command.

use tork_orm::migration::FileMigrator;
use tork_orm::OrmError;

use crate::output;
use crate::style::Style;

/// Prints the applied state of every migration.
pub async fn run(migrator: &FileMigrator, style: &Style, dir: &str) -> Result<(), OrmError> {
    let statuses = migrator.status().await?;
    output::status(style, dir, &statuses);
    Ok(())
}
