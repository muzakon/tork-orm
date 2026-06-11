# 1. Introduction

Tork ORM is a lightweight, asynchronous Object-Relational Mapper (ORM) for Rust, inspired by Python's Tortoise ORM and Django's QuerySets. It is designed to work hand-in-hand with the Tork web framework, but can also be used as a standalone database layer.

## Philosophy

Most Rust ORMs require you to write raw SQL strings, construct complex query builder structs, or use macros that generate thousands of lines of opaque code. Tork ORM takes a different path:

1. **Model-First Declarations:** You define your database tables as plain Rust structs annotated with `#[derive(Model)]`. This single struct generates metadata, column handles, index specifications, and serialization/deserialization logic.
2. **Type-Safe Columns:** Fields on a model compile into typed column handles. If you compare a `String` column to an `i32` value in a filter, it will result in a compile-time error.
3. **Implicitly Async & Safe:** Database drivers are asynchronous and query parameters are bound safely, eliminating SQL injection vectors.
4. **Relations as DTOs:** Relationships are declared declaratively using Rust attributes. Child models can be eager loaded onto parents in a single additional query, completely avoiding the common N+1 query pitfall.

## A Quick Example

Here is a preview of how a Tork ORM model is declared and queried:

```rust
use tork_orm::prelude::*;

// Define the model
#[derive(Debug, Clone, Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50), unique)]
    username: String,
    is_active: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to an in-memory SQLite database
    let db = Database::connect("sqlite://:memory:", 5).await?;

    // Create the schema (typically done via migrations)
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL, is_active INTEGER NOT NULL)".into(),
        vec![]
    ).await?;

    // Insert a new record
    let alice = User::create(
        &db,
        &User {
            id: 0, // Auto-incremented columns are ignored during inserts
            username: "alice".into(),
            is_active: true,
        }
    ).await?;
    println!("Created user with ID: {}", alice.id);

    // Retrieve active users with a type-safe QuerySet
    let active_users = User::query()
        .filter(User::is_active.eq(true))
        .order_by(User::username.asc())
        .all(&db)
        .await?;

    for user in active_users {
        println!("User: {} (Active: {})", user.username, user.is_active);
    }

    Ok(())
}
```

The chapters that follow introduce each of these components in detail. The next chapter covers defining models and fields.
