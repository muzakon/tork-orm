//! The schema manager and its fluent builders.
//!
//! A [`SchemaManager`] is handed to a migration's `up`/`down`. Its builders assemble
//! DDL and either run it against the database (execute mode) or buffer the rendered
//! SQL without touching the database (collect mode). Collect mode is what lets a
//! migration's DDL be previewed and hashed into a stable checksum.

use std::cell::RefCell;

use crate::dialect::{Dialect, DialectKind, SqlType};
use crate::driver::ExecuteResult;
use crate::executor::Executor;
use crate::row::Row;
use crate::value::Value;

use super::ddl::{ColumnSpec, DefaultValue, ForeignKeyAction, ForeignKeySpec, TableDef};
use super::{render, BoxFuture};

/// An object-safe view of an [`Executor`], so [`SchemaManager`] need not be generic
/// over the executor type. Mirrors the pattern used for preloading.
pub trait DynExecutor: Sync {
    /// Returns the dialect to render for.
    fn dialect(&self) -> &dyn Dialect;
    /// Runs a statement that returns no rows.
    fn execute<'a>(
        &'a self,
        sql: String,
        params: Vec<Value>,
    ) -> BoxFuture<'a, crate::Result<ExecuteResult>>;
    /// Runs a row-returning query.
    fn fetch_all<'a>(
        &'a self,
        sql: String,
        params: Vec<Value>,
    ) -> BoxFuture<'a, crate::Result<Vec<Row>>>;
}

impl<E: Executor + Sync> DynExecutor for E {
    fn dialect(&self) -> &dyn Dialect {
        Executor::dialect(self)
    }

    fn execute<'a>(
        &'a self,
        sql: String,
        params: Vec<Value>,
    ) -> BoxFuture<'a, crate::Result<ExecuteResult>> {
        Box::pin(Executor::execute(self, sql, params))
    }

    fn fetch_all<'a>(
        &'a self,
        sql: String,
        params: Vec<Value>,
    ) -> BoxFuture<'a, crate::Result<Vec<Row>>> {
        Box::pin(Executor::fetch_all(self, sql, params))
    }
}

/// Where a [`SchemaManager`]'s rendered DDL goes.
enum Mode<'e> {
    // The migration runner drives execute mode; it lands in the next commit.
    /// Run each statement against the database.
    #[allow(dead_code)]
    Execute(&'e dyn DynExecutor),
    /// Buffer rendered statements without running them.
    Collect(RefCell<Vec<String>>),
}

/// Builds and applies schema changes for one migration.
///
/// # Examples
///
/// ```
/// use tork_orm_core::dialect::SqliteDialect;
/// use tork_orm_core::migration::{Column, SchemaManager};
///
/// # async fn run() -> tork_orm_core::Result<()> {
/// let dialect = SqliteDialect::new();
/// let mut schema = SchemaManager::collect(&dialect);
/// schema
///     .create_table("users")
///     .column(Column::new("id").bigint().primary_key().auto_increment())
///     .execute()
///     .await?;
/// let sql = schema.into_collected();
/// assert_eq!(sql[0], "CREATE TABLE \"users\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT)");
/// # Ok(())
/// # }
/// ```
pub struct SchemaManager<'e> {
    mode: Mode<'e>,
    dialect: &'e dyn Dialect,
}

impl<'e> SchemaManager<'e> {
    /// Creates a manager that runs DDL against `executor`.
    // Used by the migration runner (next commit).
    #[allow(dead_code)]
    pub(crate) fn executing(executor: &'e dyn DynExecutor) -> Self {
        Self {
            dialect: executor.dialect(),
            mode: Mode::Execute(executor),
        }
    }

    /// Creates a manager that buffers rendered SQL for `dialect` without running it.
    pub fn collect(dialect: &'e dyn Dialect) -> Self {
        Self {
            mode: Mode::Collect(RefCell::new(Vec::new())),
            dialect,
        }
    }

    /// Consumes a collect-mode manager, returning the rendered statements.
    ///
    /// Returns an empty vector for an execute-mode manager.
    pub fn into_collected(self) -> Vec<String> {
        match self.mode {
            Mode::Collect(buffer) => buffer.into_inner(),
            Mode::Execute(_) => Vec::new(),
        }
    }

