//! The [`Transaction`] handle for explicit and closure-based transactions.
//!
//! A `Transaction` wraps a pinned connection that has had `BEGIN` run on it. It
//! implements [`Executor`], so every ORM query method that accepts `impl Executor`
//! works transparently against a transaction handle.
//!
//! # Closure API (recommended)
//!
//! ```no_run
//! use tork_orm_core::{Database, Executor};
//!
//! # async fn run() -> tork_orm_core::Result<()> {
//! let db = Database::connect("sqlite::memory:", 1).await?;
//! db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await?;
//! let inserted = db.transaction(|tx| Box::pin(async move {
//!     tx.execute("INSERT INTO t VALUES (42)".into(), vec![]).await?;
//!     Ok(1_usize)
//! })).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Explicit API
//!
//! ```no_run
//! use tork_orm_core::{Database, Executor};
//!
//! # async fn run() -> tork_orm_core::Result<()> {
//! let db = Database::connect("sqlite::memory:", 1).await?;
//! db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await?;
//! let mut tx = db.begin().await?;
//! tx.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
//! tx.commit().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Auto-rollback on drop
//!
//! If `commit()` or `rollback()` is never called the transaction is rolled back
//! when the handle is dropped. The rollback runs synchronously on the same thread
//! that drops the value so no async runtime is needed.

use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::database::{Database, Pinned};
use crate::dialect::Dialect;
use crate::driver::ExecuteResult;
use crate::executor::Executor;
use crate::row::Row;
use crate::value::Value;
use crate::BoxFuture;

/// An open database transaction.
///
/// Wraps a pinned connection with an active `BEGIN`. Implements [`Executor`] so
/// it can be passed directly to any ORM query method in place of a [`Database`].
///
/// Drop the handle without calling [`commit`](Transaction::commit) and the
/// transaction is rolled back automatically.
pub struct Transaction {
    inner: Pinned,
    /// Prevents a second rollback in `Drop` after `commit` or `rollback` ran.
    committed: bool,
    /// Monotonic counter for unique savepoint names within this transaction.
    savepoint_counter: AtomicU32,
}

impl Transaction {
    pub(crate) fn new(inner: Pinned) -> Self {
        Self { inner, committed: false, savepoint_counter: AtomicU32::new(0) }
    }

    /// Commits the transaction.
    ///
    /// If the `COMMIT` statement fails, a best-effort `ROLLBACK` is issued before
    /// the error is returned so the connection is always left in a clean state.
    ///
    /// # Errors
    ///
    /// Returns the database error if `COMMIT` fails.
    pub async fn commit(&mut self) -> crate::Result<()> {
        // Mark first so Drop does not attempt a second rollback if we return early.
        self.committed = true;
        let commit_sql = self.inner.dialect().commit_sql().to_string();
        let rollback_sql = self.inner.dialect().rollback_sql().to_string();
        let result = self.inner.execute(commit_sql, vec![]).await;
        if result.is_err() {
            let _ = self.inner.execute(rollback_sql, vec![]).await;
        }
        result.map(|_| ())
    }

    /// Rolls back the transaction explicitly.
    ///
    /// # Errors
    ///
    /// Returns the database error if `ROLLBACK` fails.
    pub async fn rollback(&mut self) -> crate::Result<()> {
        self.committed = true;
        let sql = self.inner.dialect().rollback_sql().to_string();
        self.inner.execute(sql, vec![]).await.map(|_| ())
    }

