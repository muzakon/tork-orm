//! The SQLite driver.
//!
//! Connections are pooled and reused: each call checks an idle connection out of
//! the pool, runs the blocking SQLite work on the runtime's blocking thread pool
//! via [`tokio::task::spawn_blocking`], then returns the connection. A semaphore
//! bounds how many connections run concurrently, so the blocking pool is never
//! flooded. Reusing connections preserves SQLite's per-connection prepared
//! statement cache, and a connection survives a failed query (only a panic or a
//! failed open removes it), so the pool neither leaks nor thrashes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::types::{ToSqlOutput, Value as SqliteValue, ValueRef};
use rusqlite::{Connection, ToSql};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};

use crate::driver::ExecuteResult;
use crate::error::OrmError;
use crate::row::Row;
use crate::value::Value;

/// The busy timeout applied to every connection, in milliseconds.
///
/// With write-ahead logging a brief wait lets concurrent writers serialize
/// instead of failing immediately with `SQLITE_BUSY`.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// How long a connection checkout waits for a free pool slot before failing.
///
/// Without a bound, a connection leak or a stuck query would make every later
/// checkout hang forever; a timeout turns that into a recoverable error so the
/// request fails fast instead of wedging the whole server.
const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// How a connection should be opened.
#[derive(Debug, Clone)]
enum Source {
    /// A file-backed database at the given path.
    File(String),
    /// A private in-memory database.
    Memory,
}

/// Shared pool state behind an [`Arc`].
struct Inner {
    source: Source,
    idle: Mutex<Vec<Connection>>,
    semaphore: Arc<Semaphore>,
    statements: AtomicU64,
    acquire_timeout: Duration,
}

impl Inner {
    /// Waits for a pool slot, failing with a clear error if the wait exceeds the
    /// configured checkout timeout or the pool has been closed.
    async fn acquire_permit(&self) -> crate::Result<OwnedSemaphorePermit> {
        let acquire = Arc::clone(&self.semaphore).acquire_owned();
        match tokio::time::timeout(self.acquire_timeout, acquire).await {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => Err(OrmError::connection("connection pool is closed")),
            Err(_) => Err(OrmError::connection(format!(
                "timed out after {}s waiting for a database connection",
                self.acquire_timeout.as_secs()
            ))),
        }
    }
}

impl Inner {
    /// Opens and configures a fresh connection.
    fn open(&self) -> crate::Result<Connection> {
        let conn = match &self.source {
            Source::File(path) => Connection::open(path)
                .map_err(|e| OrmError::connection(format!("cannot open `{path}`")).with_source(e))?,
            Source::Memory => Connection::open_in_memory()
                .map_err(|e| OrmError::connection("cannot open in-memory database").with_source(e))?,
        };

        conn.busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))
            .map_err(|e| OrmError::connection("cannot set busy timeout").with_source(e))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| OrmError::connection("cannot enable foreign keys").with_source(e))?;
        if matches!(self.source, Source::File(_)) {
            // Write-ahead logging improves read/write concurrency for file databases.
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(|e| OrmError::connection("cannot enable WAL").with_source(e))?;
        }
        Ok(conn)
    }
}

/// A pool of reusable SQLite connections.
///
/// Cloning a pool is cheap: clones share the same underlying connections and
/// concurrency limit.
#[derive(Clone)]
pub struct SqlitePool {
    inner: Arc<Inner>,
}

impl SqlitePool {
    /// Builds a pool from a database URL and a maximum connection count.
    ///
    /// Accepted URL forms: `sqlite://path/to.db`, `sqlite:path/to.db`, a bare
    /// path, `:memory:`, or `sqlite::memory:`. In-memory databases are private to
    /// a single connection, so the pool is clamped to one connection for them.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is empty or `max_connections` is zero.
    pub fn new(url: &str, max_connections: u32) -> crate::Result<Self> {
        if max_connections == 0 {
            return Err(OrmError::configuration("max_connections must be at least 1"));
        }
        let source = parse_url(url)?;
        let permits = match source {
            Source::Memory => 1,
            Source::File(_) => max_connections as usize,
        };
        Ok(Self {
            inner: Arc::new(Inner {
                source,
                idle: Mutex::new(Vec::new()),
                semaphore: Arc::new(Semaphore::new(permits)),
                statements: AtomicU64::new(0),
                acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            }),
        })
    }

