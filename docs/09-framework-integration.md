# 9. Tork Framework Integration

Tork ORM is designed to integrate seamlessly with the Tork web framework. It provides automatic resource dependency injection and an error conversion bridge that maps database results directly to HTTP statuses.

---

## 1. Dependency Injection (`Arc<Database>`)

To make the database connection pool available in your handlers, register it as shared state when constructing the Tork `App`. Tork's blanket resource extractor will automatically inject it into any route handler that asks for `Arc<Database>`.

```rust
use std::sync::Arc;
use tork::App;
use tork_orm::prelude::Database;

#[tork::main]
async fn main() -> tork::Result<()> {
    // 1. Establish the database connection pool
    let db = Database::connect("sqlite://app.db", 5).await?;
    let db_resource = Arc::new(db);

    // 2. Register it as application state
    App::new()
        .state(db_resource)
        .include(get_user)
        .include(count_users)
        .serve("0.0.0.0:8000")
        .await
}
```

---

## 2. The HTTP Error Bridge

When you execute queries inside route handlers, you can use the standard Rust `?` operator. Tork ORM implements `From<OrmError>` for Tork's HTTP `Error`. 

The bridge converts database failures to the appropriate HTTP status codes:

| ORM Error Kind | HTTP Status Code | Description |
|---|---|---|
| `ErrorKind::Connection` | `503 Service Unavailable` | Database pool is exhausted or connection was lost. |
| `ErrorKind::NotFound` | `404 Not Found` | A `.one()` query returned zero rows. |
| `ErrorKind::MultipleFound` | `500 Internal Server Error` | A `.one()` query returned more than one row. |
| `ErrorKind::Query` | `500 Internal Server Error` | SQL syntax error or constraint violation. |
| `ErrorKind::Conversion` | `500 Internal Server Error` | Mismatch between database column type and Rust field type. |

---

## 3. Example Route Handlers

Here is how database injection and error mapping look in practice:

```rust
use std::sync::Arc;
use serde_json::json;
use tork::{get, Result};
use tork_orm::prelude::*;

// 1. Fetching a single user
// If User::id does not exist, .one() returns ErrorKind::NotFound.
// The error bridge automatically converts this into a 404 response.
#[get("/users/{id}")]
async fn get_user(id: i64, db: Arc<Database>) -> Result<serde_json::Value> {
    let user = User::query()
        .filter(User::id.eq(id))
        .one(&db)
        .await?; // Automatic conversion to 404 if not found
    
    Ok(json!({ "id": user.id, "username": user.username }))
}

// 2. Fetching aggregates
#[get("/users")]
async fn count_users(db: Arc<Database>) -> Result<serde_json::Value> {
    let total = User::query()
        .filter(User::is_active.eq(true))
        .count(&db)
        .await?; // Errors map to 500
        
    Ok(json!({ "active_users": total }))
}
```

---

## 4. Custom Exception Handling

Because the original `OrmError` is preserved as the source of Tork's HTTP `Error`, you can write custom exception handlers in the framework to intercept, log, or customize database error responses:

```rust
use tork::App;
use tork_orm::prelude::OrmError;

let app = App::new()
    .exception_handler(|error: &OrmError| {
        // Custom logging or error formatting
        println!("Database error intercepted: {:?}", error);
    });
```
