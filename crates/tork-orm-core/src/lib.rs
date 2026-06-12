//! Core runtime for the Tork ORM.
//!
//! This crate holds the backend-neutral pieces of the ORM: the [`Value`] type that
//! crosses the driver boundary, the owned [`Row`] returned from queries, the
//! [`dialect`] abstraction that makes SQL generation backend-specific, the database
//! [`driver`]s, and the [`Database`] handle and [`Executor`] trait used to run SQL.
//!
//! End users do not depend on this crate directly; they depend on `tork-orm`, which
//! re-exports this runtime together with the derive macros.
#![forbid(unsafe_code)]

use std::future::Future;
use std::pin::Pin;

pub mod dialect;
pub mod driver;
pub mod query;

pub mod preload;

#[cfg(feature = "migrations")]
pub mod migration;

#[cfg(feature = "migrations")]
pub mod registry;

#[cfg(feature = "tork")]
mod bridge;

mod database;
mod error;
mod executor;
mod index;
mod model;
mod relation;
mod row;
mod transaction;
mod value;

pub use database::Database;
pub use transaction::{IsolationLevel, Transaction, TransactionBuilder};
pub use dialect::SqlType;
pub use error::{ErrorKind, OrmError, Result};
pub use executor::Executor;
pub use index::{IndexColumn, IndexDef, NullsOrder};
#[cfg(feature = "migrations")]
pub use registry::{registered_models, ModelSchemaEntry, TableSchema};
pub use model::{ColumnDef, ColumnDefault, ForeignKeyDef, FromRow, Model, ModelHooks};
pub use preload::{Preloaded, Preloader};
pub use query::ast::{Cte, CteQuery, Join, JoinKind, OrderItem, SelectItem, SelectStatement, UnionStatement, WithClause};
pub use query::UnionQuery;
pub use query::column::{Column, IntoAssignExpr, IntoSqlValue, Numeric};
pub use query::expr::{
    AggFunc, BinaryOp, CaseWhen, Expr, ExprOver, LogicalOp, Window, WindowBound, WindowFrame,
    WindowFrameUnit,
};
pub use query::func::{
    abs, ceil, coalesce, concat, cume_dist, current_date, current_time, current_timestamp,
    dense_rank, first_value, floor, func, greatest,
    lag, lag_default, lag_offset, last_value, lead, lead_default, lead_offset, least, length,
    lower, ntile, nth_value, nullif, now, percent_rank, position, random_value, rank, replace,
    round, row_number, substr, substr_len, trim, upper,
};
#[cfg(feature = "postgres")]
pub use query::func::{
    age, array_aggregation, at_time_zone, bool_and, bool_or, date_trunc, json_aggregation,
    jsonb_aggregation, left,
    regex_match, regex_replace, repeat, reverse, right, split_part, string_aggregation,
    to_char,
};
pub use query::projection::{ExprTuple, IntoExpr, IntoSelectItem, Projection};
pub use query::write::{Assignment, DeleteStatement, InsertStatement, OnConflict, UpdateStatement};
pub use query::{Page, QuerySet};
pub use relation::{Relation, RelationKind};
pub use row::Row;
pub use value::{BindValue, FromValue, Value};

// Re-exports so models and queries can name PostgreSQL-specific column types without
// adding the underlying crates to their own dependencies.
pub use serde_json;
pub use uuid;

/// A JSON column value (PostgreSQL `jsonb`). An alias for [`serde_json::Value`].
pub type Json = serde_json::Value;

pub use driver::ExecuteResult;

/// A boxed, `Send` future borrowing for `'a`.
///
/// Used by the closure-based transaction API and the migration engine to store
/// async work behind trait objects.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
