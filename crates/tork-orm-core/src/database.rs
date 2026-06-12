//! The [`Database`] handle: a cloneable entry point to a configured backend.
//!
//! A `Database` pairs a connection pool with the [`Dialect`](crate::dialect::Dialect)
//! that renders queries for it. It is cheap to clone (clones share the pool) and is
//! the value injected into handlers as `Arc<Database>` when the `tork` feature is
//! enabled.

use std::sync::Arc;

use crate::dialect::Dialect;
use crate::driver::ExecuteResult;
use crate::error::OrmError;
use crate::row::Row;
use crate::value::Value;

/// The concrete backend behind a [`Database`].
///
/// One variant per compiled-in driver; the active one is chosen from the URL scheme.
#[derive(Clone)]
enum Backend {
    #[cfg(feature = "sqlite")]
    Sqlite(crate::driver::sqlite::SqlitePool),
    #[cfg(feature = "postgres")]
    Postgres(crate::driver::postgres::PostgresPool),
    #[cfg(feature = "mysql")]
    Mysql(crate::driver::mysql::MysqlPool),
}

/// A handle to a database.
///
/// # Examples
///
/// ```no_run
/// use tork_orm_core::Database;
///
/// # async fn run() -> tork_orm_core::Result<()> {
/// let db = Database::connect("sqlite://app.db", 4).await?;
/// db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)".into(), vec![]).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Database {
    backend: Backend,
    dialect: Arc<dyn Dialect>,
}

impl Database {
    /// Connects to a database described by `url`, with up to `max_connections`
    /// concurrent connections.
    ///
    /// The backend is chosen from the URL scheme: `sqlite://...` (a bare path or
    /// `:memory:`) for SQLite, or `postgres://...` for PostgreSQL when the
    /// `postgres` feature is enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL names an unsupported backend or the pool cannot
    /// be created.
    pub async fn connect(url: &str, max_connections: u32) -> crate::Result<Self> {
        let scheme = url.split_once(':').map(|(scheme, _)| scheme).unwrap_or("");
        match scheme {
            #[cfg(feature = "sqlite")]
            "sqlite" | "" => {
                let pool = crate::driver::sqlite::SqlitePool::new(url, max_connections)?;
                Ok(Self {
                    backend: Backend::Sqlite(pool),
                    dialect: Arc::new(crate::dialect::SqliteDialect::new()),
                })
            }
            #[cfg(feature = "postgres")]
            "postgres" | "postgresql" => {
                let pool = crate::driver::postgres::PostgresPool::new(url, max_connections)?;
                Ok(Self {
                    backend: Backend::Postgres(pool),
                    dialect: Arc::new(crate::dialect::PostgresDialect::new()),
                })
            }
            // The PostgreSQL dialect (SQL/DDL generation) is always available, but
            // connecting needs the live driver, which is behind the `postgres`
            // feature. Recognize the scheme and give a specific message.
            #[cfg(not(feature = "postgres"))]
            "postgres" | "postgresql" => Err(OrmError::configuration(
                "this build cannot connect to PostgreSQL; enable the `postgres` \
                 feature to compile the driver",
            )),
            #[cfg(feature = "mysql")]
            "mysql" | "mariadb" => {
                let pool = crate::driver::mysql::MysqlPool::new(url, max_connections)?;
                Ok(Self {
                    backend: Backend::Mysql(pool),
                    dialect: Arc::new(crate::dialect::MySqlDialect::new()),
                })
            }
            #[cfg(not(feature = "mysql"))]
            "mysql" | "mariadb" => Err(OrmError::configuration(
                "this build cannot connect to MySQL; enable the `mysql` feature to \
                 compile the driver",
            )),
            other => Err(OrmError::configuration(format!(
                "no compiled-in backend for url scheme `{other}`"
            ))),
        }
    }

    /// Returns the dialect this database renders queries with.
    pub fn dialect(&self) -> &Arc<dyn Dialect> {
        &self.dialect
    }

