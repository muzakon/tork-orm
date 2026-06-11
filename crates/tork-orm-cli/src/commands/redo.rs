//! The `redo` command.

use tork_orm::migration::FileMigrator;
use tork_orm::OrmError;

use crate::style::{sym, Style};

/// Reverts the most recent migration and re-applies all pending.
pub async fn run(migrator: &FileMigrator, style: &Style) -> Result<(), OrmError> {
    let (reverted, applied) = migrator.redo().await?;
    println!(
        "\n  {} Reverted {} and applied {} migration{}\n",
        style.green(sym::CHECK),
        reverted,
        applied,
        if applied == 1 { "" } else { "s" },
    );
    Ok(())
}
