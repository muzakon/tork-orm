//! The `create` command.

use std::path::Path;

use time::OffsetDateTime;
use tork_orm::migration::head_revision;
use tork_orm::OrmError;
use tork_orm_config::Migrations;

use crate::scaffold::{self, DateParts};
use crate::style::{sym, Style};

/// Scaffolds a new migration file, chained onto the current head.
///
/// The file name follows the configured `file_template` and `revision_style`; the
/// migration's `revision`/`down_revision` chain inside the file is independent of
/// the file name, so files can still be renamed freely.
pub fn run(
    style: &Style,
    dir: &str,
    migrations: &Migrations,
    name: &str,
) -> Result<(), OrmError> {
    let directory = Path::new(dir);
    std::fs::create_dir_all(directory).map_err(|e| {
        OrmError::configuration(format!("cannot create migrations directory `{dir}`: {e}"))
    })?;

    let snake = scaffold::snake_case(name);
    if snake.is_empty() {
        return Err(OrmError::configuration("the migration name is empty"));
    }
    let slug = scaffold::truncate_slug(&snake, migrations.truncate_slug_length);

    let revision = scaffold::revision_id(migrations.revision_style, count_migrations(directory));
    let down_revision = head_revision(directory)?;

    let now = OffsetDateTime::now_utc();
    let date = DateParts {
        year: format!("{:04}", now.year()),
        month: format!("{:02}", u8::from(now.month())),
        day: format!("{:02}", now.day()),
        hour: format!("{:02}", now.hour()),
        minute: format!("{:02}", now.minute()),
        second: format!("{:02}", now.second()),
    };

    let file_name = scaffold::render_file_name(&migrations.file_template, &revision, &slug, &date);
    let path = directory.join(&file_name);
    if path.exists() {
        return Err(OrmError::configuration(format!(
            "`{}` already exists",
            path.display()
        )));
    }

    let content = scaffold::template(&revision, down_revision.as_deref(), &snake);
    std::fs::write(&path, content)
        .map_err(|e| OrmError::configuration(format!("cannot write `{}`: {e}", path.display())))?;

    println!(
        "\n  {} {}  {}\n",
        style.green(sym::CHECK),
        style.dim("Created"),
        style.cyan(&path.display().to_string()),
    );
    Ok(())
}

/// Counts the existing `.sql` migration files in `directory`.
///
/// Used to derive the next number for the `sequence` revision style. A missing or
/// unreadable directory counts as zero.
fn count_migrations(directory: &Path) -> usize {
    std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry.path().extension().and_then(|ext| ext.to_str()) == Some("sql")
                })
                .count()
        })
        .unwrap_or(0)
}
