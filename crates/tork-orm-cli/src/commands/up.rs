//! The `up` command.

use tork_orm::migration::FileMigrator;
use tork_orm::OrmError;

use crate::cli::UpTarget;
use crate::output::{self, Action};
use crate::style::Style;

/// Applies pending migrations, optionally up to a target revision.
pub async fn run(migrator: &FileMigrator, style: &Style, target: UpTarget) -> Result<(), OrmError> {
    let results = match target {
        UpTarget::Head => migrator.up().await?,
        UpTarget::To(revision) => migrator.up_to(&revision).await?,
    };
    output::migrations_done(style, Action::Up, &results);
    Ok(())
}