    /// Sends rendered statements to the database, or buffers them in collect mode.
    async fn dispatch(&self, statements: Vec<String>) -> crate::Result<()> {
        match &self.mode {
            Mode::Execute(executor) => {
                for statement in statements {
                    executor.execute(statement, Vec::new()).await?;
                }
                Ok(())
            }
            Mode::Collect(buffer) => {
                buffer.borrow_mut().extend(statements);
                Ok(())
            }
        }
    }

    /// Begins a `CREATE TABLE`.
    pub fn create_table(&mut self, name: &str) -> CreateTable<'_, 'e> {
        CreateTable {
            schema: self,
            def: TableDef::new(name),
        }
    }

    /// Begins a `DROP TABLE`.
    pub fn drop_table(&mut self, name: &str) -> DropTable<'_, 'e> {
        DropTable {
            schema: self,
            name: name.to_string(),
            if_exists: false,
        }
    }

    /// Runs verbatim SQL. The caller owns its correctness and escaping.
    pub async fn raw(&mut self, sql: &str) -> crate::Result<()> {
        self.dispatch(vec![sql.to_string()]).await
    }

    /// Runs verbatim SQL only when the target dialect matches `kind`.
    ///
    /// On a non-matching dialect it contributes nothing, in both execute and
    /// collect modes, so checksums stay stable across the dialects a migration
    /// does not target.
    pub async fn raw_for(&mut self, kind: DialectKind, sql: &str) -> crate::Result<()> {
        if self.dialect.kind() == kind {
            self.dispatch(vec![sql.to_string()]).await
        } else {
            Ok(())
        }
    }
}

/// A `CREATE TABLE` builder.
pub struct CreateTable<'a, 'e> {
    schema: &'a mut SchemaManager<'e>,
    def: TableDef,
}

impl CreateTable<'_, '_> {
    /// Adds `IF NOT EXISTS`.
    pub fn if_not_exists(mut self) -> Self {
        self.def.if_not_exists = true;
        self
    }

    /// Adds a column.
    pub fn column(mut self, column: Column) -> Self {
        self.def.columns.push(column.into_spec());
        self
    }

    /// Declares a composite primary key over the named columns.
    pub fn primary_key(mut self, columns: &[&str]) -> Self {
        self.def.primary_key = columns.iter().map(|c| c.to_string()).collect();
        self
    }

    /// Adds a foreign key constraint.
    pub fn foreign_key(mut self, foreign_key: ForeignKey) -> Self {
        self.def.foreign_keys.push(foreign_key.into_spec());
        self
    }

    /// Adds `created_at` and `updated_at` timestamp columns defaulting to the
    /// current time.
    pub fn timestamps(mut self) -> Self {
        self.def.columns.push(timestamp_column("created_at"));
        self.def.columns.push(timestamp_column("updated_at"));
        self
    }

    /// Renders and applies the statement.
    pub async fn execute(self) -> crate::Result<()> {
        let statements = render::create_table(self.schema.dialect, &self.def);
        self.schema.dispatch(statements).await
    }
}

/// A `DROP TABLE` builder.
pub struct DropTable<'a, 'e> {
    schema: &'a mut SchemaManager<'e>,
    name: String,
    if_exists: bool,
}

impl DropTable<'_, '_> {
    /// Adds `IF EXISTS`.
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }

    /// Renders and applies the statement.
    pub async fn execute(self) -> crate::Result<()> {
        let statement = render::drop_table(self.schema.dialect, &self.name, self.if_exists);
        self.schema.dispatch(vec![statement]).await
    }
}

/// Builds a `created_at`/`updated_at` style timestamp column.
fn timestamp_column(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        ty: SqlType::Timestamp,
        nullable: false,
        primary_key: false,
        auto_increment: false,
        unique: false,
        default: Some(DefaultValue::CurrentTimestamp),
    }
}

/// A column definition for a migration.
///
/// Distinct from the query-side `Column<M, T>`; this one builds DDL. Migration
/// files bring it in with `use tork_orm::migration::*`.
pub struct Column {
    spec: ColumnSpec,
}

