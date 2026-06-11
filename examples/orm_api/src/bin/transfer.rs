//! Demonstrates transactions: creating a user and their first post atomically.
//!
//! Both inserts run inside a single transaction. If either fails the whole
//! operation is rolled back, leaving the database unchanged. The example also
//! shows `Transaction::savepoint` for a nested partial rollback.
//!
//! Run from the example directory:
//!
//! ```text
//! DATABASE_URL=sqlite://app.db cargo run -p orm_api --bin transfer
//! ```

use orm_api::models::{Post, User};
use tork_orm::prelude::*;
use tork_orm::transaction;

#[tokio::main]
async fn main() -> tork_orm::Result<()> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
    let db = Database::connect(&url, 4).await?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             username TEXT NOT NULL UNIQUE,
             email TEXT NOT NULL UNIQUE,
             is_active INTEGER NOT NULL DEFAULT 1
         );
         CREATE TABLE IF NOT EXISTS posts (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             user_id INTEGER NOT NULL REFERENCES users(id),
             title TEXT NOT NULL,
             view_count INTEGER NOT NULL DEFAULT 0
         );"
        .to_string(),
    )
    .await?;

    // Closure form: commit on Ok, rollback on Err.
    let (user, post) = transaction!(db, |tx| async move {
        let user = User::create(tx, &User {
            id: 0,
            username: "alice".into(),
            email: "alice@example.com".into(),
            is_active: true,
        })
        .await?;
        let post = Post::create(tx, &Post {
            id: 0,
            user_id: user.id,
            title: "Hello, world!".into(),
            view_count: 0,
        })
        .await?;
        Ok((user, post))
    })
    .await?;

    println!("Created user {} (id={})", user.username, user.id);
    println!("Created post {:?} (id={})", post.title, post.id);

    // Savepoint example: attempt a second post; simulate a failure to show that
    // only the inner work is discarded while the outer transaction commits.
    let result = transaction!(db, |tx| async move {
        let count_before = Post::query().count(tx).await?;

        let _ = tx.savepoint(|sp| Box::pin(async move {
            Post::create(sp, &Post {
                id: 0,
                user_id: user.id,
                title: "Draft (will be rolled back)".into(),
                view_count: 0,
            }).await?;
            // Simulate a business-rule failure after the insert.
            Err::<(), _>(OrmError::query("draft posts not allowed"))
        })).await;

        let count_after = Post::query().count(tx).await?;
        Ok((count_before, count_after))
    })
    .await?;

    println!(
        "Posts before savepoint attempt: {}, after rollback: {} (no net change)",
        result.0, result.1
    );

    // Builder form: IMMEDIATE acquires a write lock before the first statement.
    db.transaction_with()
        .immediate()
        .run(|tx| Box::pin(async move {
            Post::create(tx, &Post {
                id: 0,
                user_id: user.id,
                title: "Committed via IMMEDIATE transaction".into(),
                view_count: 0,
            }).await?;
            Ok(())
        }))
        .await?;

    println!(
        "Total posts: {}",
        Post::query().count(&db).await?
    );

    Ok(())
}
