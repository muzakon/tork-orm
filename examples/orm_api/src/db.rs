//! Database configuration, lifespan, and initial schema.

use std::sync::Arc;

use tork::{settings, LifespanContext, Resources, Result};
use tork_orm::prelude::*;

use crate::models::{Post, User};

/// Database settings, loaded from the environment (prefix `DB_`).
///
/// Defaults to an in-memory database so the example runs with no setup; point
/// `DB_URL` at a file (for example `sqlite://app.db`) to persist data.
#[settings(prefix = "DB")]
pub struct DatabaseConfig {
    /// The database URL.
    #[setting(default = "sqlite::memory:")]
    pub url: String,
    /// The maximum number of pooled connections.
    #[setting(default = 5, ge = 1, le = 64)]
    pub max_connections: u32,
}

/// The database resource container.
///
/// Built once at startup and registered so handlers receive `Arc<Database>`.
#[derive(Clone, Resources)]
pub struct Db {
    #[resource]
    pub database: Arc<Database>,
}

#[tork::lifespan]
impl Db {
    /// Connects to the database, creates the schema, and seeds sample data.
    async fn startup(_ctx: LifespanContext) -> Result<Self> {
        let config = DatabaseConfig::load()?;
        let database = Database::connect(&config.url, config.max_connections).await?;
        migrate(&database).await?;
        seed_if_empty(&database).await?;
        Ok(Db {
            database: Arc::new(database),
        })
    }

    /// Closes the pool at shutdown.
    async fn shutdown(self) -> Result<()> {
        self.database.close().await;
        Ok(())
    }
}

/// Creates the tables if they do not yet exist.
async fn migrate(db: &Database) -> Result<()> {
    db.execute(
        "CREATE TABLE IF NOT EXISTS users (\
            id INTEGER PRIMARY KEY, \
            username TEXT NOT NULL, \
            email TEXT NOT NULL, \
            is_active INTEGER NOT NULL\
        )"
        .into(),
        vec![],
    )
    .await?;
    db.execute(
        "CREATE TABLE IF NOT EXISTS posts (\
            id INTEGER PRIMARY KEY, \
            user_id INTEGER NOT NULL REFERENCES users(id), \
            title TEXT NOT NULL, \
            view_count INTEGER NOT NULL\
        )"
        .into(),
        vec![],
    )
    .await?;
    Ok(())
}

/// Inserts a small sample data set the first time the database is empty.
async fn seed_if_empty(db: &Database) -> Result<()> {
    if User::query().count(db).await? > 0 {
        return Ok(());
    }

    let alice = User::create(
        db,
        &User {
            id: 0,
            username: "alice".into(),
            email: "alice@example.com".into(),
            is_active: true,
        },
    )
    .await?;
    let bob = User::create(
        db,
        &User {
            id: 0,
            username: "bob".into(),
            email: "bob@example.com".into(),
            is_active: false,
        },
    )
    .await?;

    let post = |user_id: i64, title: &str, view_count: i64| Post {
        id: 0,
        user_id,
        title: title.into(),
        view_count,
    };
    Post::bulk_create(
        db,
        &[
            post(alice.id, "Hello world", 120),
            post(alice.id, "Second post", 80),
            post(bob.id, "Bob writes", 30),
        ],
    )
    .await?;
    Ok(())
}
