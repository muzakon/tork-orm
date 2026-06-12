//! The MySQL driver.
//!
//! Built on `mysql_async`, which is natively asynchronous and provides its own
//! connection pool. The driver mirrors the SQLite/PostgreSQL surface (`fetch_all`,
//! `execute`, `execute_batch`, `acquire_pinned`, `statement_count`, `close`) so it
//! slots into the same `Backend`-enum dispatch in [`Database`](crate::Database).
//!
//! MySQL has no `RETURNING`; `Model::create` re-selects by the `LAST_INSERT_ID()`
//! reported in [`ExecuteResult::last_insert_rowid`]. Connections are plaintext.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, Params, Pool, PoolConstraints, PoolOpts};
use mysql_async::{Row as MyRow, Value as MyValue};
use tokio::sync::Mutex;

use crate::driver::ExecuteResult;
use crate::error::OrmError;
use crate::row::Row;
use crate::value::Value;

/// The MySQL binary charset id, used to tell a `BLOB` from a `TEXT` column (both
/// share the protocol blob type; only the charset distinguishes them).
const BINARY_CHARSET: u16 = 63;

/// A pool of reusable MySQL connections.
#[derive(Clone)]
pub struct MysqlPool {
    pool: Pool,
    statements: Arc<AtomicU64>,
}

impl MysqlPool {
    /// Builds a pool from a `mysql://` URL and a maximum connection count.
    ///
    /// # Errors
    ///
    /// Returns an error if `max_connections` is zero or the URL cannot be parsed.
    pub fn new(url: &str, max_connections: u32) -> crate::Result<Self> {
        if max_connections == 0 {
            return Err(OrmError::configuration("max_connections must be at least 1"));
        }
        let opts = Opts::from_url(url)
            .map_err(|e| OrmError::configuration("invalid MySQL url").with_source(e))?;
        let constraints = PoolConstraints::new(0, max_connections as usize)
            .ok_or_else(|| OrmError::configuration("invalid MySQL pool size"))?;
        let opts = OptsBuilder::from_opts(opts)
            .pool_opts(PoolOpts::default().with_constraints(constraints));
        Ok(Self { pool: Pool::new(opts), statements: Arc::new(AtomicU64::new(0)) })
    }

    /// Runs a query that returns rows.
    pub async fn fetch_all(&self, sql: String, params: Vec<Value>) -> crate::Result<Vec<Row>> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let mut conn = self.get().await?;
        let rows = query(&mut conn, &sql, params).await;
        rows
    }

    /// Runs a statement that returns no rows.
    pub async fn execute(&self, sql: String, params: Vec<Value>) -> crate::Result<ExecuteResult> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let mut conn = self.get().await?;
        execute(&mut conn, &sql, params).await
    }

    /// Runs a batch of semicolon-separated statements with no bound parameters.
    pub async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let mut conn = self.get().await?;
        execute_batch(&mut conn, &sql).await
    }

    /// Returns the number of statements run through this pool so far.
    pub fn statement_count(&self) -> u64 {
        self.statements.load(Ordering::Relaxed)
    }

    /// Closes the pool, dropping idle connections.
    pub async fn close(&self) {
        let _ = self.pool.clone().disconnect().await;
    }

    /// Checks out a single connection and pins it for exclusive, sequential use.
    pub(crate) async fn acquire_pinned(&self) -> crate::Result<PinnedMysql> {
        let conn = self.get().await?;
        Ok(PinnedMysql {
            conn: Mutex::new(Some(conn)),
            statements: Arc::clone(&self.statements),
        })
    }

    /// Checks a connection out of the pool.
    async fn get(&self) -> crate::Result<Conn> {
        self.pool
            .get_conn()
            .await
            .map_err(|e| OrmError::connection("cannot acquire a MySQL connection").with_source(e))
    }
}

/// Runs a row-returning query on a connection.
///
/// Always uses a prepared statement (binary protocol) so values come back typed
/// rather than as text — the text protocol returns every column as bytes.
async fn query(conn: &mut Conn, sql: &str, params: Vec<Value>) -> crate::Result<Vec<Row>> {
    let bound = Params::Positional(params.into_iter().map(to_my_value).collect());
    let rows: Vec<MyRow> = conn
        .exec(sql, bound)
        .await
        .map_err(|e| OrmError::query("query execution failed").with_source(e))?;
    rows.into_iter().map(read_row).collect()
}

/// Runs a non-row-returning statement, returning affected rows and the last insert id.
///
/// Paramless statements use the text protocol, because transaction-control commands
/// (`START TRANSACTION`, `SAVEPOINT`, `COMMIT`, …) are not allowed in the prepared
/// protocol. Parameterized statements use a prepared statement.
async fn execute(conn: &mut Conn, sql: &str, params: Vec<Value>) -> crate::Result<ExecuteResult> {
    if params.is_empty() {
        conn.query_drop(sql)
            .await
            .map_err(|e| OrmError::query("statement execution failed").with_source(e))?;
    } else {
        let bound = Params::Positional(params.into_iter().map(to_my_value).collect());
        conn.exec_drop(sql, bound)
            .await
            .map_err(|e| OrmError::query("statement execution failed").with_source(e))?;
    }
    Ok(ExecuteResult {
        rows_affected: conn.affected_rows(),
        last_insert_rowid: conn.last_insert_id().unwrap_or(0) as i64,
    })
}

/// Runs a batch of statements. MySQL multi-statement support is not assumed; the
/// batch is split on `;` and each non-empty statement is run in turn.
async fn execute_batch(conn: &mut Conn, sql: &str) -> crate::Result<()> {
    for statement in sql.split(';') {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        conn.query_drop(statement)
            .await
            .map_err(|e| OrmError::query("statement batch failed").with_source(e))?;
    }
    Ok(())
}