    /// Overrides how long a connection checkout waits for a free slot before
    /// failing with a timeout error (default 30 seconds).
    ///
    /// Must be called before the pool is cloned or shared, as it rebuilds the
    /// shared state. A zero duration keeps the default.
    pub fn with_acquire_timeout(mut self, timeout: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.acquire_timeout = if timeout.is_zero() {
                DEFAULT_ACQUIRE_TIMEOUT
            } else {
                timeout
            };
        }
        self
    }

    /// Runs a query that returns rows.
    pub async fn fetch_all(&self, sql: String, params: Vec<Value>) -> crate::Result<Vec<Row>> {
        self.with_connection(move |conn| fetch_all(conn, &sql, &params))
            .await
    }

    /// Runs a statement that returns no rows.
    pub async fn execute(&self, sql: String, params: Vec<Value>) -> crate::Result<ExecuteResult> {
        self.with_connection(move |conn| execute(conn, &sql, &params))
            .await
    }

    /// Runs a batch of semicolon-separated statements with no bound parameters.
    ///
    /// Used to apply a migration's SQL, which may hold several statements.
    pub async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        self.with_connection(move |conn| execute_batch(conn, &sql))
            .await
    }

    /// Returns the number of statements run through this pool so far.
    ///
    /// Useful in tests to confirm a query strategy (for example, that preloading
    /// adds one query per relation rather than one per row).
    pub fn statement_count(&self) -> u64 {
        self.inner.statements.load(Ordering::Relaxed)
    }

    /// Checks out a single connection and pins it for the caller's exclusive use.
    ///
    /// Every statement run through the returned handle uses the same connection,
    /// so a sequence such as `BEGIN`/.../`COMMIT` is sound regardless of the pool
    /// size. The connection returns to the pool when the handle is dropped. Used by
    /// the migration runner and the transaction API to pin a connection.
    pub(crate) async fn acquire_pinned(&self) -> crate::Result<PinnedSqlite> {
        let permit = self.inner.acquire_permit().await?;

        let checked_out = lock(&self.inner.idle).pop();
        let conn = match checked_out {
            Some(conn) => conn,
            None => {
                let inner = Arc::clone(&self.inner);
                tokio::task::spawn_blocking(move || inner.open())
                    .await
                    .map_err(|e| OrmError::connection(format!("database worker failed: {e}")))??
            }
        };

        Ok(PinnedSqlite {
            inner: Arc::clone(&self.inner),
            conn: Arc::new(Mutex::new(Some(conn))),
            _permit: permit,
        })
    }

    /// Drops all idle connections. In-flight calls keep their connection until done.
    pub async fn close(&self) {
        let drained = {
            let mut idle = lock(&self.inner.idle);
            std::mem::take(&mut *idle)
        };
        drop(drained);
    }

    /// Checks out a connection, runs the blocking work off-runtime, and returns it.
    async fn with_connection<F, T>(&self, work: F) -> crate::Result<T>
    where
        F: FnOnce(&mut Connection) -> crate::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.inner.statements.fetch_add(1, Ordering::Relaxed);

        // Bound concurrency before touching the blocking pool, failing fast if no
        // slot frees up within the checkout timeout instead of hanging forever.
        let permit = self.inner.acquire_permit().await?;

        let checked_out = lock(&self.inner.idle).pop();
        let inner = Arc::clone(&self.inner);

        // The blocking task returns the connection to the pool itself and reports
        // the result over a channel. This way, if the caller's future is cancelled
        // (for example by `tokio::time::timeout`) while the query runs, the
        // connection is still returned rather than dropped, so the pool never
        // leaks connections under request cancellation. The permit is held inside
        // the task so the concurrency bound stays correct until the work finishes.
        let (tx, rx) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut conn = match checked_out {
                Some(conn) => conn,
                None => match inner.open() {
                    Ok(conn) => conn,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                },
            };
            // A query error does not poison the connection, so it goes back to the
            // pool regardless; only a failed open leaves us without one.
            let result = work(&mut conn);
            lock(&inner.idle).push(conn);
            let _ = tx.send(result);
        });

        rx.await
            .map_err(|_| OrmError::query("database worker was dropped before completing"))?
    }
}

