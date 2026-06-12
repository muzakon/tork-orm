//! Tork ORM — a Tortoise-style async ORM for Rust, native to the Tork web framework.
//!
//! This is the facade crate: the single crate end users depend on. It re-exports
//! the runtime from `tork-orm-core` and the derive macros from `tork-orm-macros`.
//! Queries are expressed through typed column handles rather than raw strings, and
//! a dialect-agnostic core keeps the query model independent of any one database.
//!
//! # Example
//!
//! ```no_run
//! use tork_orm::prelude::*;
//!
//! #[derive(Debug, Clone, Model)]
//! #[table(name = "users")]
//! struct User {
//!     #[field(primary_key, auto)]
//!     id: i64,
//!     #[field(varchar(length = 50))]
//!     username: String,
//!     is_active: bool,
//! }
//!
//! # async fn run() -> tork_orm::Result<()> {
//! let db = Database::connect("sqlite://app.db", 4).await?;
//!
//! // Typed, filter-first queries that bind every value.
//! let active = User::query()
//!     .filter(User::is_active.eq(true))
//!     .order_by(User::id.desc())
//!     .limit(20)
//!     .all(&db)
//!     .await?;
//!
//! // Inserts return the stored row, including the generated id.
//! let created = User::create(
//!     &db,
//!     &User { id: 0, username: "alice".into(), is_active: true },
//! )
//! .await?;
//! # let _ = (active, created);
//! # Ok(())
//! # }
//! ```
//! # Type safety
//!
//! A column is typed on its Rust type, so comparing it against an incompatible
//! value is a compile error rather than a run-time failure:
//!
//! ```compile_fail
//! use tork_orm::prelude::*;
//!
//! #[derive(Model)]
//! #[table(name = "users")]
//! struct User {
//!     #[field(primary_key, auto)]
//!     id: i64,
//!     is_active: bool,
//! }
//!
//! // `is_active` is a bool column; comparing it to a string does not compile.
//! let _ = User::is_active.eq("not a bool");
//! ```
#![forbid(unsafe_code)]

pub use tork_orm_core::*;
pub use tork_orm_macros::*;

/// Database migrations: the schema builder, runner, and the `#[migration]` macro.
///
/// Bringing `tork_orm::migration::*` into scope pulls in everything needed to
/// write a migration, including the `#[migration]` attribute alongside the schema
/// types.
#[cfg(feature = "migrations")]
pub mod migration {
    pub use tork_orm_core::migration::*;
    pub use tork_orm_macros::migration;
}

/// Implementation details used by generated code. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "migrations")]
    pub use inventory;
}

/// Registers a model in the link-time registry so `migrate generate` can find it.
///
/// `#[derive(Model)]` emits a call to this macro. With the `migrations` feature it
/// submits a [`ModelSchemaEntry`]; without it the macro expands to nothing, so a
/// model compiles and links the same whether or not generate is in use.
#[cfg(feature = "migrations")]
#[macro_export]
macro_rules! register_model {
    ($ty:ty) => {
        $crate::__private::inventory::submit! {
            $crate::ModelSchemaEntry::new(
                <$ty as $crate::Model>::TABLE,
                || <$ty as $crate::Model>::table_schema(),
            )
        }
    };
}

/// No-op registration when migrations (and the registry) are disabled.
#[cfg(not(feature = "migrations"))]
#[macro_export]
macro_rules! register_model {
    ($ty:ty) => {};
}

/// Runs an async block inside a database transaction, boxing it automatically.
///
/// This is ergonomic sugar over [`Database::transaction`], which requires
/// `Box::pin` because the compiler cannot name the return type of an async
/// closure. The macro adds that boilerplate so the call site reads naturally.
///
/// # Examples
///
/// ```no_run
/// use tork_orm::prelude::*;
/// use tork_orm::transaction;
///
/// # async fn run(db: Database) -> tork_orm::Result<()> {
/// let rows = transaction!(db, |tx| async move {
///     tx.execute("INSERT INTO t VALUES (1)".into(), vec![]).await?;
///     Ok(1_usize)
/// }).await?;
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! transaction {
    ($db:expr, |$tx:ident| $body:expr) => {
        $db.transaction(|$tx| ::std::boxed::Box::pin($body))
    };
}

/// The common imports for working with the ORM.
///
/// Bringing `tork_orm::prelude::*` into scope pulls in the `Model`/`QueryResult`
/// derives and the `relations` attribute, the database handle and executor, the
/// query builder and column/expression types, the value and row types, and the
/// error type.
pub mod prelude {
    pub use crate::{
        abs, ceil, coalesce, concat, floor, func, length, lower, round, substr, substr_len, trim,
        upper, Assignment, BindValue, BoxFuture, Column,
        ColumnDef, ColumnDefault, Database, ErrorKind, Executor, Expr, ForeignKeyDef, FromRow, FromValue,
        IndexColumn, IndexDef, IsolationLevel, Json, Model, ModelHooks, OrderItem, OrmError, Preloaded, QuerySet,
        Relation, RelationKind, Result, Row, SqlType, Transaction, TransactionBuilder, Value,
    };
    // The derive and attribute macros (`Model`, `relations`).
    pub use tork_orm_macros::*;
}
