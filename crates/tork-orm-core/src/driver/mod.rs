//! Database drivers: the bridge between the backend-neutral query layer and a
//! concrete database engine.
//!
//! A driver owns its connections, runs SQL with bound parameters off the async
//! runtime, and reads results back into [`Row`](crate::Row)s. This phase ships a
//! single SQLite driver; future backends add a sibling module behind their feature.
//!
//! # Adding a backend
//!
//! Dispatch lives in [`Database`](crate::Database) over a private `Backend` enum
//! (a static enum, not dynamic dispatch). A new driver provides this surface and
//! gets a new enum arm:
//!
//! - `fetch_all(sql, params) -> Vec<Row>`
//! - `execute(sql, params) -> ExecuteResult`
//! - `execute_batch(sql) -> ()`
//! - `statement_count() -> u64`
//! - `close()`
//! - `acquire_pinned() -> <pinned connection>` exposing `fetch_all` / `execute` /
//!   `execute_batch` / `rollback_now` for transactions and migrations.
//!
//! A formal `Driver` trait is intentionally deferred until a second driver exists,
//! so its shape is informed by a real second backend rather than guessed.

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

/// The outcome of a statement that does not return rows.
///
/// Returned by execute-style calls (`INSERT` / `UPDATE` / `DELETE` and DDL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecuteResult {
    /// The number of rows the statement changed.
    pub rows_affected: u64,
    /// The row id of the most recent insert on this connection.
    ///
    /// Meaningful only on backends with an implicit row id (SQLite's `rowid`);
    /// `0` when no insert produced one. Backends without a row id (such as
    /// PostgreSQL) leave this `0` and return generated keys via `RETURNING`
    /// instead.
    pub last_insert_rowid: i64,
}