/// Locks the idle list, recovering from a poisoned mutex (the guarded data is a
/// plain `Vec` of connections, so a prior panic leaves it in a usable state).
fn lock(mutex: &Mutex<Vec<Connection>>) -> std::sync::MutexGuard<'_, Vec<Connection>> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Parses a database URL into a connection source.
fn parse_url(url: &str) -> crate::Result<Source> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(OrmError::configuration("database url is empty"));
    }

    // Strip an optional `sqlite:` / `sqlite://` scheme.
    let without_scheme = trimmed
        .strip_prefix("sqlite://")
        .or_else(|| trimmed.strip_prefix("sqlite:"))
        .unwrap_or(trimmed);

    if without_scheme == ":memory:" || without_scheme.is_empty() {
        return Ok(Source::Memory);
    }
    Ok(Source::File(without_scheme.to_string()))
}

/// Implements binding so a [`Value`] can be passed as a SQLite parameter.
impl ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let value = match self {
            Value::Null => SqliteValue::Null,
            Value::Bool(b) => SqliteValue::Integer(i64::from(*b)),
            Value::Int(i) => SqliteValue::Integer(*i),
            Value::Real(r) => SqliteValue::Real(*r),
            Value::Text(s) => SqliteValue::Text(s.clone()),
            Value::Blob(b) => SqliteValue::Blob(b.clone()),
            Value::Timestamp(ts) => SqliteValue::Text(format_timestamp(ts)?),
            // PostgreSQL-specific values: SQLite has no native types for these, and
            // a `sqlite`-declared project is rejected at compile time before reaching
            // here. Store a defensive text form for any value built directly.
            Value::Json(j) => SqliteValue::Text(j.to_string()),
            Value::Uuid(u) => SqliteValue::Text(u.to_string()),
            Value::Array(items) => SqliteValue::Text(format!("{items:?}")),
        };
        Ok(ToSqlOutput::Owned(value))
    }
}

/// Formats a timestamp as RFC 3339 text for storage.
fn format_timestamp(ts: &time::OffsetDateTime) -> rusqlite::Result<String> {
    ts.format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// Reads a driver-native column value into a backend-neutral [`Value`].
fn read_value(raw: ValueRef<'_>) -> crate::Result<Value> {
    Ok(match raw {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(r) => Value::Real(r),
        ValueRef::Text(bytes) => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| OrmError::conversion("column text is not valid UTF-8"))?;
            Value::Text(text.to_string())
        }
        ValueRef::Blob(bytes) => Value::Blob(bytes.to_vec()),
    })
}

/// Runs a row-returning query against a connection.
fn fetch_all(conn: &mut Connection, sql: &str, params: &[Value]) -> crate::Result<Vec<Row>> {
    let mut statement = conn
        .prepare_cached(sql)
        .map_err(|e| OrmError::query(format!("cannot prepare `{sql}`")).with_source(e))?;

    let column_names: Arc<[String]> = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into();
    let column_count = column_names.len();

    let mut rows = statement
        .query(rusqlite::params_from_iter(params.iter()))
        .map_err(|e| OrmError::query("query execution failed").with_source(e))?;

    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| OrmError::query("reading a row failed").with_source(e))?
    {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let raw = row
                .get_ref(index)
                .map_err(|e| OrmError::query("reading a column failed").with_source(e))?;
            values.push(read_value(raw)?);
        }
        out.push(Row::with_columns(Arc::clone(&column_names), values));
    }
    Ok(out)
}

/// Runs a batch of statements (no parameters) against a connection.
fn execute_batch(conn: &mut Connection, sql: &str) -> crate::Result<()> {
    conn.execute_batch(sql)
        .map_err(|e| OrmError::query("statement batch failed").with_source(e))
}

/// Runs a non-row-returning statement against a connection.
fn execute(conn: &mut Connection, sql: &str, params: &[Value]) -> crate::Result<ExecuteResult> {
    let affected = conn
        .prepare_cached(sql)
        .map_err(|e| OrmError::query(format!("cannot prepare `{sql}`")).with_source(e))?
        .execute(rusqlite::params_from_iter(params.iter()))
        .map_err(|e| OrmError::query("statement execution failed").with_source(e))?;

    Ok(ExecuteResult {
        rows_affected: affected as u64,
        last_insert_rowid: conn.last_insert_rowid(),
    })
}

/// A single connection pinned out of the pool for exclusive, sequential use.
///
/// Returns the connection to the pool when dropped.
pub(crate) struct PinnedSqlite {
    inner: Arc<Inner>,
    conn: Arc<Mutex<Option<Connection>>>,
    _permit: OwnedSemaphorePermit,
}