    /// Runs a row-returning query with bound parameters.
    pub async fn fetch_all(&self, sql: String, params: Vec<Value>) -> crate::Result<Vec<Row>> {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(pool) => pool.fetch_all(sql, params).await,
            #[cfg(feature = "postgres")]
            Backend::Postgres(pool) => pool.fetch_all(sql, params).await,
            #[cfg(feature = "mysql")]
            Backend::Mysql(pool) => pool.fetch_all(sql, params).await,
        }
    }

    /// Runs a statement that returns no rows.
    pub async fn execute(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<ExecuteResult> {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(pool) => pool.execute(sql, params).await,
            #[cfg(feature = "postgres")]
            Backend::Postgres(pool) => pool.execute(sql, params).await,
            #[cfg(feature = "mysql")]
            Backend::Mysql(pool) => pool.execute(sql, params).await,
        }
    }

    /// Runs a batch of semicolon-separated statements with no bound parameters.
    pub async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(pool) => pool.execute_batch(sql).await,
            #[cfg(feature = "postgres")]
            Backend::Postgres(pool) => pool.execute_batch(sql).await,
            #[cfg(feature = "mysql")]
            Backend::Mysql(pool) => pool.execute_batch(sql).await,
        }
    }

    /// Returns the number of statements run through this database so far.
    ///
    /// Useful in tests to confirm a query strategy (such as preloading adding one
    /// query per relation, not one per row).
    pub fn statement_count(&self) -> u64 {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(pool) => pool.statement_count(),
            #[cfg(feature = "postgres")]
            Backend::Postgres(pool) => pool.statement_count(),
            #[cfg(feature = "mysql")]
            Backend::Mysql(pool) => pool.statement_count(),
        }
    }

    /// Releases idle connections held by the pool.
    pub async fn close(&self) {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(pool) => pool.close().await,
            #[cfg(feature = "postgres")]
            Backend::Postgres(pool) => pool.close().await,
            #[cfg(feature = "mysql")]
            Backend::Mysql(pool) => pool.close().await,
        }
    }

    /// Pins a single connection for exclusive, sequential use.
    ///
    /// Used by the migration runner and the transaction API so a sequence of
    /// statements (including `BEGIN`/`COMMIT`) all run on the same connection.
    pub(crate) async fn pinned(&self) -> crate::Result<Pinned> {
        let backend = match &self.backend {
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(pool) => PinnedBackend::Sqlite(pool.acquire_pinned().await?),
            #[cfg(feature = "postgres")]
            Backend::Postgres(pool) => PinnedBackend::Postgres(pool.acquire_pinned().await?),
            #[cfg(feature = "mysql")]
            Backend::Mysql(pool) => PinnedBackend::Mysql(pool.acquire_pinned().await?),
        };
        Ok(Pinned {
            backend,
            dialect: Arc::clone(&self.dialect),
        })
    }
}

/// A pinned connection exposed as an [`Executor`](crate::Executor).
pub(crate) struct Pinned {
    backend: PinnedBackend,
    dialect: Arc<dyn Dialect>,
}

/// The backend-specific pinned connection.
enum PinnedBackend {
    #[cfg(feature = "sqlite")]
    Sqlite(crate::driver::sqlite::PinnedSqlite),
    #[cfg(feature = "postgres")]
    Postgres(crate::driver::postgres::PinnedPostgres),
    #[cfg(feature = "mysql")]
    Mysql(crate::driver::mysql::PinnedMysql),
}

impl crate::executor::Executor for Pinned {
    fn dialect(&self) -> &dyn Dialect {
        self.dialect.as_ref()
    }

    async fn fetch_all(&self, sql: String, params: Vec<Value>) -> crate::Result<Vec<Row>> {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            PinnedBackend::Sqlite(pinned) => pinned.fetch_all(sql, params).await,
            #[cfg(feature = "postgres")]
            PinnedBackend::Postgres(pinned) => pinned.fetch_all(sql, params).await,
            #[cfg(feature = "mysql")]
            PinnedBackend::Mysql(pinned) => pinned.fetch_all(sql, params).await,
        }
    }

    async fn execute(&self, sql: String, params: Vec<Value>) -> crate::Result<ExecuteResult> {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            PinnedBackend::Sqlite(pinned) => pinned.execute(sql, params).await,
            #[cfg(feature = "postgres")]
            PinnedBackend::Postgres(pinned) => pinned.execute(sql, params).await,
            #[cfg(feature = "mysql")]
            PinnedBackend::Mysql(pinned) => pinned.execute(sql, params).await,
        }
    }
}

impl Pinned {
    /// Runs a batch of statements on the pinned connection.
    pub(crate) async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            PinnedBackend::Sqlite(pinned) => pinned.execute_batch(sql).await,
            #[cfg(feature = "postgres")]
            PinnedBackend::Postgres(pinned) => pinned.execute_batch(sql).await,
            #[cfg(feature = "mysql")]
            PinnedBackend::Mysql(pinned) => pinned.execute_batch(sql).await,
        }
    }

    /// Rolls back synchronously without spawning a task.
    ///
    /// Intended for `Drop` impls where no async runtime is available.
    pub(crate) fn rollback_now(&self) {
        match &self.backend {
            #[cfg(feature = "sqlite")]
            PinnedBackend::Sqlite(pinned) => pinned.rollback_now(),
            #[cfg(feature = "postgres")]
            PinnedBackend::Postgres(pinned) => pinned.rollback_now(),
            #[cfg(feature = "mysql")]
            PinnedBackend::Mysql(pinned) => pinned.rollback_now(),
        }
    }
}
