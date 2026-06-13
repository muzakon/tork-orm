//! The schema manager and its fluent builders.
//!
//! A [`SchemaManager`] is handed to a migration's `up`/`down`. Its builders assemble
//! DDL and either run it against the database (execute mode) or buffer the rendered
//! SQL without touching the database (collect mode). Collect mode is what lets a
//! migration's DDL be previewed and hashed into a stable checksum.

use crate::dialect::{Dialect, DialectKind, SqlType};
use crate::driver::ExecuteResult;
use crate::executor::Executor;
use crate::index::{IndexColumn, IndexDef};
use crate::query::expr::Expr;
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
    /// Run each statement against the database.
    Execute(&'e dyn DynExecutor),
    /// Buffer rendered statements without running them.
    Collect(Vec<String>),
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
    pub(crate) fn executing(executor: &'e dyn DynExecutor) -> Self {
        Self {
            dialect: executor.dialect(),
            mode: Mode::Execute(executor),
        }
    }

    /// Creates a manager that buffers rendered SQL for `dialect` without running it.
    pub fn collect(dialect: &'e dyn Dialect) -> Self {
        Self {
            mode: Mode::Collect(Vec::new()),
            dialect,
        }
    }

    /// Consumes a collect-mode manager, returning the rendered statements.
    ///
    /// Returns an empty vector for an execute-mode manager.
    pub fn into_collected(self) -> Vec<String> {
        match self.mode {
            Mode::Collect(buffer) => buffer,
            Mode::Execute(_) => Vec::new(),
        }
    }

    /// Sends rendered statements to the database, or buffers them in collect mode.
    ///
    /// Takes `&mut self` (rather than `&self` with interior mutability) so the
    /// future this is awaited in only needs to be `Send`, never `Sync`.
    async fn dispatch(&mut self, statements: Vec<String>) -> crate::Result<()> {
        let executor = match &mut self.mode {
            Mode::Execute(executor) => *executor,
            Mode::Collect(buffer) => {
                buffer.extend(statements);
                return Ok(());
            }
        };
        for statement in statements {
            executor.execute(statement, Vec::new()).await?;
        }
        Ok(())
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

    /// Begins a `CREATE INDEX` named `name`.
    ///
    /// Set the table with [`CreateIndex::on_table`] and the columns with
    /// [`CreateIndex::column`] / [`CreateIndex::columns`] before calling
    /// [`CreateIndex::execute`].
    pub fn create_index(&mut self, name: &str) -> CreateIndex<'_, 'e> {
        CreateIndex {
            schema: self,
            table: String::new(),
            def: IndexDef::new(name),
            if_not_exists: false,
        }
    }

    /// Begins a `DROP INDEX` for the index named `name`.
    pub fn drop_index(&mut self, name: &str) -> DropIndex<'_, 'e> {
        DropIndex {
            schema: self,
            name: name.to_string(),
            if_exists: false,
        }
    }

    /// Begins a `CREATE TRIGGER` named `name`.
    ///
    /// A thin convenience that renders the trigger header (`CREATE TRIGGER <name>
    /// <timing> <event> ON <table> [FOR EACH ROW]`) followed by a raw action body.
    /// Trigger bodies are dialect-specific (PostgreSQL `EXECUTE FUNCTION f()`,
    /// SQLite `BEGIN ... END`), so the body is the caller's responsibility — as is
    /// any function the trigger calls, created via [`raw`](Self::raw).
    pub fn create_trigger(&mut self, name: &str) -> CreateTrigger<'_, 'e> {
        CreateTrigger {
            schema: self,
            name: name.to_string(),
            timing: TriggerTiming::Before,
            event: TriggerEvent::Insert,
            table: String::new(),
            for_each_row: false,
            body: String::new(),
        }
    }

    /// Begins a `DROP TRIGGER` for the trigger named `name`.
    pub fn drop_trigger(&mut self, name: &str) -> DropTrigger<'_, 'e> {
        DropTrigger {
            schema: self,
            name: name.to_string(),
            table: None,
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

    /// Adds a table-level `CHECK (...)` constraint. The expression is rendered
    /// verbatim, so write it in SQL: `.check("price_cents >= 0")`.
    pub fn check(mut self, expression: impl Into<String>) -> Self {
        self.def.checks.push(expression.into());
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
        let statements = render::create_table(self.schema.dialect, &self.def)?;
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

/// When a trigger fires relative to the row event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTiming {
    /// `BEFORE` the event.
    Before,
    /// `AFTER` the event.
    After,
}

impl TriggerTiming {
    fn as_sql(self) -> &'static str {
        match self {
            TriggerTiming::Before => "BEFORE",
            TriggerTiming::After => "AFTER",
        }
    }
}

/// The row event a trigger fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEvent {
    /// `INSERT`.
    Insert,
    /// `UPDATE`.
    Update,
    /// `DELETE`.
    Delete,
}

impl TriggerEvent {
    fn as_sql(self) -> &'static str {
        match self {
            TriggerEvent::Insert => "INSERT",
            TriggerEvent::Update => "UPDATE",
            TriggerEvent::Delete => "DELETE",
        }
    }
}

/// A `CREATE TRIGGER` builder.
///
/// # Examples
///
/// ```
/// use tork_orm_core::dialect::PostgresDialect;
/// use tork_orm_core::migration::{SchemaManager, TriggerEvent};
///
/// # async fn run() -> tork_orm_core::Result<()> {
/// let dialect = PostgresDialect::new();
/// let mut schema = SchemaManager::collect(&dialect);
/// schema
///     .create_trigger("set_updated_at")
///     .before()
///     .event(TriggerEvent::Update)
///     .on("users")
///     .for_each_row()
///     .body("EXECUTE FUNCTION touch_updated_at()")
///     .execute()
///     .await?;
/// assert_eq!(
///     schema.into_collected()[0],
///     "CREATE TRIGGER \"set_updated_at\" BEFORE UPDATE ON \"users\" \
///FOR EACH ROW EXECUTE FUNCTION touch_updated_at()"
/// );
/// # Ok(())
/// # }
/// ```
pub struct CreateTrigger<'a, 'e> {
    schema: &'a mut SchemaManager<'e>,
    name: String,
    timing: TriggerTiming,
    event: TriggerEvent,
    table: String,
    for_each_row: bool,
    body: String,
}

impl CreateTrigger<'_, '_> {
    /// Sets the firing time.
    pub fn timing(mut self, timing: TriggerTiming) -> Self {
        self.timing = timing;
        self
    }

    /// Sugar for `BEFORE`.
    pub fn before(self) -> Self {
        self.timing(TriggerTiming::Before)
    }

    /// Sugar for `AFTER`.
    pub fn after(self) -> Self {
        self.timing(TriggerTiming::After)
    }

    /// Sets the row event.
    pub fn event(mut self, event: TriggerEvent) -> Self {
        self.event = event;
        self
    }

    /// Sets the table the trigger is attached to.
    pub fn on(mut self, table: &str) -> Self {
        self.table = table.to_string();
        self
    }

    /// Adds `FOR EACH ROW`.
    pub fn for_each_row(mut self) -> Self {
        self.for_each_row = true;
        self
    }

    /// Sets the raw, dialect-specific action body (e.g. PostgreSQL
    /// `EXECUTE FUNCTION f()` or SQLite `BEGIN ... END`).
    pub fn body(mut self, body: &str) -> Self {
        self.body = body.to_string();
        self
    }

    /// Renders and applies the `CREATE TRIGGER`.
    pub async fn execute(self) -> crate::Result<()> {
        let mut sql = String::from("CREATE TRIGGER ");
        self.schema.dialect.quote_identifier(&self.name, &mut sql);
        sql.push(' ');
        sql.push_str(self.timing.as_sql());
        sql.push(' ');
        sql.push_str(self.event.as_sql());
        sql.push_str(" ON ");
        self.schema.dialect.quote_identifier(&self.table, &mut sql);
        if self.for_each_row {
            sql.push_str(" FOR EACH ROW");
        }
        if !self.body.is_empty() {
            sql.push(' ');
            sql.push_str(&self.body);
        }
        self.schema.dispatch(vec![sql]).await
    }
}

/// A `DROP TRIGGER` builder.
pub struct DropTrigger<'a, 'e> {
    schema: &'a mut SchemaManager<'e>,
    name: String,
    table: Option<String>,
    if_exists: bool,
}

impl DropTrigger<'_, '_> {
    /// Adds `IF EXISTS`.
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }

    /// Names the table the trigger is on (required by PostgreSQL: `DROP TRIGGER
    /// name ON table`; ignored by SQLite).
    pub fn on(mut self, table: &str) -> Self {
        self.table = Some(table.to_string());
        self
    }

    /// Renders and applies the `DROP TRIGGER`.
    pub async fn execute(self) -> crate::Result<()> {
        let mut sql = String::from("DROP TRIGGER ");
        if self.if_exists {
            sql.push_str("IF EXISTS ");
        }
        self.schema.dialect.quote_identifier(&self.name, &mut sql);
        // PostgreSQL requires the table; SQLite does not accept it.
        if self.schema.dialect.kind() == DialectKind::Postgres {
            if let Some(table) = &self.table {
                sql.push_str(" ON ");
                self.schema.dialect.quote_identifier(table, &mut sql);
            }
        }
        self.schema.dispatch(vec![sql]).await
    }
}

