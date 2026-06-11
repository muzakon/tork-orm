//! The [`Transaction`] handle for explicit and closure-based transactions.
//!
//! A `Transaction` wraps a pinned connection that has had `BEGIN` run on it. It
//! implements [`Executor`], so every ORM query method that accepts `impl Executor`
//! works transparently against a transaction handle.
//!
//! # Explicit API
//!
//! ```no_run
//! # use tork_orm_core::{Database, Executor};
//! # async fn run(db: &Database) -> tork_orm_core::Result<()> {
//! let mut tx = db.begin().await?;
//! db.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
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

use crate::database::{Database, Pinned};
use crate::dialect::Dialect;
use crate::driver::ExecuteResult;
use crate::executor::Executor;
use crate::row::Row;
use crate::value::Value;

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
}

impl Transaction {
    pub(crate) fn new(inner: Pinned) -> Self {
        Self { inner, committed: false }
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