impl Column {
    /// Starts a nullable column named `name` (set a type before using it).
    pub fn new(name: impl Into<String>) -> Self {
        let mut spec = ColumnSpec::new(name, SqlType::Text);
        spec.nullable = true;
        Self { spec }
    }

    /// Sets the type to a 32-bit integer.
    pub fn integer(mut self) -> Self {
        self.spec.ty = SqlType::Integer;
        self
    }

    /// Sets the type to a 64-bit integer.
    pub fn bigint(mut self) -> Self {
        self.spec.ty = SqlType::BigInt;
        self
    }

    /// Sets the type to bounded text of at most `length`.
    pub fn varchar(mut self, length: u32) -> Self {
        self.spec.ty = SqlType::Varchar(length);
        self
    }

    /// Sets the type to unbounded text.
    pub fn text(mut self) -> Self {
        self.spec.ty = SqlType::Text;
        self
    }

    /// Sets the type to a boolean.
    pub fn boolean(mut self) -> Self {
        self.spec.ty = SqlType::Boolean;
        self
    }

    /// Sets the type to a floating point number.
    pub fn real(mut self) -> Self {
        self.spec.ty = SqlType::Real;
        self
    }

    /// Sets the type to a timestamp.
    pub fn timestamp(mut self) -> Self {
        self.spec.ty = SqlType::Timestamp;
        self
    }

    /// Sets the type to a binary blob.
    pub fn blob(mut self) -> Self {
        self.spec.ty = SqlType::Blob;
        self
    }

    /// Marks the column `NOT NULL`.
    pub fn not_null(mut self) -> Self {
        self.spec.nullable = false;
        self
    }

    /// Marks the column nullable.
    pub fn nullable(mut self) -> Self {
        self.spec.nullable = true;
        self
    }

    /// Marks the column (part of) the primary key.
    pub fn primary_key(mut self) -> Self {
        self.spec.primary_key = true;
        self
    }

    /// Marks the column auto-incrementing (with `primary_key`).
    pub fn auto_increment(mut self) -> Self {
        self.spec.auto_increment = true;
        self
    }

    /// Adds a `UNIQUE` constraint.
    pub fn unique(mut self) -> Self {
        self.spec.unique = true;
        self
    }

    /// Sets a default value.
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.spec.default = Some(value.into());
        self
    }

    /// Consumes the builder, returning the column spec.
    fn into_spec(self) -> ColumnSpec {
        self.spec
    }
}

/// A foreign key constraint builder.
///
/// # Examples
///
/// ```
/// use tork_orm_core::migration::{ForeignKey, ForeignKeyAction};
///
/// let fk = ForeignKey::new()
///     .from("posts", "user_id")
///     .to("users", "id")
///     .on_delete(ForeignKeyAction::Cascade);
/// # let _ = fk;
/// ```
pub struct ForeignKey {
    spec: ForeignKeySpec,
}

impl ForeignKey {
    /// Starts an empty foreign key.
    pub fn new() -> Self {
        Self {
            spec: ForeignKeySpec {
                columns: Vec::new(),
                ref_table: String::new(),
                ref_columns: Vec::new(),
                on_delete: ForeignKeyAction::NoAction,
                on_update: ForeignKeyAction::NoAction,
            },
        }
    }

    /// Adds a local column. The `table` is the table being created; it is accepted
    /// for readability and is otherwise implied.
    pub fn from(mut self, table: &str, column: &str) -> Self {
        let _ = table;
        self.spec.columns.push(column.to_string());
        self
    }

    /// Adds the referenced table and column.
    pub fn to(mut self, table: &str, column: &str) -> Self {
        self.spec.ref_table = table.to_string();
        self.spec.ref_columns.push(column.to_string());
        self
    }

    /// Sets the `ON DELETE` action.
    pub fn on_delete(mut self, action: ForeignKeyAction) -> Self {
        self.spec.on_delete = action;
        self
    }

    /// Sets the `ON UPDATE` action.
    pub fn on_update(mut self, action: ForeignKeyAction) -> Self {
        self.spec.on_update = action;
        self
    }

    /// Consumes the builder, returning the spec.
    fn into_spec(self) -> ForeignKeySpec {
        self.spec
    }
}

impl Default for ForeignKey {
    fn default() -> Self {
        Self::new()
    }
}