    /// Runs `f` inside a savepoint nested within this transaction.
    ///
    /// The savepoint is committed (released) on `Ok` and rolled back on `Err`.
    /// Unlike a top-level transaction rollback, rolling back a savepoint only
    /// undoes the work done inside `f`; the outer transaction continues.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tork_orm_core::{Database, Executor, OrmError};
    ///
    /// # async fn run() -> tork_orm_core::Result<()> {
    /// let db = Database::connect("sqlite::memory:", 1).await?;
    /// db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await?;
    /// db.transaction(|tx| Box::pin(async move {
    ///     tx.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
    ///     // This inner failure only undoes the INSERT of 2; the INSERT of 1 survives.
    ///     let _ = tx.savepoint(|sp| Box::pin(async move {
    ///         sp.execute("INSERT INTO t VALUES (2)".into(), vec![]).await?;
    ///         Err::<(), _>(OrmError::query("oops"))
    ///     })).await;
    ///     Ok(())
    /// })).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the savepoint SQL fails or if `f` returns an error
    /// (after rolling back the savepoint).
    pub async fn savepoint<F, R>(&self, f: F) -> crate::Result<R>
    where
        F: for<'a> FnOnce(&'a Transaction) -> BoxFuture<'a, crate::Result<R>>,
        R: Send + 'static,
    {
        let n = self.savepoint_counter.fetch_add(1, Ordering::Relaxed);
        let sp_name = format!("tork_sp_{n}");

        let savepoint_sql = self.inner.dialect().savepoint_sql(&sp_name);
        let release_sql = self.inner.dialect().release_sql(&sp_name);
        let rollback_to_sql = self.inner.dialect().rollback_to_sql(&sp_name);

        self.inner.execute(savepoint_sql, vec![]).await?;

        match f(self).await {
            Ok(value) => {
                self.inner.execute(release_sql, vec![]).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = self.inner.execute(rollback_to_sql, vec![]).await;
                Err(error)
            }
        }
    }
}

impl Executor for Transaction {
    fn dialect(&self) -> &dyn Dialect {
        self.inner.dialect()
    }

    fn fetch_all(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = crate::Result<Vec<Row>>> + Send {
        self.inner.fetch_all(sql, params)
    }

    fn execute(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = crate::Result<ExecuteResult>> + Send {
        self.inner.execute(sql, params)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.committed {
            self.inner.rollback_now();
        }
    }
}

impl Database {
    /// Runs `f` inside a transaction, committing on `Ok` and rolling back on `Err`.
    ///
    /// This is the preferred way to run transactional work. The closure receives a
    /// `&Transaction` that implements [`Executor`], so every ORM method works
    /// against it without modification.
    ///
    /// The future must be boxed because the compiler cannot name the return type
    /// of an async closure; use [`Box::pin`] or the `transaction!` macro from
    /// the `tork-orm` facade, which adds that boilerplate automatically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tork_orm_core::{Database, Executor};
    ///
    /// # async fn run() -> tork_orm_core::Result<()> {
    /// let db = Database::connect("sqlite::memory:", 1).await?;
    /// db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await?;
    /// db.transaction(|tx| Box::pin(async move {
    ///     tx.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
    ///     tx.execute("INSERT INTO t VALUES (2)".into(), vec![]).await?;
    ///     Ok(())
    /// })).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if `BEGIN`, any statement inside `f`, or `COMMIT` fails.
    pub async fn transaction<F, R>(&self, f: F) -> crate::Result<R>
    where
        F: for<'a> FnOnce(&'a Transaction) -> BoxFuture<'a, crate::Result<R>>,
        R: Send + 'static,
    {
        let mut tx = self.begin().await?;
        match f(&tx).await {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Opens a new transaction on a pinned connection.
    ///
    /// Runs `BEGIN` on the connection and returns a [`Transaction`] handle.
    /// The handle implements [`Executor`], so it can be passed directly to ORM
    /// query methods. Call [`Transaction::commit`] to persist the work, or let
    /// the handle drop to roll back automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool has no connections available or `BEGIN`
    /// fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tork_orm_core::Database;
    ///
    /// # async fn run() -> tork_orm_core::Result<()> {
    /// let db = Database::connect("sqlite::memory:", 1).await?;
    /// let mut tx = db.begin().await?;
    /// db.execute("CREATE TABLE t (x INTEGER)".into(), vec![]).await?;
    /// tx.commit().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn begin(&self) -> crate::Result<Transaction> {
        let pinned = self.pinned().await?;
        let begin_sql = pinned.dialect().begin_sql().to_string();
        pinned.execute(begin_sql, vec![]).await?;
        Ok(Transaction::new(pinned))
    }
}
