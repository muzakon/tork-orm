# 12. Database Backends and Dialects

Tork ORM speaks three SQL dialects from one model: SQLite, PostgreSQL, and MySQL. The same model and query code runs against any of them; the ORM renders backend-specific SQL behind a dialect abstraction.

## Connecting

The backend is chosen by the connection URL scheme. Each non-SQLite driver sits behind a Cargo feature, so you only compile the drivers you use.

```toml
[dependencies]
tork-orm = { version = "...", features = ["postgres", "mysql"] }
```

```rust
// SQLite (bundled, always available)
let db = Database::connect("sqlite://app.db", 5).await?;
let db = Database::connect(":memory:", 1).await?;

// PostgreSQL (feature = "postgres")
let db = Database::connect("postgres://user:pass@localhost:5432/app", 5).await?;

// MySQL (feature = "mysql")
let db = Database::connect("mysql://user:pass@localhost:3306/app", 5).await?;
```

The second argument is the connection-pool size.

### Connection checkout timeout

When every connection in the pool is busy, a new query waits for one to free up. To keep a connection leak or a stuck query from wedging the whole server, that wait is bounded: a checkout that cannot get a connection within **30 seconds** fails with a timeout error instead of hanging forever, so the request fails fast and the runtime stays responsive. A cancelled query (for example one wrapped in `tokio::time::timeout` that fires) returns its connection to the pool rather than leaking it, so the pool does not thrash under load.

A connection that fails a query is health-checked before going back to the pool: an ordinary error (a constraint violation, a missing table) leaves it healthy and it is reused, but a connection poisoned by a terminal error (disk full, corruption, an IO failure) fails the check and is discarded, so a broken connection is never handed to a later query — a fresh one is opened on demand.

## Feature Matrix

Most of the query API is identical on every backend. A few features are dialect-specific; using one where it is not supported produces a clear error at execution time rather than invalid SQL.

| Feature | SQLite | PostgreSQL | MySQL |
| --- | --- | --- | --- |
| Core CRUD, filters, joins, group/having, window functions, CTEs, UNION | Yes | Yes | Yes |
| `INSERT ... RETURNING` | Yes | Yes | No (re-selects by last id) |
| Upsert | `ON CONFLICT` | `ON CONFLICT` | `ON DUPLICATE KEY UPDATE` |
| `FULL OUTER JOIN` | 3.39+ | Yes | No (rejected) |
| Aggregate `FILTER` | Yes | Yes | Emulated with `CASE` |
| `DISTINCT ON` | No | Yes | No |
| Lock modifiers (`for_share`, `skip_locked`, `nowait`, `lock_of`) | No | Yes | Yes |
| JSON columns and operators | No | Yes (`jsonb`) | Yes |
| Arrays and `UUID` columns | No | Yes | No |
| Enums (`#[derive(DbEnum)]`) | Yes (`CHECK`) | Yes (`CHECK`) | Yes (native `ENUM`) |

Shared features render the right syntax per dialect. JSON operators, for example, render as `->` and `JSON_CONTAINS(...)` on MySQL but `->` and `@>` on PostgreSQL.

## Build-Time Dialect Gating

If your project targets a single backend, declare it in `Cargo.toml`:

```toml
[package.metadata.tork]
dialect = "postgres"   # or "mysql" or "sqlite"
```

With a dialect declared, using a column type the backend cannot support becomes a compile error rather than a runtime failure. For example, a `uuid::Uuid` or `Vec<String>` column on a `mysql`-declared project fails to compile, while a `serde_json::Value` column compiles because MySQL has native JSON. Enum columns compile on every dialect.
