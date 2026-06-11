//! App-embedded `migrate generate`.
//!
//! The standalone `tork-orm` binary cannot see an application's Rust models, so
//! schema-diffing generate runs from inside the app, where the model types are
//! linked. This binary diffs the models against a database and writes a migration
//! that reconciles the indexes (and creates any wholly missing table).
//!
//! Run it from the example directory, pointing at a database and the migrations
//! directory:
//!
//! ```text
//! DATABASE_URL=sqlite://app.db MIGRATIONS_DIR=migrations \
//!     cargo run -p orm_api --bin generate
//! ```

use std::path::Path;

use orm_api::models::{Post, User};
use tork_orm::migration::generate::{generate, write_migration};
use tork_orm::prelude::*;

#[tokio::main]
async fn main() -> tork_orm::Result<()> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
    let dir = std::env::var("MIGRATIONS_DIR").unwrap_or_else(|_| "migrations".to_string());

    let db = Database::connect(&url, 1).await?;

    // The explicit model list is shown here; `generate_from_registry` collects every
    // model linked into the binary, which is handier in a larger application.
    let change = generate(&db, &[User::table_schema(), Post::table_schema()]).await?;

    if change.is_empty() {
        println!("Schema is up to date; nothing to generate.");
        return Ok(());
    }

    match write_migration(Path::new(&dir), "auto", &change)? {
        Some(path) => println!("Wrote {}", path.display()),
        None => println!("No changes."),
    }
    Ok(())
}
