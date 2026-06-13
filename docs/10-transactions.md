# 10. Database Transactions

Tork ORM provides robust, asynchronous transaction management with support for closure-based auto-transactions, explicit manual handles, customizable isolation levels, nested transactions via savepoints, and automatic rollback on drop.

---

## 1. The `Executor` Trait

To write reusable queries or repository methods that can run both on a direct connection pool and inside a transaction, accept `impl Executor` rather than a concrete `Database` handle.

The [`Executor`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/executor.rs#L22) trait is implemented by:
- `Database` (the connection pool)
- `Arc<Database>` (dependency injection handles)
- `Transaction` (explicit transaction handles)
- `&Transaction`

### Repository Pattern Example

```rust
use tork_orm::prelude::*;

pub struct UserRepository;

impl UserRepository {
    // This method can accept either &db or tx
    pub async fn find_by_id(&self, executor: impl Executor, id: i64) -> Result<Option<User>> {
        User::query()
            .filter(User::id.eq(id))
            .first(executor)
            .await
    }
}
```

---

## 2. Closure-Based Transactions (Recommended)

The most ergonomic way to run transactional work is using the `transaction!` macro. It automatically commits when the closure returns `Ok` and rolls back if it returns `Err`.

```rust
use tork_orm::prelude::*;
use tork_orm::transaction;

let result = transaction!(db, |tx| async move {
    // 1. Create a user
    let user = User::create(tx, &User {
        id: 0,
        username: "alice".into(),
        is_active: true,
    }).await?;

    // 2. Create an associated post
    Post::create(tx, &Post {
        id: 0,
        user_id: user.id,
        title: "Welcome!".into(),
    }).await?;

    Ok(user)
}).await?;
```

> [!NOTE]
> The `transaction!` macro automatically wraps the inner async block in `Box::pin` behind the scenes, making the syntax clean and readable.

---

## 3. Explicit / Manual Transactions

If you need fine-grained manual control over when a transaction commits or rolls back, you can use the explicit `begin` API:

```rust
use tork_orm::prelude::*;

// 1. Start the transaction
let mut tx = db.begin().await?;

// 2. Execute queries using `tx`
tx.execute("INSERT INTO logs (message) VALUES (?)".into(), vec![Value::Text("Manual entry".into())]).await?;

// 3. Commit or rollback explicitly
tx.commit().await?; // Or tx.rollback().await?;
```

### Automatic Rollback on Drop
If a transaction handle is dropped without calling `.commit()` or `.rollback()` (for example, if a function returns early due to an error `?`), the transaction is **automatically rolled back** on drop.

### Concurrent Statements on One Transaction
A transaction owns a single pinned connection, so its statements run one at a time. If you issue two statements on the same transaction concurrently (for example with `tokio::join!`), they are **serialized** — the second waits for the first to finish rather than failing. Prefer issuing transaction statements sequentially; the serialization is a safety net, not a way to parallelize work within a transaction.

---

## 4. Transaction Options & Isolation Levels

You can configure transaction locking behavior (isolation levels) using the `transaction_with()` builder. This allows configuring SQLite's transaction lock states:

| Option | Method | Description |
|---|---|---|
| `DEFERRED` | `.deferred()` | Default. Acquires no locks until the first read/write. |
| `IMMEDIATE` | `.immediate()` | Acquires a write lock immediately. Other writers are blocked, but concurrent readers can proceed. |
| `EXCLUSIVE` | `.exclusive()` | Acquires an exclusive lock immediately, preventing all concurrent reads and writes. |

The **standard SQL isolation levels** are also available and map per dialect
(PostgreSQL `BEGIN ISOLATION LEVEL ...`, MySQL `SET TRANSACTION ISOLATION LEVEL ...`;
SQLite is serializable through its locking, so they fall back to a plain `BEGIN`):

| Method | Level |
|---|---|
| `.read_uncommitted()` | `READ UNCOMMITTED` |
| `.read_committed()` | `READ COMMITTED` |
| `.repeatable_read()` | `REPEATABLE READ` |
| `.serializable()` | `SERIALIZABLE` |

### Retrying on conflicts

Under `SERIALIZABLE` or heavy write contention the database may abort a transaction
and expect the client to retry. `Database::transaction_retry(max_attempts, f)` reruns
the closure in a fresh transaction when it fails with a transient conflict (a lock
timeout, deadlock, or serialization failure, detected by `OrmError::is_retryable()`).
The closure may run more than once, so keep its in-memory effects idempotent.

```rust
db.transaction_retry(5, |tx| Box::pin(async move {
    tx.execute("UPDATE accounts SET balance = balance - 10 WHERE id = 1".into(), vec![]).await?;
    Ok(())
}))
.await?;
```

### Example

```rust
use tork_orm::prelude::*;

db.transaction_with()
    .immediate() // Acquire write locks immediately
    .run(|tx| Box::pin(async move {
        // Query execution...
        Ok(())
    }))
    .await?;
```

---

## 5. Nested Transactions (Savepoints)

Tork ORM supports nested transactions using savepoints. An error in an inner savepoint rolls back only the work done inside the savepoint, leaving the outer transaction unaffected.

```rust
use tork_orm::prelude::*;

db.transaction(|tx| Box::pin(async move {
    // 1. Create parent record (outer transaction)
    User::create(tx, &new_user).await?;

    // 2. Create posts (inner savepoint transaction)
    let inner_result = tx.savepoint(|sp| Box::pin(async move {
        Post::create(sp, &new_post).await?;
        // If this fails, only the post is rolled back
        Ok(())
    })).await;

    // The outer transaction can still proceed and commit
    Ok(())
})).await?;
```
