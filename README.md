# Tork ORM

An asynchronous, Tortoise-style ORM for Rust with first-class support for **SQLite,
PostgreSQL, and MySQL** from a single model. Queries are expressed through typed column
handles (mismatched comparisons are compile errors), relations preload without N+1, and
schema migrations are plain SQL files driven by a zero-compilation CLI.

Part of the [Tork](https://github.com/muzakon/tork) project; usable standalone or with
the [Tork web framework](https://github.com/muzakon/tork-framework).

## Features

- **Type-safe queries** — `User::query().filter(User::age.gt(18)).order_by(User::id.desc())`.
- **Multi-dialect** — one model runs on SQLite/PostgreSQL/MySQL; dialect-specific SQL and
  build-time gating of unsupported column types.
- **Relations & preloading** — `has_many`/`belongs_to`, N+1-free `.preload(...)`, self-joins.
- **Rich querying** — joins, group-by/having, window functions, CTEs, UNION, subqueries,
  `DISTINCT ON`, row locking (`FOR UPDATE`/`SKIP LOCKED`), keyset pagination, JSON & full-text.
- **Lifecycle columns** — auto `created_at`/`updated_at`, optimistic-lock `version`, and
  `deleted_at` soft-delete with an automatic query scope.
- **Database enums** — `#[derive(DbEnum)]`, native `ENUM` on MySQL and `CHECK`-constrained
  text elsewhere.
- **Migrations** — model-declared foreign-key actions and CHECK constraints; SQL-file
  migrations with up/down, generated from your models and run by the CLI.

## Quick example

```rust
use tork_orm::prelude::*;

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
    let db = Database::connect("sqlite://app.db", 5).await?;

    // Insert (returns the stored row with its generated id).
    let user = User::create(&db, &User { id: 0, username: "alice".into(), is_active: true }).await?;

    // Typed, filter-first queries that bind every value.
    let active = User::query()
        .filter(User::is_active.eq(true))
        .order_by(User::id.desc())
        .limit(20)
        .all(&db)
        .await?;

    println!("created {} of {}", user.username, active.len());
    Ok(())
}
```

Connection URLs: `sqlite://app.db` / `:memory:`, `postgres://user:pass@host:5432/db`
(feature `postgres`), `mysql://user:pass@host:3306/db` (feature `mysql`).

## Migrations (CLI)

The ORM ships the `tork-orm` binary (the `tork` CLI exposes the same commands as
`tork migrate ...`). Migrations are plain `.sql` files, so no project compilation is
needed to run them:

```sh
tork-orm migrate init                 # create the migrations directory
tork-orm migrate create add_users     # scaffold a new migration
tork-orm migrate up                   # apply all pending  (also: up <revision>)
tork-orm migrate status               # show applied / pending
tork-orm migrate down                 # revert one  (also: down <n> | base | <revision>)
```

The schema itself is generated from your models (app-embedded `migrate generate`); it
detects new tables, added/dropped columns, and indexes. See the
[migrations guide](docs/08-migrations-and-cli.md).

## Documentation

A full guide lives in [`docs/`](docs/README.md):

1. [Introduction](docs/01-introduction.md)
2. [Defining Models](docs/02-models.md)
3. [Indexing](docs/03-indexing.md)
4. [Querying and QuerySets](docs/04-queries.md)
5. [Writes: Insert, Update, Delete](docs/05-writes.md)
6. [Relations and Preloading](docs/06-relations-and-preloading.md)
7. [Aggregates and Projections](docs/07-aggregates-and-projections.md)
8. [Migrations and the CLI](docs/08-migrations-and-cli.md)
9. [Framework Integration](docs/09-framework-integration.md)
10. [Transactions](docs/10-transactions.md)
11. [Scalar Functions](docs/11-functions.md)
12. [Database Backends and Dialects](docs/12-database-backends.md)

Runnable examples: [`examples/orm_api`](examples/orm_api) (with the web framework) and
[`examples/ecommerce`](examples/ecommerce) (a ~28-table schema with production-readiness
tests: transactions, locking, constraints, security).

## Building

The default build links the framework bridge (the `tork` feature). Inside the superproject
the sibling `../framework` path resolves; to build the ORM on its own, disable it:

```sh
cargo build --no-default-features --features sqlite,migrations
```
