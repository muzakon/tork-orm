//! The `init` command.

use std::path::Path;

use tork_orm::OrmError;

use crate::style::{sym, Style};

/// Creates the migrations directory.
pub fn run(style: &Style, dir: &str, yes: bool) -> Result<(), OrmError> {
    let directory = Path::new(dir);

    if directory.exists() {
        let non_empty = std::fs::read_dir(directory)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if non_empty && !yes {
            return Err(OrmError::configuration(format!(
                "`{dir}` already exists and is not empty (pass --yes to use it anyway)"
            )));
        }
    } else {
        std::fs::create_dir_all(directory).map_err(|e| {
            OrmError::configuration(format!("cannot create `{dir}`: {e}"))
        })?;
    }

    println!(
        "\n  {} {}  {}\n",
        style.green(sym::CHECK),
        style.dim("Migrations directory"),
        style.cyan(dir),
    );
    Ok(())
}
