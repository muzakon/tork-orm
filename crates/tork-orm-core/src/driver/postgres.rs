//! The PostgreSQL driver.
//!
//! Unlike the SQLite driver, `tokio-postgres` is natively asynchronous, so there is
//! no `spawn_blocking`; connection pooling and recycling are delegated to
//! `deadpool-postgres`. The driver mirrors the SQLite driver's public surface
//! (`fetch_all`, `execute`, `execute_batch`, `acquire_pinned`, `statement_count`,
//! `close`) so it slots into the same `Backend`-enum dispatch in
//! [`Database`](crate::Database).
//!
//! Connections are plaintext (`NoTls`); TLS is a later addition.

use std::error::Error as StdError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::BytesMut;
use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use tokio_postgres::types::{to_sql_checked, IsNull, ToSql, Type};
use tokio_postgres::NoTls;

use crate::driver::ExecuteResult;
use crate::error::OrmError;
use crate::row::Row;
use crate::value::Value;

/// A boxed error as required by the `tokio-postgres` `ToSql` trait.
type BoxError = Box<dyn StdError + Sync + Send>;

/// A pool of reusable PostgreSQL connections.
///
/// Cloning is cheap; clones share the same underlying `deadpool` pool.
#[derive(Clone)]
pub struct PostgresPool {
    pool: Pool,
    statements: Arc<AtomicU64>,
}

impl PostgresPool {
    /// Builds a pool from a `postgres://` URL and a maximum connection count.
    ///
    /// Connections are created lazily, so a bad host surfaces on first use rather
    /// than here (matching the SQLite driver). The URL is parsed eagerly, so a
    /// malformed URL is reported immediately.
    ///
    /// # Errors
    ///
    /// Returns an error if `max_connections` is zero, the URL cannot be parsed, or
    /// the pool cannot be built.
    pub fn new(url: &str, max_connections: u32) -> crate::Result<Self> {
        if max_connections == 0 {
            return Err(OrmError::configuration("max_connections must be at least 1"));
        }
        let pg_config: tokio_postgres::Config = url
            .parse()
            .map_err(|e| OrmError::configuration("invalid PostgreSQL url").with_source(e))?;
        let manager = Manager::from_config(
            pg_config,
            NoTls,
            ManagerConfig { recycling_method: RecyclingMethod::Fast },
        );
        let pool = Pool::builder(manager)
            .max_size(max_connections as usize)
            .build()
            .map_err(|e| OrmError::configuration("cannot build PostgreSQL pool").with_source(e))?;
        Ok(Self { pool, statements: Arc::new(AtomicU64::new(0)) })
    }

    /// Runs a query that returns rows.
    pub async fn fetch_all(&self, sql: String, params: Vec<Value>) -> crate::Result<Vec<Row>> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let client = self.get().await?;
        query(&client, &sql, &params).await
    }

    /// Runs a statement that returns no rows.
    pub async fn execute(&self, sql: String, params: Vec<Value>) -> crate::Result<ExecuteResult> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let client = self.get().await?;
        execute(&client, &sql, &params).await
    }

    /// Runs a batch of semicolon-separated statements with no bound parameters.
    pub async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let client = self.get().await?;
        client
            .batch_execute(&sql)
            .await
            .map_err(|e| OrmError::query("statement batch failed").with_source(e))
    }

    /// Returns the number of statements run through this pool so far.
    pub fn statement_count(&self) -> u64 {
        self.statements.load(Ordering::Relaxed)
    }

    /// Closes the pool, dropping idle connections.
    pub async fn close(&self) {
        self.pool.close();
    }

    /// Checks out a single connection and pins it for exclusive, sequential use.
    ///
    /// Used by the migration runner and transaction API so a `BEGIN`/.../`COMMIT`
    /// sequence runs on one connection.
    pub(crate) async fn acquire_pinned(&self) -> crate::Result<PinnedPostgres> {
        let object = self.get().await?;
        Ok(PinnedPostgres {
            object: Mutex::new(Some(object)),
            statements: Arc::clone(&self.statements),
        })
    }

    /// Checks a connection out of the pool.
    async fn get(&self) -> crate::Result<Object> {
        self.pool
            .get()
            .await
            .map_err(|e| OrmError::connection("cannot acquire a PostgreSQL connection").with_source(e))
    }
}

/// Runs a row-returning query on a checked-out client.
async fn query(client: &Object, sql: &str, params: &[Value]) -> crate::Result<Vec<Row>> {
    let bound: Vec<&(dyn ToSql + Sync)> = params.iter().map(|v| v as &(dyn ToSql + Sync)).collect();
    let rows = client
        .query(sql, &bound)
        .await
        .map_err(|e| OrmError::query("query execution failed").with_source(e))?;
    rows.iter().map(read_row).collect()
}

