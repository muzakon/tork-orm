//! The `create` command.

use std::path::Path;

use tork_orm::migration::head_revision;
use tork_orm::OrmError;

use crate::scaffold;
use crate::style::{sym, Style};

/// Scaffolds a new migration file, chained onto the current head.
pub fn run(style: &Style, dir: &str, name: &str) -> Result<(), OrmError> {
    let directory = Path::new(dir);
    std::fs::create_dir_all(directory).map_err(|e| {
        OrmError::configuration(format!("cannot create migrations directory `{dir}`: {e}"))
    })?;

    let revision = scaffold::new_revision();
    let down_revision = head_revision(directory)?;
    let snake = scaffold::snake_case(name);
    if snake.is_empty() {
        return Err(OrmError::configuration("the migration name is empty"));
    }

    let file_name = scaffold::file_name(&revision, &snake);
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