/// Converts a bound [`Value`] into a `mysql_async` value.
fn to_my_value(value: Value) -> MyValue {
    match value {
        Value::Null => MyValue::NULL,
        Value::Bool(b) => MyValue::Int(i64::from(b)),
        Value::Int(i) => MyValue::Int(i),
        Value::Real(r) => MyValue::Double(r),
        Value::Text(s) => MyValue::Bytes(s.into_bytes()),
        Value::Blob(b) => MyValue::Bytes(b),
        Value::Timestamp(ts) => {
            let utc = ts.to_offset(time::UtcOffset::UTC);
            MyValue::Date(
                utc.year() as u16,
                u8::from(utc.month()),
                utc.day(),
                utc.hour(),
                utc.minute(),
                utc.second(),
                utc.microsecond(),
            )
        }
        // MySQL accepts a JSON document as a string literal for a JSON column.
        Value::Json(json) => MyValue::Bytes(json.to_string().into_bytes()),
        Value::Uuid(uuid) => MyValue::Bytes(uuid.to_string().into_bytes()),
        Value::Array(items) => MyValue::Bytes(format!("{items:?}").into_bytes()),
    }
}

/// Reads a `mysql_async` row into a backend-neutral [`Row`].
fn read_row(row: MyRow) -> crate::Result<Row> {
    let columns = row.columns();
    let names: Arc<[String]> = columns
        .iter()
        .map(|column| column.name_str().into_owned())
        .collect::<Vec<_>>()
        .into();
    let mut values = Vec::with_capacity(columns.len());
    for (index, column) in columns.iter().enumerate() {
        let raw = row.as_ref(index).cloned().unwrap_or(MyValue::NULL);
        values.push(read_value(&raw, column.column_type(), column.character_set())?);
    }
    Ok(Row::with_columns(names, values))
}

/// Converts a `mysql_async` value into a [`Value`], using the column's type and
/// charset to disambiguate text/blob/json.
fn read_value(value: &MyValue, column_type: ColumnType, charset: u16) -> crate::Result<Value> {
    Ok(match value {
        MyValue::NULL => Value::Null,
        MyValue::Int(i) => Value::Int(*i),
        MyValue::UInt(u) => Value::Int(*u as i64),
        MyValue::Float(f) => Value::Real(f64::from(*f)),
        MyValue::Double(f) => Value::Real(*f),
        MyValue::Date(year, month, day, hour, min, sec, micro) => {
            let date = time::Date::from_calendar_date(
                i32::from(*year),
                time::Month::try_from(*month)
                    .map_err(|_| OrmError::conversion("invalid month in DATETIME"))?,
                *day,
            )
            .map_err(|_| OrmError::conversion("invalid DATETIME date"))?;
            let clock = time::Time::from_hms_micro(*hour, *min, *sec, *micro)
                .map_err(|_| OrmError::conversion("invalid DATETIME time"))?;
            Value::Timestamp(time::PrimitiveDateTime::new(date, clock).assume_utc())
        }
        // `TIME` columns are uncommon here; surface them as text.
        MyValue::Time(neg, days, hours, mins, secs, micros) => Value::Text(format!(
            "{}{}:{:02}:{:02}:{:02}.{:06}",
            if *neg { "-" } else { "" },
            days,
            hours,
            mins,
            secs,
            micros
        )),
        MyValue::Bytes(bytes) => {
            if column_type == ColumnType::MYSQL_TYPE_JSON {
                serde_json::from_slice(bytes)
                    .map(Value::Json)
                    .unwrap_or_else(|_| Value::Text(String::from_utf8_lossy(bytes).into_owned()))
            } else if is_binary_blob(column_type, charset) {
                Value::Blob(bytes.clone())
            } else {
                Value::Text(String::from_utf8_lossy(bytes).into_owned())
            }
        }
    })
}

/// Returns `true` if a `Bytes` column holds binary data (a real `BLOB`), rather
/// than text — MySQL gives `TEXT` and `BLOB` the same protocol type, so the binary
/// charset is what distinguishes them.
fn is_binary_blob(column_type: ColumnType, charset: u16) -> bool {
    matches!(
        column_type,
        ColumnType::MYSQL_TYPE_BLOB
            | ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_GEOMETRY
    ) && charset == BINARY_CHARSET
}

/// A single connection pinned out of the pool for exclusive, sequential use.
pub(crate) struct PinnedMysql {
    conn: Mutex<Option<Conn>>,
    statements: Arc<AtomicU64>,
}

impl PinnedMysql {
    /// Runs a row-returning query on the pinned connection.
    pub(crate) async fn fetch_all(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<Vec<Row>> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.conn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| OrmError::query("pinned connection is unavailable"))?;
        query(conn, &sql, params).await
    }

    /// Runs a non-row-returning statement on the pinned connection.
    pub(crate) async fn execute(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<ExecuteResult> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.conn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| OrmError::query("pinned connection is unavailable"))?;
        execute(conn, &sql, params).await
    }

    /// Runs a batch of statements on the pinned connection.
    pub(crate) async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.conn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| OrmError::query("pinned connection is unavailable"))?;
        execute_batch(conn, &sql).await
    }

    /// Rolls back the open transaction without awaiting, for use from `Drop`.
    ///
    /// Best-effort: spawns the `ROLLBACK` onto the current runtime when one is
    /// present. The common paths roll back through the async `execute` above.
    pub(crate) fn rollback_now(&self) {
        let Ok(mut guard) = self.conn.try_lock() else { return };
        let Some(mut conn) = guard.take() else { return };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = conn.query_drop("ROLLBACK").await;
                // `conn` drops here, returning the connection to the pool.
            });
        }
    }
}
