//! The `down` command.

use tork_orm::migration::FileMigrator;
use tork_orm::OrmError;

use crate::cli::DownTarget;
use crate::output::{self, Action};
use crate::style::Style;

/// Reverts migrations, confirming first when more than one would be reverted.
pub async fn run(
    migrator: &FileMigrator,
    style: &Style,
    target: DownTarget,
    yes: bool,
) -> Result<(), OrmError> {
    let to_revert = preview(migrator, &target).await?;
    if to_revert.is_empty() {
        output::migrations_done(style, Action::Down, &[]);
        return Ok(());
    }

    if to_revert.len() > 1 && !yes {
        output::revert_preview(style, &to_revert);
        if !output::confirm("Proceed?").unwrap_or(false) {
            output::info(style, "Cancelled");
            return Ok(());
        }
    }

    let results = match target {
        DownTarget::Steps(steps) => migrator.down(steps).await?,
        DownTarget::Base => migrator.down_all().await?,
        DownTarget::To(revision) => migrator.down_to(&revision).await?,
    };
    output::migrations_done(style, Action::Down, &results);
    Ok(())
}

/// Computes the (revision, name) list a `down` would revert, newest first.
async fn preview(
    migrator: &FileMigrator,
    target: &DownTarget,
) -> Result<Vec<(String, String)>, OrmError> {
    let applied: Vec<(String, String)> = migrator
        .status()
        .await?
        .into_iter()
        .filter(|entry| entry.applied)
        .map(|entry| (entry.revision, entry.name))
        .collect();

    let revert = match target {
        DownTarget::Steps(steps) => applied.iter().rev().take(*steps).cloned().collect(),
        DownTarget::Base => applied.iter().rev().cloned().collect(),
        DownTarget::To(prefix) => match applied.iter().position(|(r, _)| r.starts_with(prefix)) {
            Some(index) => applied[index + 1..].iter().rev().cloned().collect(),
            None => Vec::new(),
        },
    };
    Ok(revert)
}