/// Runs a non-row-returning statement on a checked-out client.
async fn execute(client: &Object, sql: &str, params: &[Value]) -> crate::Result<ExecuteResult> {
    let bound: Vec<&(dyn ToSql + Sync)> = params.iter().map(|v| v as &(dyn ToSql + Sync)).collect();
    let affected = client
        .execute(sql, &bound)
        .await
        .map_err(|e| OrmError::query("statement execution failed").with_source(e))?;
    Ok(ExecuteResult {
        rows_affected: affected,
        // PostgreSQL has no implicit row id; generated keys come back via RETURNING.
        last_insert_rowid: 0,
    })
}

/// Reads a `tokio-postgres` row into a backend-neutral [`Row`].
fn read_row(row: &tokio_postgres::Row) -> crate::Result<Row> {
    let columns: Arc<[String]> = row
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect::<Vec<_>>()
        .into();
    let mut values = Vec::with_capacity(row.len());
    for index in 0..row.len() {
        values.push(read_value(row, index)?);
    }
    Ok(Row::with_columns(columns, values))
}

/// Reads a single column into a [`Value`], keyed on its runtime type, mapping SQL
/// `NULL` to [`Value::Null`].
fn read_value(row: &tokio_postgres::Row, index: usize) -> crate::Result<Value> {
    let ty = row.columns()[index].type_();
    if let tokio_postgres::types::Kind::Array(element) = ty.kind() {
        return read_array(row, index, element);
    }
    let value = if *ty == Type::BOOL {
        get_opt::<bool>(row, index)?.map_or(Value::Null, Value::Bool)
    } else if *ty == Type::INT2 {
        get_opt::<i16>(row, index)?.map_or(Value::Null, |n| Value::Int(i64::from(n)))
    } else if *ty == Type::INT4 {
        get_opt::<i32>(row, index)?.map_or(Value::Null, |n| Value::Int(i64::from(n)))
    } else if *ty == Type::INT8 {
        get_opt::<i64>(row, index)?.map_or(Value::Null, Value::Int)
    } else if *ty == Type::FLOAT4 {
        get_opt::<f32>(row, index)?.map_or(Value::Null, |n| Value::Real(f64::from(n)))
    } else if *ty == Type::FLOAT8 {
        get_opt::<f64>(row, index)?.map_or(Value::Null, Value::Real)
    } else if *ty == Type::BYTEA {
        get_opt::<Vec<u8>>(row, index)?.map_or(Value::Null, Value::Blob)
    } else if *ty == Type::TIMESTAMPTZ {
        get_opt::<time::OffsetDateTime>(row, index)?.map_or(Value::Null, Value::Timestamp)
    } else if *ty == Type::JSON || *ty == Type::JSONB {
        get_opt::<serde_json::Value>(row, index)?.map_or(Value::Null, Value::Json)
    } else if *ty == Type::UUID {
        get_opt::<uuid::Uuid>(row, index)?.map_or(Value::Null, Value::Uuid)
    } else {
        // TEXT, VARCHAR, BPCHAR, NAME, and anything else: read as text.
        get_opt::<String>(row, index)?.map_or(Value::Null, Value::Text)
    };
    Ok(value)
}

/// Reads an array column into a [`Value::Array`], mapping each element by the array's
/// element type. A `NULL` array column becomes [`Value::Null`].
fn read_array(row: &tokio_postgres::Row, index: usize, element: &Type) -> crate::Result<Value> {
    /// Reads `Option<Vec<Option<$t>>>` and wraps each element with `$wrap`.
    macro_rules! read_elements {
        ($t:ty, $wrap:expr) => {{
            let column: Option<Vec<Option<$t>>> = row
                .try_get(index)
                .map_err(|e| OrmError::conversion(format!("cannot read array column {index}")).with_source(e))?;
            match column {
                None => Value::Null,
                Some(items) => Value::Array(
                    items
                        .into_iter()
                        .map(|item| item.map_or(Value::Null, $wrap))
                        .collect(),
                ),
            }
        }};
    }

    let value = if *element == Type::BOOL {
        read_elements!(bool, Value::Bool)
    } else if *element == Type::INT2 {
        read_elements!(i16, |n| Value::Int(i64::from(n)))
    } else if *element == Type::INT4 {
        read_elements!(i32, |n| Value::Int(i64::from(n)))
    } else if *element == Type::INT8 {
        read_elements!(i64, Value::Int)
    } else if *element == Type::FLOAT4 {
        read_elements!(f32, |n| Value::Real(f64::from(n)))
    } else if *element == Type::FLOAT8 {
        read_elements!(f64, Value::Real)
    } else if *element == Type::UUID {
        read_elements!(uuid::Uuid, Value::Uuid)
    } else {
        read_elements!(String, Value::Text)
    };
    Ok(value)
}

