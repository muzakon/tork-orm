//! Database migrations: revision-based schema changes with up/down.
//!
//! A migration describes a schema change and how to undo it. The
//! [`SchemaManager`] handed to its `up`/`down` offers a fluent, backend-neutral DDL
//! builder; the [`render`] layer turns that into SQL for the active dialect. A
//! migration's rendered DDL is also what a later runner hashes into a stable
//! checksum and applies in revision order.
//!
//! This module is the migration engine; the command-line tooling that scaffolds and
//! drives migrations is separate.
//!
//! # Examples
//!
//! ```
//! use tork_orm_core::dialect::SqliteDialect;
//! use tork_orm_core::migration::{Column, ForeignKey, ForeignKeyAction, SchemaManager};
//!
//! # async fn run() -> tork_orm_core::Result<()> {
//! let dialect = SqliteDialect::new();
//! let mut schema = SchemaManager::collect(&dialect);
//! schema
//!     .create_table("posts")
//!     .column(Column::new("id").bigint().primary_key().auto_increment())
//!     .column(Column::new("user_id").bigint().not_null())
//!     .column(Column::new("title").varchar(255).not_null())
//!     .foreign_key(
//!         ForeignKey::new()
//!             .from("posts", "user_id")
//!             .to("users", "id")
//!             .on_delete(ForeignKeyAction::Cascade),
//!     )
//!     .execute()
//!     .await?;
//! assert!(schema.into_collected()[0].contains("FOREIGN KEY"));
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::pin::Pin;

pub mod ddl;
pub mod render;
pub mod schema;

/// A boxed, `Send` future borrowing for `'a`.
///
/// The migration engine stores async work behind trait objects (so migrations can
/// be collected into a set); this is the future type those methods return.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub use ddl::{
    AlterAction, AlterTable, ColumnSpec, DefaultValue, ForeignKeyAction, ForeignKeySpec, IndexSpec,
    TableDef,
};
pub use schema::{Column, CreateTable, DropTable, DynExecutor, ForeignKey, SchemaManager};

/// The common imports for writing a migration.
pub mod prelude {
    pub use super::{Column, ForeignKey, ForeignKeyAction, SchemaManager};
    pub use crate::dialect::DialectKind;
    pub use crate::Result;
}
