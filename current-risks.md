# Current Risks & Untested Behaviors

This document lists the components, edge cases, and features of Tork ORM that are currently untested or carry potential risks, ordered from **most critical (highest risk)** to **lowest risk**.

---

## 1. Preloader Variable Limit Crash (Too Many SQL Variables - High Risk)
- **Risk:** When preloading a child relation, the preloader constructs an `IN` clause binding all distinct parent keys as parameters ([`preload.rs:L167`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/preload.rs#L167)).
- **Bug:** Databases enforce limits on the maximum number of query variables (e.g., SQLite restricts this to 999 by default; MSSQL to 2100). If you preload relations for a large number of parent records (e.g., > 1000), the query will crash with a `too many SQL variables` database error.
- **Status:** RESOLVED. The preloader chunks distinct parent keys to `Dialect::max_bind_params` (SQLite 999, PostgreSQL/MySQL 65535) and runs one batch query per chunk, merging the rows. Covered by `preload_chunks_keys_past_the_variable_limit`.

## 2. Critical Data-Loss Bug in `down_to` Migration Rollback (High Risk)
- **Risk:** The migrator's `FileMigrator::down_to` method calculates the number of steps to roll back as `chain.len().saturating_sub(position + 1)`, where `position` is the target migration's index in the local migration chain ([`files.rs:L189-199`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/migration/files.rs#L189-L199)).
- **Bug:** This count represents the total number of files *conceptually* following the target in the local chain, regardless of whether they have been applied. If there are new, unapplied migrations at the end of the local chain, this step count will be larger than the actual number of applied migrations after the target. When calling `self.down(after)`, the runner will roll back the latest `after` applied migrations, meaning it will roll back the target itself and even earlier applied migrations that the user intended to preserve.
- **Consequence:** Under common developer scenarios (e.g. pulling a new branch containing new unapplied migrations, then attempting to roll back a local applied migration to resolve conflicts), developers will experience silent, unexpected data loss in earlier migrations.
- **Status:** RESOLVED. `down_to` now counts only the *applied* migrations strictly after the target (from `_tork_migrations`), so unapplied files later in the chain no longer cause the target or earlier migrations to be reverted. Covered by `down_to_reverts_only_applied_migrations_after_target`.

## 3. Serverless Stateless Ephemerality & SQLite Data Loss (High Risk / Deployment Hazard)
- **Risk:** The ORM currently compiles and defaults specifically to SQLite connection pools.
- **Vulnerability:** In serverless stateless environments (such as AWS Fargate, GCP Cloud Run, or GCP App Engine), container filesystems are completely ephemeral and scale to zero. Any write operation to a local SQLite database file is lost upon container shutdown or rotation. Moreover, if multiple container instances scale out to handle high traffic concurrently, each instance writes to its own isolated database file, leading to divergent "split-brain" states.
- **Status:** Untested and unsupported without a centralized database backend or a network mount.

## 4. Lack of Distributed Migration Lock / Race Conditions (High Risk / Concurrency Hazard)
- **Risk:** The migration runner executes DDL statements and registers status inside transactions but does not apply any distributed lock or database-level lock during migration execution.
- **Vulnerability:** In serverless environments, deploying a new version often triggers multiple container instances to spin up concurrently. If migrations run automatically on startup, multiple containers will run `migrator.up()` at the exact same moment. This creates lock contentions on `_tork_migrations` or duplicate DDL commands, causing container crashes and deployment boot loops.
- **Status:** RESOLVED. `FileMigrator` takes a session advisory lock around up/down (PostgreSQL `pg_advisory_lock`, MySQL `GET_LOCK`, keyed by the bookkeeping table; released when the connection ends). Concurrent migrators serialize instead of racing. SQLite relies on its file-level write lock + busy timeout (dialect returns no lock SQL). Covered by `uses_session_advisory_locks_for_migrations` / `uses_named_user_locks_for_migrations`.

## 5. Linker Stripping & Dead-Code Elimination (High Operational Risk)
- **Risk:** The ORM relies on the `inventory` crate for link-time model schema registration so that `migrate generate` can discover models automatically ([`lib.rs:L93-102`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm/src/lib.rs#L93-L102)).
- **Vulnerability:** If models are defined in a separate library crate in a multi-crate workspace, and the main binary does not directly reference symbols inside the model library, the Rust compiler/linker's dead-code elimination will silently strip the static registration symbols. As a result, `migrate generate` will fail to detect any of the models defined in the library, and no database tables will be generated for them.
- **Status:** Untested and unhandled.

## 6. Hardcoded SQLite `RETURNING` Support (High SQLite Version Hazard)
- **Risk:** `SqliteDialect::supports_returning` is hardcoded to return `true` ([`sqlite.rs:L58-60`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/dialect/sqlite.rs#L58-L60)).
- **Vulnerability:** SQLite added support for the `RETURNING` clause in version 3.35.0 (released in 2021). If this ORM is compiled/deployed in environments with older system SQLite libraries (e.g. CentOS 8, RHEL 8, Debian 10, or older container images running musl/glibc), all insert operations will fail immediately with syntax errors.
- **Status:** RESOLVED. `SqliteDialect::supports_returning` now returns `rusqlite::version_number() >= 3_035_000`, so a build linking an older system SQLite falls back to a re-select instead of failing every insert. Covered by `returning_support_follows_the_runtime_sqlite_version`.

## 7. Unimplemented Transaction Options & Standard Isolation Levels (High Risk / Feature Deficit)
- **Risk:** While standard SQL isolation levels and option builders are commonly needed for multi-dialect production databases, they are **entirely missing** from the codebase:
  - **No `TransactionOptions` Struct:** The builder-style configuration struct (`TransactionOptions::new().isolation(...).read_only(...)`) does not exist.
  - **No Standard SQL Isolation Levels:** The proposed `IsolationLevel` variants (`ReadUncommitted`, `ReadCommitted`, `RepeatableRead`, `Serializable`) are not defined. The codebase only implements SQLite-specific locking states (`Deferred`, `Immediate`, `Exclusive`).
- **Status:** RESOLVED. `IsolationLevel` now includes the standard SQL levels (`ReadUncommitted`/`ReadCommitted`/`RepeatableRead`/`Serializable`) with `TransactionBuilder` methods (`.read_committed()`, `.serializable()`, ...). Each dialect maps them: PostgreSQL `BEGIN ISOLATION LEVEL ...`, MySQL a `SET TRANSACTION ISOLATION LEVEL ...` preamble (`Dialect::isolation_setup_sql`), SQLite a plain `BEGIN` (it is serializable via its locking). Covered by `standard_isolation_levels_render_directly` / `standard_isolation_levels_use_a_set_statement` / `serializable_isolation_runs_on_sqlite`.

## 8. Lack of Retryable Transaction API (High Risk / Feature Deficit)
- **Risk:** Under serializable isolation levels or heavy write concurrency, database queries frequently fail with deadlock or serialization conflicts. Without a retry mechanism, the application will crash or return HTTP 500 errors to clients.
- **Vulnerability:** The proposed `transaction_retry` and `TransactionRetry` APIs are **entirely unimplemented**. Developers must write custom loop logic manually to handle and retry serialization failures.
- **Status:** RESOLVED. `Database::transaction_retry(max_attempts, f)` reruns the closure in a fresh transaction when it fails with a transient conflict, detected by `OrmError::is_retryable()` (lock timeout, deadlock, or serialization failure, across backends). Covered by `transaction_retry_recovers_from_a_transient_conflict` and `transaction_retry_gives_up_on_a_non_retryable_error`.

## 9. Future Cancellation Connection Leak & Pool Starvation (High Risk / Resource Exhaustion)
- **Risk:** When a database query is cancelled due to a client timeout or future cancellation (e.g., via `tokio::time::timeout`), the underlying `tokio::task::spawn_blocking` worker continues executing to completion. However, because the cancelled query future was dropped, the `PinnedSqlite` handle is dropped.
- **Bug:** During query execution, the connection is taken out of `PinnedSqlite` (`self.conn` is `None`). When `PinnedSqlite` drops prematurely during cancellation, its `Drop` implementation sees `None` and does not return the connection to the pool. When the background thread finally completes, the connection is discarded and closed.
- **Consequence:** Under frequent request timeouts or cancellations, connection handles are permanently lost from the pool, causing massive performance thrashing as new connections are opened and configured continually, leading to thread and descriptor exhaustion.
- **Status:** RESOLVED. The blocking worker now returns the connection to the pool itself and reports the result over a channel, so a cancelled query future no longer drops the connection. The pinned-connection (transaction) path restores its connection through a shared slot for the same reason. Covered by `cancelled_query_returns_its_connection_to_the_pool`.

## 10. Bulk Create Variable Limit Crash (Too Many SQL Variables - High Risk)
- **Risk:** The bulk creation implementation [`model.rs:L212-241`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/model.rs#L212-L241) generates a single multi-row `INSERT` statement containing all fields for all rows in a single batch.
- **Bug:** The number of variables bound in this query is `values.len() * columns.len()`. If this number exceeds the database-enforced parameter limit (e.g. 999 parameters in SQLite), the query will immediately crash with a database error.
- **Consequence:** Inserting large datasets (e.g. 500 records with 3 columns each) via `bulk_create` will crash the application in production unless developers manually chunk the inputs.
- **Status:** RESOLVED. `bulk_create` splits rows into chunks of `max_bind_params / columns` (per-dialect: SQLite 999, PostgreSQL/MySQL 65535) and runs one INSERT per chunk. Covered by `bulk_create_chunks_past_the_variable_limit`.

## 11. Migration Branching Conflicts in Git (High Operational Risk)
- **Risk:** The migration engine requires a strictly linear chain ([`files.rs:L491-495`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/migration/files.rs#L491-L495)).
- **Vulnerability:** If two developers generate migrations in parallel on different Git branches, both migrations will use the same `down_revision` parent. Upon merging to `main`, the migration chain branches. The migrator will immediately block execution and fail with the error `branching not supported yet: two migrations follow {parent}`.
- **Status:** Untested for automated recovery. Developers must manually edit SQL file headers to re-linearize the chain before they can deploy.

## 12. Permissive Checksum Mismatch Policy in Production (Medium Risk)
- **Risk:** If a migration file is modified after it has already been applied, the migrator compares its hash against the database registry ([`files.rs:L133-136`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/migration/files.rs#L133-L136)).
- **Vulnerability:** The default action for a mismatch is `OnMismatch::Warn`, which prints a warning to stderr and proceeds. In production, this can lead to silent schema drifts if developers accidentally modify applied migrations, eventually causing runtime queries to fail.
- **Status:** RESOLVED. The default is now `OnMismatch::Error` (both `FileMigrator` and `Migrator`): an edited applied migration aborts the run. The CLI exposes `--allow-checksum-mismatch` to downgrade to a warning for local development. Covered by `editing_an_applied_migration_aborts_up_by_default` and `changed_checksum_errors_by_default_and_warn_overrides`.

## 13. SQL Injection Bypass via Dialect Escaping (High Security Risk)
- **Risk:** In [`writer.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/dialect/writer.rs#L419-L428), `quote_string_literal` escapes strings by only doubling single quotes (`'`).
- **Vulnerability:** 
  - On **MySQL**, backslash (`\`) acts as an escape character inside single-quoted strings by default. A string containing a backslash followed by a single quote can bypass the doubled-quote escape mechanism, leading to **SQL injection** when rendering inline values (such as in partial index predicates or DDL statements).
  - On **PostgreSQL**, if the connection is configured with `standard_conforming_strings = off`, backslash escapes are enabled, causing a similar SQL injection vulnerability.
- **Status:** RESOLVED. Inline string escaping is now dialect-aware via `Dialect::escape_string_literal`: MySQL also doubles backslashes (`quote_string_literal_mysql`), closing the backslash-before-quote bypass. PostgreSQL/SQLite keep quote-doubling (Tork assumes the default `standard_conforming_strings = on`). Covered by `mysql_escapes_backslashes_in_string_literals` / `sqlite_does_not_escape_backslashes`.

## 14. SQL Injection via Unescaped Scalar Function Names (High Security Risk)
- **Risk:** The SQL expression writer [`writer.rs:L133-143`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/dialect/writer.rs#L133-L143) handles function calls (`Expr::Func`) by rendering the function name verbatim using `self.push_sql(name)`.
- **Vulnerability:** There is no escaping or quoting applied to the function name itself. If the application dynamically constructs queries using runtime-supplied string values as function names (e.g., dynamic database-side transformations guided by user input), an attacker can supply malicious SQL payloads that bypass filter constructs.
- **Status:** RESOLVED. `Expr::Func` names are written through `push_function_name`, which keeps only identifier-safe characters (`[A-Za-z0-9_.]`), so a name built from untrusted input is neutralized into a harmless unknown-function token instead of injecting SQL. Covered by `function_names_cannot_inject_sql`.

## 15. Path Traversal & Arbitrary File Creation in Connection Strings (High Security Risk)
- **Risk:** The SQLite connection path parsing in [`sqlite.rs:L223-239`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/driver/sqlite.rs#L223-L239) does not sanitize or restrict the parsed database path.
- **Vulnerability:** If an application allows dynamic database URLs (e.g., in multi-tenant environments where the database name is derived from user input or request headers), a user can supply path traversal components (like `sqlite://../../../../etc/passwd` or `sqlite://var/lib/malicious.db`). SQLite will attempt to create or write to that file location, allowing arbitrary file creation or file corruption.
- **Status:** RESOLVED. SQLite connection-path parsing rejects `..` (parent-directory) components, so a path derived from untrusted input cannot escape to an arbitrary file; absolute paths chosen by the application are still allowed. Covered by `rejects_parent_directory_traversal_in_the_path`.

## 16. Concurrent Queries on Transaction Handles (Critical Concurrency Hazard)
- **Risk:** The transaction wrapper [`PinnedSqlite`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/driver/sqlite.rs#L336-L340) locks and takes (`Option::take`) the inner connection out of the mutex for the duration of a `spawn_blocking` query, returning it only when the query finishes.
- **Bug:** If a user tries to execute two queries concurrently on the same transaction object (for instance, using `tokio::join!` or spawning multiple futures using the same `tx` reference):
  ```rust
  let (res1, res2) = tokio::join!(
      tx.execute("SELECT ...".into(), vec![]),
      tx.execute("SELECT ...".into(), vec![]),
  );
  ```
  the second query will immediately fail with the error `pinned connection is already in use` instead of waiting for the connection to be returned.
- **Status:** RESOLVED. `PinnedSqlite` now holds an async gate (`tokio::sync::Mutex`) that each operation locks for its duration, so concurrent statements on the same transaction (for example via `tokio::join!`) serialize and queue instead of failing with `pinned connection is already in use`. Covered by `concurrent_queries_on_a_transaction_serialize`.

## 17. Preloading Type Lookup Collision (Critical Bug)
- **Risk:** The preloader uses a `HashMap<TypeId, Box<dyn Any>>` to attach preloaded relation slices to parent models (implemented in [`preload.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/preload.rs#L71-L85)).
- **Bug:** If a model has **multiple relations referencing the same child model type** (e.g., a `Post` model with an `author` relation pointing to `User` and a `reviewer` relation also pointing to `User`), preloading both:
  ```rust
  Post::query().preload(Post::author()).preload(Post::reviewer())
  ```
  will cause a key collision. The second preload step silently overwrites the first in the `relations` map. When calling `.get::<User>()`, the parent will only return the second relation's slice.
- **Status:** RESOLVED. The `relations` map is now keyed by relation identity (type plus join columns), so each relation keeps its own slot. `get_via(&relation)` reads a specific relation's rows; `get::<C>()` still works for the single-relation case. Covered by `two_relations_to_the_same_type_keep_separate_slots`.

## 18. Infinite Hang on Pool Exhaustion (High Risk)
- **Risk:** When all connections in the pool are checked out, calling `pool.acquire_pinned()` or `pool.with_connection()` waits on the semaphore permit indefinitely ([`sqlite.rs:L186`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/driver/sqlite.rs#L186)).
- **Vulnerability:** If there is a connection leak in the application or if execution gets blocked, checking out a connection will hang the entire thread/request indefinitely. Gaining access to a timeout-bounded checkout is currently impossible.
- **Status:** RESOLVED. Connection checkout is bounded by a timeout (default 30s, configurable via `SqlitePool::with_acquire_timeout`); exceeding it returns a clear timeout error instead of hanging. Covered by `checkout_times_out_instead_of_hanging_forever`.

## 19. Persistent Broken Connection Poisoning (High Risk)
- **Risk:** If a connection in the pool hits a terminal database error (e.g. connection timeout, corrupted file, or disk-full error), the driver still returns the connection handle back into the idle pool [`sqlite.rs:L209-211`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/driver/sqlite.rs#L209-L211).
- **Bug:** There is no health check, validation query, or connection recycling logic. Once a connection goes bad, it will continuously be checked out from the pool and fail every subsequent query that is routed to it.
- **Status:** RESOLVED. After a query error, the pooled connection is health-probed (`SELECT 1`) before being returned: a healthy connection (ordinary query error) is reused, but one poisoned by a terminal error fails the probe and is discarded instead of returned, so a broken connection no longer fails every later query routed to it. Covered by `a_query_error_keeps_a_healthy_connection`.

## 20. Dialect Extensibility & Open-Closed Violations (High Risk / Architectural Flaw)
- **Risk:** Although a `Dialect` trait exists, DDL and query generation are largely hardcoded in the core library rather than delegating to the dialect. Adding new databases (like PostgreSQL, MySQL, MSSQL, or Oracle) is heavily restricted:
  - **Hardcoded Column Types:** Type formatting (e.g., mapping `Blob` to `BLOB` vs `BYTEA`, or `Timestamp` to `TIMESTAMP WITH TIME ZONE` vs `DATETIME`) is hardcoded in the core function `render_type` ([`render.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/migration/render.rs#L292-L309)). Dialects have no way to override type syntax.
  - **Hardcoded Auto-Increment/Identity Column Syntax:** Identity column generation is branched on a hardcoded match statement `match dialect.kind()` ([`render.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/migration/render.rs#L257-L266)) inside the core engine instead of querying the dialect.
  - **Hardcoded Query Pagination:** In [`writer.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/dialect/writer.rs#L250-L260), pagination rendering is hardcoded to `LIMIT N OFFSET M`. Dialects cannot override pagination syntax, which will break on databases like Oracle or MSSQL.
  - **SQLite-Centric Isolation Levels:** The `IsolationLevel` enum ([`transaction.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/transaction.rs#L60-L78)) hardcodes SQLite-specific locking modes (`Deferred`, `Immediate`, `Exclusive`) instead of standard SQL isolation levels (e.g., `READ COMMITTED`, `SERIALIZABLE`), forcing other dialects to use stubs or map them incorrectly.
- **Status:** Adding a new dialect requires modifying the core engine code in multiple places, violating the Open-Closed principle.

## 21. Non-Integer Primary Keys on Non-`RETURNING` Dialects (High Risk / Bug)
- **Risk:** If a dialect does not support `RETURNING` statements, `Model::create` executes the insert and then re-selects the row using SQLite's `last_insert_rowid` coerced into a `Value::Int` (implemented in [`model.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/model.rs#L183-L205)).
- **Bug:** If the table uses a non-integer primary key (like a UUID `TEXT` or a `VARCHAR` string) on a database without `RETURNING`, `last_insert_rowid` will not match the primary key value. The reload step will fetch the wrong row or fail with a `not found` error.
- **Status:** RESOLVED. On a non-`RETURNING` dialect, `Model::create` reloads by the supplied primary-key value when it is non-integer (a UUID or string), instead of `last_insert_rowid` which only matches an integer key. Covered by `create_with_a_string_primary_key_reloads_by_value` (MySQL live test).

## 25. Column & Table Renaming Data Loss (High Risk)
- **Risk:** The schema generator does not track renames. If a model name or column name is renamed, the generator interprets this as a `DROP` of the old entity and a `CREATE`/`ADD` of the new one. Applying the generated migration will silently destroy all data in that column or table.
- **Status:** Untested for safety. No warning comments or safety checks are generated to prevent data-destroying drop operations caused by renaming.

## 26. Preloader Join Key Auto-Column Exclusion Bug (Medium Risk / Functional Failure)
- **Risk:** The preloader extracts parent model values for relationship matching via `column_value(parent, from_column)` ([`preload.rs:L313-325`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/preload.rs#L313-L325)). If the join key is not the primary key, it looks up the value in `parent.insert_values()`.
- **Bug:** `Model::insert_values` excludes all fields marked with `auto` (auto-increment columns or fields with DB-assigned defaults) so the database can assign them on creation. If a relation links parent/child records using a non-primary key auto-increment column or auto-timestamp column (e.g. `user_number`), `column_value` will return `None`.
- **Consequence:** Preloading a relation on any database-assigned columns will silently fail to link parents and children, returning empty child lists in memory.
- **Status:** Untested and broken.

## 27. Network File System (EFS/Cloud Filestore) SQLite WAL Lock Corruption (Medium Risk)
- **Risk:** Deploying SQLite on serverless containers using network-attached mounts (such as AWS EFS or GCP Cloud Filestore) to persist the database file.
- **Vulnerability:** SQLite WAL (Write-Ahead Logging) mode requires shared memory mapping (`.shm` file) to coordinate transactional locks between processes. Network file systems do not support POSIX mmap locks. Running WAL mode over EFS will fail to open or cause random database lock corruptions.
- **Status:** Untested on network shares.

## 28. Distributed Connection Pool Saturation on Scaling (Medium Risk / Resource Hazard)
- **Risk:** The database connection pool (`SqlitePool`) is instantiated locally per container.
- **Vulnerability:** In serverless environments, containers spin up and shut down on demand (auto-scaling). If the ORM is extended to use a centralized database (such as PostgreSQL), a sudden surge in traffic can spin up 100 containers. If each container opens `max_connections = 10`, the database will receive 1,000 connections simultaneously, easily saturating the backend connection limits and failing incoming queries.
- **Status:** No centralized connection proxy or serverless database adapter integration.

## 29. Timezone-Mismatch Key Failures in Preloading (Medium Risk)
- **Risk:** The preloader converts join keys to string representation using debug rendering [`preload.rs:L329-339`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/preload.rs#L329-L339) (e.g. `ts:OffsetDateTime`).
- **Bug:** If parent and child records are inserted with different timezone offsets representing the exact same moment (e.g. UTC `Z` vs `+00:00`), their string representation keys will differ (e.g., `ts:2026-06-11T20:00:00Z` vs `ts:2026-06-11T20:00:00+00:00`). The preloader will fail to stitch them together, returning empty child vectors.
- **Status:** Untested and vulnerable to timezone offset variations.

## 30. Database Schema Information Disclosure (Medium Security Risk)
- **Risk:** Database query and statement compilation errors include the full raw SQL text in the returned error string ([`sqlite.rs:L282`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/driver/sqlite.rs#L282)).
- **Vulnerability:** When integrated with the Tork web framework, these database errors are converted to HTTP `500` errors. If the framework propagates these error messages to client HTTP responses, it will leak internal database structures, table names, column names, and query patterns to public users.
- **Status:** Untested for sensitive schema filtering in production mode.

## 31. Fragile DateTime/Timestamp String Parsing (Medium Risk)
- **Risk:** The core parsing logic for date-times ([`value.rs:L210-219`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/value.rs#L210-L219)) strictly expects date strings to follow RFC 3339 formatting.
- **Bug:** Standard database date formats (such as SQLite's default `YYYY-MM-DD HH:MM:SS` format or MySQL's timestamp syntax) will fail to parse and immediately crash with a runtime conversion error.
- **Status:** Untested against non-RFC-3339 database text values.

## 32. Missing Primitive Integer Mappings (Medium Risk)
- **Risk:** The `BindValue` and `FromValue` traits ([`value.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/value.rs)) are only implemented for `bool`, `i32`, `i64`, `f64`, `String`, `Vec<u8>`, and `OffsetDateTime`.
- **Limitation:** Developers cannot define models utilizing common primitive integer types such as `u64`, `u32`, `usize`, `i16`, `u16`, `i8`, or `u8`. Structs with these types will fail to compile.
- **Status:** Unsupported and untested.

## 33. Prepared Statement Cache Leaks & Bloat (Medium Risk)
- **Risk:** The SQLite driver uses `prepare_cached` ([`sqlite.rs:L281`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/driver/sqlite.rs#L281)) to execute queries.
- **Bug:** If a developer constructs queries dynamically via string interpolation (e.g. `db.execute(format!("INSERT INTO logs VALUES ('{}')", msg), vec![])`) rather than using parameters, every unique SQL string creates a new entry in SQLite's per-connection prepared statement cache, causing infinite memory bloat.
- **Status:** No cache limits, evictions, or warnings are present.

## 34. Lack of Compound Primary Key Support (Medium Risk / Limitation)
- **Risk:** The `Model` trait defines primary keys as a single string literal `const PRIMARY_KEY: &'static str` (declared in [`model.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/model.rs#L72)).
- **Limitation:** Tables requiring compound primary keys (e.g., a many-to-many join table utilizing `(user_id, role_id)`) are unsupported. The `Model` derive macro enforces exactly one primary key field and will fail to compile.
- **Status:** Untested for multi-column identifiers.

## 35. Hardcoded SQLite Schema Introspection (Medium Risk)
- **Risk:** The schema introspection functions (e.g., `existing_columns`, `existing_indexes`, `table_exists` in [`introspect.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/migration/introspect.rs)) are written using raw, hardcoded SQLite pragmas (`pragma_table_info`, `pragma_index_list`, `sqlite_master`).
- **Limitation:** Even if connection drivers for PostgreSQL or MySQL were added, introspection would crash because it does not delegate to dialect-specific SQL structures (like `information_schema` tables).
- **Status:** Introspection is tightly coupled to SQLite and untested on other SQL database engines.

## 36. Index Name Length Truncation Collisions (Medium Risk)
- **Risk:** Auto-generated index names (`<table>_<column>_idx`) can easily exceed identifier limits in PostgreSQL (63 chars) or MySQL (64 chars) for tables with long names or composite fields. Databases silently truncate long index names on creation.
- **Bug:** The introspector will read the truncated name from the database, but the ORM registry will expect the full auto-generated name, resulting in a perpetual schema diff mismatch during `migrate generate`.
- **Status:** No length verification or truncation safeguards exist in macro index generation.

## 37. SQLite `SQLITE_BUSY` and Lock Contention (Medium Risk)
- **Risk:** SQLite uses database-level locks. Under high write concurrency, a transaction started with `BEGIN DEFERRED` (the default) may try to upgrade its lock to a write lock and fail with a `database is locked` (`SQLITE_BUSY`) error if another transaction is writing.
- **Status:** Untested under heavy concurrent write loads. Although `BEGIN IMMEDIATE` is supported to prevent upgrades, there is no automatic retry logic or recovery handler in the connection pool for busy states.

## 38. Silent SQLite WAL Mode Write Contention (Medium Risk)
- **Risk:** SQLite's WAL (Write-Ahead Logging) mode allows multiple concurrent read connections but only one write connection.
- **Vulnerability:** When configuring the database pool with multiple connections (`max_connections > 1`), concurrent write queries scheduled on different pool connections will experience contention. This results in threads blocking on the 5-second `BUSY_TIMEOUT_MS` and throwing database locks or write errors under sustained write-heavy workloads.
- **Status:** Untested under high write concurrency.

## 39. Lack of Deep/Nested Relation Preloading (Medium Risk / Limitation)
- **Risk:** The `Preloader<M>` and `Relation` APIs only support loading direct child relationships (1-level deep). Nested preloading (e.g., loading a user's posts, and for each post, its comments) is completely unsupported by the builder API.
- **Status:** Untested and unimplemented.

## 40. Lack of JSON or Structured Document Column Support (Medium Risk / DX Gap)
- **Risk:** There is no native column type representation or binder for JSON structures (e.g., SQLite JSON text or PG `JSONB`).
- **DX Gap:** Developers cannot save Rust structs directly into database columns using automated Serde serialization; they must manually serialize/deserialize them to strings on every database operation.
- **Status:** Untested and unsupported.

## 41. Missing `uuid::Uuid` Field Binding (Medium Risk / DX Gap)
- **Risk:** Although `uuid` is a workspace dependency (used to generate migration filenames), the ORM does not implement `BindValue` or `FromValue` for the `uuid::Uuid` type.
- **Vulnerability:** Structs containing `uuid::Uuid` fields fail to compile under the `Model` derive. Developers are forced to store UUIDs as `String` / `TEXT` columns and manually format them at runtime.
- **Status:** Unsupported and untested.

## 42. Unbounded Preload Collection Growth (Medium Risk / Memory Hazard)
- **Risk:** The `Relation` preloading API does not expose a `.limit(N)` builder method for preloading child datasets (defined in [`relation.rs`](file:///Users/muzak/Desktop/tork/orm/crates/tork-orm-core/src/relation.rs)).
- **Limitation:** When preloading a relation, the ORM always loads all associated records from the child table into memory. If a parent record has thousands of children (e.g., a popular user with 100,000 posts), calling `.preload()` will cause memory exhaustion and block execution.
- **Status:** Untested for memory safety bounds.

## 43. DDL Implicit Commits on Non-SQLite Backends (Medium Risk)
- **Risk:** Migrations execute within a transaction by default. However, databases like MySQL trigger an **implicit commit** for DDL statements (like `CREATE TABLE`).
- **Bug:** If a multi-statement MySQL migration fails midway, statements run prior to the failure cannot be rolled back, breaking transaction atomicity.
- **Status:** Untested on MySQL or other implicit-commit database engines.

## 44. Orphaned Tables (Model Deletions) (Medium Risk)
- **Risk:** The schema generator (`generate`) only diffs models present in the Rust codebase against the database. If a model is deleted from the codebase, the generator does not detect that a table exists in the database but lacks a matching model, meaning no `DROP TABLE` is ever generated.
- **Status:** Untested. Removing models leaves orphaned tables in the database indefinitely.

## 45. Silent Errors during Auto-Rollback on Drop (Medium Risk)
- **Risk:** `impl Drop for Transaction` triggers a synchronous rollback (`self.inner.rollback_now()`) when the transaction handle drops without being committed. If the underlying SQLite connection is corrupted or busy, the rollback may fail silently.
- **Status:** The drop implementation cannot return an error or yield asynchronously. If a rollback fails during drop, there is no logging or exception propagated, potentially leaving the connection in an undefined state.

## 46. Runtime Value Conversion Failures (Medium Risk)
- **Risk:** Because SQLite uses dynamic typing, a column defined as `INTEGER` in Rust can hold a `TEXT` value in the database. When fetching this row, mapping the value back to `i64` will fail with a runtime conversion error.
- **Status:** The mapping of mismatched database types is caught at runtime (returning `ErrorKind::Conversion`), not compile-time. There are no tests verifying behavior when SQLite values deviate from the strict model definitions.

## 47. Lack of Model Validation Hook/Lifecycle Integration (Medium Risk)
- **Risk:** While the workspace includes the `garde` validation library, the ORM does not define any lifecycle hooks (such as `before_save`, `before_create`, or `before_update`) on the `Model` trait to trigger validations.
- **Vulnerability:** Developers must manually remember to call validation checks before calling database methods. If forgotten, invalid or corrupted input data can be persisted directly to the database without verification.
- **Status:** Unimplemented.

## 48. Silent In-Memory Database Schema & Data Loss (Medium Risk)
- **Risk:** In SQLite, an in-memory database (`:memory:`) is entirely tied to the lifecycle of the active connection handles.
- **Vulnerability:** Although the pool clamps connection size to 1 for in-memory targets, if the connection pool is closed (`pool.close()`) or if the single connection fails and is pruned/recreated by the driver, the entire database schema and data are silently lost.
- **Status:** Untested database lifecycle boundary.

## 49. Lack of Transaction Propagation / Nested Transaction API (Medium Risk / Feature Deficit)
- **Risk:** The transaction API only exposes top-level transactions and savepoints. It lacks standardized transaction propagation strategies (such as `PROPAGATION_REQUIRED` or `PROPAGATION_REQUIRES_NEW`).
- **DX Gap:** Developers cannot write modular transactional services where internal helpers dynamically join an active transaction or suspend it to open a new nested context, requiring manual handle passing and custom savepoint coordination.
- **Status:** RESOLVED (by design). Nested transactional units are provided by `Transaction::savepoint(f)`, which runs `f` inside a `SAVEPOINT` released on `Ok` and rolled back on `Err` without aborting the outer transaction (a `REQUIRES_NEW`-style nested unit). The `REQUIRED` pattern (join the active transaction) is the default: pass the same `&Transaction` (or `impl Executor`) down. Tork's explicit handle-passing replaces implicit propagation strategies. Covered by the savepoint transaction tests.

## 50. Untracked Column Options & Foreign Keys (Low-Medium Risk)
- **Risk:** The generator only checks for type and nullability mismatches on existing columns. Changes to foreign key definitions (e.g. changing `ON DELETE SET NULL` to `ON DELETE CASCADE`) are completely ignored by the generator.
- **Status:** Modifying column options (like foreign keys) does not produce any schema diffs or alerts.

## 51. Irreversible Index Drops (Low-Medium Risk)
- **Risk:** Deleting an index from a model emits a `DROP INDEX` statement. However, the `down` migration has no way of restoring the dropped index because its original definition is no longer in the codebase.
- **Status:** The generator emits `-- cannot recreate dropped index "name" (its definition is unknown)` in the `down` migration, leaving the rollback step broken/unusable.

## 52. Raw DDL Default Value Injection (Low-Medium Risk)
- **Risk:** The `DefaultValue::Raw(sql)` variant allows dynamically appending raw SQL strings directly into DDL columns during table creation.
- **Vulnerability:** If an application generates schema definitions dynamically using untrusted user inputs to set default column values, this leads to DDL SQL injection.
- **Status:** Untested for input sanitization.

## 53. Invalid `VARCHAR(0)` Spec Generation (Low-Medium Risk)
- **Risk:** The `Model` derive macro parses `varchar(length = N)` but does not validate if `N > 0`.
- **Bug:** Declaring a field with `#[field(varchar(length = 0))]` compiles successfully but generates invalid DDL specifications (`VARCHAR(0)`), causing database schema creation errors at runtime.
- **Status:** Untested.

---

## 54. Production Readiness Requirements & Missing Tests

The following verification steps and test suites are **mandatory** to complete before deploying this ORM into production:

### A. Connection Pool Telemetry, Logging, and Metrics
- **Requirement:** Implement logging for slow queries (e.g. queries taking > 500ms) and collect pool metrics (idle connection count, checked-out connection count, and acquire wait time).
- **Missing Test:** A telemetry/metrics test suite verifying that checkout durations and connection lifecycle states (open, close, acquire, release) are recorded and reported correctly to metrics hooks.

### B. Dry-Run Migration Verification
- **Requirement:** Migrations must support a dry-run flag (`--dry-run`) to print the generated SQL to stdout/logs without applying it. This allows teams to audit changes in CI/CD before deployment.
- **Missing Test:** Testing the CLI and migration runner with dry-run parameters to verify that the database schema table remains unchanged.

### C. SQLite WAL Disk Footprint Controls
- **Requirement:** SQLite in WAL (Write-Ahead Logging) mode can experience unbounded `.wal` file growth if read transactions are held open indefinitely. We must configure synchronous checkpoints (`PRAGMA wal_checkpoint(PASSIVE)`) or size controls.
- **Missing Test:** Heavy read/write integration test simulating long-running read transactions to ensure WAL files do not saturate the disk.

### D. Destructive Migration Guardrails
- **Requirement:** The migration runner/CLI must fail or require explicit confirmation (e.g., `--allow-destructive`) if a migration contains data-destroying changes such as `DROP TABLE` or `DROP COLUMN`.
- **Missing Test:** A CLI test confirming that migrations with structural drops fail by default when run in production mode.