/// Reads an optional typed column, turning a type/decode error into an `OrmError`.
fn get_opt<'a, T>(row: &'a tokio_postgres::Row, index: usize) -> crate::Result<Option<T>>
where
    T: tokio_postgres::types::FromSql<'a>,
{
    row.try_get::<_, Option<T>>(index)
        .map_err(|e| OrmError::conversion(format!("cannot read column {index}")).with_source(e))
}

/// Binds a [`Value`] as a PostgreSQL parameter.
///
/// The serialized form adapts to the target column type so the ORM's single
/// `Value::Int(i64)` binds correctly to `SMALLINT`/`INTEGER`/`BIGINT` columns (and
/// `Value::Real(f64)` to `REAL`/`DOUBLE PRECISION`).
impl ToSql for Value {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, BoxError> {
        match self {
            Value::Null => Ok(IsNull::Yes),
            Value::Bool(b) => b.to_sql(ty, out),
            Value::Int(i) => {
                if *ty == Type::INT2 {
                    i16::try_from(*i)?.to_sql(ty, out)
                } else if *ty == Type::INT4 {
                    i32::try_from(*i)?.to_sql(ty, out)
                } else {
                    i.to_sql(ty, out)
                }
            }
            Value::Real(r) => {
                if *ty == Type::FLOAT4 {
                    (*r as f32).to_sql(ty, out)
                } else {
                    r.to_sql(ty, out)
                }
            }
            Value::Text(s) => s.to_sql(ty, out),
            Value::Blob(b) => b.to_sql(ty, out),
            Value::Timestamp(t) => t.to_sql(ty, out),
            Value::Json(j) => j.to_sql(ty, out),
            Value::Uuid(u) => u.to_sql(ty, out),
            // tokio-postgres's blanket `ToSql for Vec<T>` frames the array and
            // recurses into each element's `Value::to_sql` with the element type.
            Value::Array(items) => items.to_sql(ty, out),
        }
    }

    // Accept any target type; `to_sql` performs the variant-specific encoding and
    // surfaces a real mismatch as an error.
    fn accepts(_ty: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

/// A single connection pinned out of the pool for exclusive, sequential use.
///
/// The connection returns to the pool when the handle is dropped (deadpool manages
/// recycling). Used by the transaction API and migration runner.
pub(crate) struct PinnedPostgres {
    object: Mutex<Option<Object>>,
    statements: Arc<AtomicU64>,
}

impl PinnedPostgres {
    /// Takes the connection out for one operation.
    fn take(&self) -> crate::Result<Object> {
        self.object
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(|| OrmError::query("pinned connection is already in use"))
    }

    /// Returns the connection after an operation completes.
    fn put(&self, object: Object) {
        *self.object.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(object);
    }

    /// Runs a row-returning query on the pinned connection.
    pub(crate) async fn fetch_all(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<Vec<Row>> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let object = self.take()?;
        let result = query(&object, &sql, &params).await;
        self.put(object);
        result
    }

    /// Runs a non-row-returning statement on the pinned connection.
    pub(crate) async fn execute(
        &self,
        sql: String,
        params: Vec<Value>,
    ) -> crate::Result<ExecuteResult> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let object = self.take()?;
        let result = execute(&object, &sql, &params).await;
        self.put(object);
        result
    }

    /// Runs a batch of statements on the pinned connection.
    pub(crate) async fn execute_batch(&self, sql: String) -> crate::Result<()> {
        self.statements.fetch_add(1, Ordering::Relaxed);
        let object = self.take()?;
        let result = object
            .batch_execute(&sql)
            .await
            .map_err(|e| OrmError::query("statement batch failed").with_source(e));
        self.put(object);
        result
    }

    /// Rolls back the open transaction without awaiting, for use from `Drop`.
    ///
    /// PostgreSQL rollback is asynchronous, so this best-effort spawns the
    /// `ROLLBACK` onto the current runtime when one is present. The common paths
    /// (`Transaction::commit`/`rollback`) roll back through the async `execute`
    /// above; this only guards an un-committed handle dropped on, e.g., a panic.
    pub(crate) fn rollback_now(&self) {
        let Ok(object) = self.take() else { return };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    let _ = object.batch_execute("ROLLBACK").await;
                    // `object` drops here, returning the connection to the pool.
                });
            }
            // No runtime: drop the connection; deadpool reclaims it and the server
            // rolls back the abandoned transaction when the session resets.
            Err(_) => drop(object),
        }
    }
}