/// A `CREATE INDEX` builder.
///
/// # Examples
///
/// ```
/// use tork_orm_core::dialect::SqliteDialect;
/// use tork_orm_core::migration::{IndexColumn, SchemaManager};
///
/// # async fn run() -> tork_orm_core::Result<()> {
/// let dialect = SqliteDialect::new();
/// let mut schema = SchemaManager::collect(&dialect);
/// schema
///     .create_index("idx_posts_user_created")
///     .on_table("posts")
///     .unique()
///     .columns([IndexColumn::new("user_id"), IndexColumn::new("created_at").desc()])
///     .execute()
///     .await?;
/// let sql = schema.into_collected();
/// assert_eq!(
///     sql[0],
///     "CREATE UNIQUE INDEX \"idx_posts_user_created\" ON \"posts\" \
///      (\"user_id\", \"created_at\" DESC)"
/// );
/// # Ok(())
/// # }
/// ```
pub struct CreateIndex<'a, 'e> {
    schema: &'a mut SchemaManager<'e>,
    table: String,
    def: IndexDef,
    if_not_exists: bool,
}

impl CreateIndex<'_, '_> {
    /// Sets the table the index is on.
    pub fn on_table(mut self, table: &str) -> Self {
        self.table = table.to_string();
        self
    }