impl PinnedSqlite {
    /// Takes the connection out for one blocking operation, then puts it back.
    fn take_conn(&self) -> crate::Result<Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(|| OrmError::query("pinned connection is already in use"))
    }

    /// Runs one blocking operation on the pinned connection.
    ///
    /// The connection is restored to the shared slot inside the blocking task, so
    /// if the caller's future is cancelled mid-statement the connection is still
    /// returned to the handle (and, on drop, to the pool) rather than being lost.
    async fn run<F, T>(&self, work: F) -> crate::Result<T>
    where
        F: FnOnce(&mut Connection) -> crate::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.inner.statements.fetch_add(1, Ordering::Relaxed);
        let mut conn = self.take_conn()?;
        let slot = Arc::clone(&self.conn);
        let (tx, rx) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let result = work(&mut conn);
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(conn);
            let _ = tx.send(result);
        });
        rx.await
            .map_err(|_| OrmError::query("database worker was dropped before completing"))?
    }

    /// Runs a row-returning query on the pinned connection.
    pub(crate) async fn fetch_all(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<Vec<Row>> {
        self.run(move |conn| fetch_all(conn, &sql, &params)).await
    }

    /// Runs a non-row-returning statement on the pinned connection.
    pub(crate) async fn execute(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<ExecuteResult> {
        self.run(move |conn| execute(conn, &sql, &params)).await
    }

    /// Runs a batch of statements on the pinned connection.
    pub(crate) async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        self.run(move |conn| execute_batch(conn, &sql)).await
    }

    /// Rolls back synchronously without `spawn_blocking`.
    ///
    /// Safe to call from a `Drop` impl where no async context is available. If
    /// the connection is not currently available (the mutex is None because a
    /// concurrent spawn_blocking holds it), the rollback is skipped — SQLite
    /// closes any open transaction when the connection is eventually dropped.
    pub(crate) fn rollback_now(&self) {
        if let Ok(conn) = self.take_conn() {
            let _ = conn.execute_batch("ROLLBACK");
            *self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(conn);
        }
    }
}

impl Drop for PinnedSqlite {
    fn drop(&mut self) {
        if let Some(conn) = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            lock(&self.inner.idle).push(conn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl SqlitePool {
        /// Number of connections currently idle in the pool (test-only).
        fn idle_len(&self) -> usize {
            lock(&self.inner.idle).len()
        }
    }

    /// A row-returning query slow enough to be cancelled by a sub-millisecond
    /// timeout: a recursive CTE counting to several million.
    const SLOW_QUERY: &str = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL \
         SELECT n + 1 FROM c WHERE n < 4000000) SELECT count(*) FROM c";

    #[tokio::test]
    async fn checkout_times_out_instead_of_hanging_forever() {
        // One connection, a short checkout timeout. Pinning the only connection
        // leaves a pooled query with no slot, so it must fail fast rather than
        // wait forever.
        let pool = SqlitePool::new(":memory:", 1)
            .unwrap()
            .with_acquire_timeout(Duration::from_millis(50));

        let pinned = pool.acquire_pinned().await.unwrap();

        let start = std::time::Instant::now();
        let result = pool.fetch_all("SELECT 1".into(), vec![]).await;
        let waited = start.elapsed();

        let error = result.expect_err("checkout should time out");
        assert!(
            error.to_string().contains("timed out"),
            "expected a timeout error, got: {error}"
        );
        assert!(waited < Duration::from_secs(5), "must fail fast, not hang");

        // Releasing the pinned connection lets the next query proceed.
        drop(pinned);
        let rows = pool.fetch_all("SELECT 1".into(), vec![]).await.unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn cancelled_query_returns_its_connection_to_the_pool() {
        // A query whose future is cancelled (here by a timeout firing before it
        // finishes) must not lose its connection: the blocking task returns it to
        // the pool so later checkouts reuse it instead of the pool thrashing.
        let pool = SqlitePool::new(":memory:", 1).unwrap();

        let cancelled = tokio::time::timeout(
            Duration::from_millis(1),
            pool.fetch_all(SLOW_QUERY.into(), vec![]),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "the slow query should be cancelled by the timeout"
        );

        // The blocking task is still finishing the query; once it does, it returns
        // the connection to the idle pool. Wait for that to happen.
        let mut returned = false;
        for _ in 0..200 {
            if pool.idle_len() == 1 {
                returned = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            returned,
            "the cancelled query's connection was never returned to the pool"
        );

        // And the pool still serves queries on that single recovered connection.
        let rows = pool.fetch_all("SELECT 1".into(), vec![]).await.unwrap();
        assert_eq!(rows.len(), 1);
    }
}