    /// Marks the index `UNIQUE`.
    pub fn unique(mut self) -> Self {
        self.def.unique = true;
        self
    }

    /// Adds a single column.
    pub fn column(mut self, column: IndexColumn) -> Self {
        self.def.columns.push(column);
        self
    }

    /// Adds several columns at once.
    pub fn columns(mut self, columns: impl IntoIterator<Item = IndexColumn>) -> Self {
        self.def.columns.extend(columns);
        self
    }

    /// Sets the index method (`USING`); supported only on backends that have one.
    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.def.method = Some(method.into());
        self
    }

    /// Adds covering columns (`INCLUDE`); supported only on backends that have them.
    pub fn include<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.def
            .include
            .extend(columns.into_iter().map(Into::into));
        self
    }

    /// Restricts the index to rows matching `predicate` (a partial index).
    pub fn where_(mut self, predicate: Expr) -> Self {
        self.def.predicate = Some(predicate);
        self
    }

    /// Adds `IF NOT EXISTS`.
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }

    /// Renders and applies the statement.
    pub async fn execute(self) -> crate::Result<()> {
        let statement =
            render::create_index(self.schema.dialect, &self.table, &self.def, self.if_not_exists)?;
        self.schema.dispatch(vec![statement]).await
    }
}

/// A `DROP INDEX` builder.
pub struct DropIndex<'a, 'e> {
    schema: &'a mut SchemaManager<'e>,
    name: String,
    if_exists: bool,
}

impl DropIndex<'_, '_> {
    /// Adds `IF EXISTS`.
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }

    /// Renders and applies the statement.
    pub async fn execute(self) -> crate::Result<()> {
        let statement = render::drop_index(self.schema.dialect, &self.name, self.if_exists);
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
        debug_assert!(length > 0, "varchar length must be > 0");
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

    /// Sets the type to a JSON document (`JSONB` on PostgreSQL, `JSON` on MySQL,
    /// `TEXT` on SQLite).
    pub fn json(mut self) -> Self {
        self.spec.ty = SqlType::Json;
        self
    }

    /// Sets the type to a UUID (`UUID` on PostgreSQL, `CHAR(36)` on MySQL,
    /// `TEXT` on SQLite).
    pub fn uuid(mut self) -> Self {
        self.spec.ty = SqlType::Uuid;
        self
    }

    /// Sets the type to an enum constrained to `variants`.
    ///
    /// Rendered as a native `ENUM(...)` on MySQL and as a text column with a
    /// `CHECK (... IN (...))` constraint elsewhere. Both arguments are `'static`,
    /// so the usual call passes string literals:
    /// `Column::new("status").enum_type("status", &["active", "inactive"])`.
    pub fn enum_type(
        mut self,
        name: &'static str,
        variants: &'static [&'static str],
    ) -> Self {
        self.spec.ty = SqlType::Enum { name, variants };
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
