//! The [`Model`] trait and the metadata a `#[derive(Model)]` produces.
//!
//! A model is a Rust struct that mirrors a database table. The derive generates
//! the table name, a description of every column, and the conversions between a
//! row and an instance. The column metadata is intentionally richer than query
//! execution needs today (it records SQL types and foreign keys) so that a later
//! migrations phase can build on it.

use crate::dialect::{render_insert, render_select, SqlType};
use crate::error::OrmError;
use crate::executor::Executor;
use crate::query::QuerySet;
use crate::query::ast::{SelectItem, SelectStatement};
use crate::query::expr::{BinaryOp, Expr};
use crate::query::write::{Assignment, DeleteStatement, InsertStatement, OnConflict, UpdateStatement};
use crate::row::Row;
use crate::value::Value;

/// The action a foreign key takes when the referenced row changes.
///
/// Lives here (rather than in the feature-gated `migration` module) so it can be
/// recorded on a [`ForeignKeyDef`] that is always compiled; `migration` re-exports
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForeignKeyAction {
    /// `NO ACTION` (the default).
    #[default]
    NoAction,
    /// `RESTRICT`.
    Restrict,
    /// `CASCADE`.
    Cascade,
    /// `SET NULL`.
    SetNull,
    /// `SET DEFAULT`.
    SetDefault,
}

impl ForeignKeyAction {
    /// Returns the SQL keyword, or `None` for the default `NO ACTION`.
    pub fn as_sql(self) -> Option<&'static str> {
        match self {
            ForeignKeyAction::NoAction => None,
            ForeignKeyAction::Restrict => Some("RESTRICT"),
            ForeignKeyAction::Cascade => Some("CASCADE"),
            ForeignKeyAction::SetNull => Some("SET NULL"),
            ForeignKeyAction::SetDefault => Some("SET DEFAULT"),
        }
    }
}

/// A foreign key reference recorded on a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignKeyDef {
    /// The referenced table.
    pub table: &'static str,
    /// The referenced column in that table.
    pub column: &'static str,
    /// The `ON DELETE` action.
    pub on_delete: ForeignKeyAction,
    /// The `ON UPDATE` action.
    pub on_update: ForeignKeyAction,
}

/// A database-side default value declared on a model column.
///
/// A column with a default is omitted from `INSERT` (the database fills it) and the
/// default is emitted in the column's DDL. `Copy`, so it fits in [`ColumnDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnDefault {
    /// `DEFAULT CURRENT_TIMESTAMP` — the database fills the insert time.
    CurrentTimestamp,
    /// A generated UUID (PostgreSQL `DEFAULT gen_random_uuid()`).
    Uuid,
    /// Verbatim SQL default, the caller's responsibility.
    Raw(&'static str),
}

/// The compile-time description of a single model column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnDef {
    /// The column name in the database.
    pub name: &'static str,
    /// The abstract SQL type of the column.
    pub sql_type: SqlType,
    /// Whether the column is (part of) the primary key.
    pub primary_key: bool,
    /// Whether the database assigns the value automatically (auto-increment).
    pub auto: bool,
    /// Whether the column accepts `NULL` (the Rust field is an `Option`).
    pub nullable: bool,
    /// A foreign key reference, if the column points at another table.
    pub foreign_key: Option<ForeignKeyDef>,
    /// A database-side default; when set, the column is omitted from `INSERT`.
    pub default: Option<ColumnDefault>,
}

/// Builds an instance from a result row.
///
/// Implemented by `#[derive(Model)]` for full models and by
/// `#[derive(QueryResult)]` for projection DTOs. Mapping is by column name, so the
/// order of selected columns does not have to match the field order.
pub trait FromRow: Sized {
    /// Reads each field from its like-named column in `row`.
    fn from_row(row: &Row) -> crate::Result<Self>;
}

/// Lifecycle hooks fired by the instance write methods.
///
/// Every model gets a no-op implementation automatically. To add behavior, mark
/// the model `#[table(hooks)]` (which suppresses the generated empty impl) and
/// write your own:
///
/// ```
/// use tork_orm_core::{Executor, ModelHooks};
/// # #[derive(Clone)] struct User { updated_at: i64 }
/// impl ModelHooks for User {
///     fn before_save(&mut self) {
///         self.updated_at += 1; // e.g. set a timestamp
///     }
///     async fn after_create<E: Executor + Send + Sync>(&self, _db: &E) -> tork_orm_core::Result<()> {
///         // side effect: audit log, emit an event, …
///         Ok(())
///     }
/// }
/// ```
///
/// Hooks fire only for the instance methods (`create`, `save`, `delete`, `upsert`,
/// `upsert_on`); bulk operations (`bulk_create`, `QuerySet::update`/`delete`) bypass
/// them. An `after_*` hook returning `Err` aborts the operation — inside a
/// `transaction` that rolls the change back; outside one, the row has already been
/// written.
pub trait ModelHooks {
    /// Runs before an insert, able to mutate the row (set timestamps, a
    /// client-side UUID, …).
    fn before_create(&mut self) {}

    /// Runs after a successful insert, on the stored row (with its DB-generated
    /// values). Async; an error aborts the operation.
    fn after_create<E: Executor + Send + Sync>(
        &self,
        executor: &E,
    ) -> impl std::future::Future<Output = crate::Result<()>> + Send {
        let _ = executor;
        async { Ok(()) }
    }

    /// Runs before a `save`, able to mutate the row (e.g. bump `updated_at`).
    fn before_save(&mut self) {}

    /// Runs after a successful `save`. Async; an error aborts the operation.
    fn after_save<E: Executor + Send + Sync>(
        &self,
        executor: &E,
    ) -> impl std::future::Future<Output = crate::Result<()>> + Send {
        let _ = executor;
        async { Ok(()) }
    }

    /// Runs before a `delete`.
    fn before_delete(&self) {}

    /// Runs after a successful `delete`. Async; an error aborts the operation.
    fn after_delete<E: Executor + Send + Sync>(
        &self,
        executor: &E,
    ) -> impl std::future::Future<Output = crate::Result<()>> + Send {
        let _ = executor;
        async { Ok(()) }
    }
}

/// A struct that maps to a database table.
///
/// # Examples
///
/// ```
/// use tork_orm_core::{ColumnDef, Model};
///
/// fn primary_key<M: Model>() -> &'static str {
///     M::PRIMARY_KEY
/// }
/// ```
pub trait Model: FromRow + ModelHooks + Clone + Send + Sync + 'static {
    /// The table this model maps to.
    const TABLE: &'static str;
    /// The description of every column, in declaration order.
    const COLUMNS: &'static [ColumnDef];
    /// The name of the primary key column.
    const PRIMARY_KEY: &'static str;

    /// The column auto-set to the current time on every [`save`](Self::save), if
    /// the model declares a `#[field(updated_at)]` column. `None` otherwise.
    const UPDATED_AT: Option<&'static str> = None;

    /// The soft-delete timestamp column, if the model declares a
    /// `#[field(deleted_at)]` column. `None` otherwise.
    ///
    /// When set, [`delete`](Self::delete) and `QuerySet::delete` stamp this column
    /// instead of removing the row, and `Model::query` excludes rows where it is
    /// non-null by default.
    const DELETED_AT: Option<&'static str> = None;

    /// Table-level `CHECK (...)` constraint expressions declared with
    /// `#[table(check = "...")]`. Empty by default.
    const CHECKS: &'static [&'static str] = &[];

    /// The optimistic-lock version column, if the model declares a
    /// `#[field(version)]` column. `None` otherwise.
    ///
    /// When set, [`save`](Self::save) only updates the row when its version still
    /// matches, bumps the version, and returns an [`ErrorKind::Conflict`] error if
    /// the row was changed concurrently.
    const VERSION: Option<&'static str> = None;

    /// Returns the column-name and value pairs to write on insert.
    ///
    /// Auto-assigned columns (such as an auto-increment primary key) are omitted
    /// so the database fills them in.
    fn insert_values(&self) -> Vec<(&'static str, Value)>;

    /// Returns the value of the primary key column for this instance.
    fn primary_key_value(&self) -> Value;

    /// Returns this instance's current optimistic-lock version, or `None` when the
    /// model has no `#[field(version)]` column. Generated by `#[derive(Model)]`.
    fn version_value(&self) -> Option<Value> {
        None
    }

    /// Increments this instance's in-memory version after a successful locked
    /// [`save`](Self::save), so a subsequent save uses the new value. A no-op when
    /// the model has no version column. Generated by `#[derive(Model)]`.
    fn bump_version(&mut self) {}

    /// Applies the model's client-side field defaults (`#[field(default_with = …)]`)
    /// to any field still at its empty value.
    ///
    /// Generated by `#[derive(Model)]`; a no-op when no field declares one. Called
    /// by `create`/`upsert` just before [`before_create`](ModelHooks::before_create),
    /// so a value you set explicitly is preserved and only an unset field is filled.
    fn apply_client_defaults(&mut self) {}

    /// Returns the indexes declared on this model.
    ///
    /// The default is empty; `#[derive(Model)]` overrides it from the field-level
    /// `index`/`unique` attributes and the table-level `#[table(indexes = [...])]`
    /// list. This is a method rather than an associated constant because a partial
    /// index's predicate is a runtime [`Expr`](crate::Expr).
    fn indexes() -> Vec<crate::IndexDef>
    where
        Self: Sized,
    {
        Vec::new()
    }

    /// Renders every index on this model to its `CREATE INDEX` statement.
    ///
    /// This is the reflection helper a future schema-diffing tool builds on; it is
    /// also handy in tests. Each index is rendered for `dialect`, so an unsupported
    /// feature (such as an index method on a backend that lacks one) surfaces as an
    /// error here.
    #[cfg(feature = "migrations")]
    fn index_statements(dialect: &dyn crate::dialect::Dialect) -> crate::Result<Vec<String>>
    where
        Self: Sized,
    {
        Self::indexes()
            .iter()
            .map(|index| crate::migration::render::create_index(dialect, Self::TABLE, index, false))
            .collect()
    }

    /// Returns this model's full intended schema (columns and indexes).
    ///
    /// The reflection `migrate generate` diffs against the live database.
    #[cfg(feature = "migrations")]
    fn table_schema() -> crate::registry::TableSchema
    where
        Self: Sized,
    {
        crate::registry::TableSchema {
            table: Self::TABLE,
            columns: Self::COLUMNS.to_vec(),
            indexes: Self::indexes(),
            checks: Self::CHECKS.to_vec(),
        }
    }

    /// Starts a query over this model.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model};
    /// # async fn run<M: Model>(db: Database) -> tork_orm_core::Result<()> {
    /// let rows = M::query().limit(10).all(&db).await?;
    /// # let _ = rows;
    /// # Ok(())
    /// # }
    /// ```
    fn query() -> QuerySet<Self>
    where
        Self: Sized,
    {
        QuerySet::new()
    }

    /// Finds a single row by its primary key.
    ///
    /// Returns the row if found, or
    /// [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) when no row has that
    /// key.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let user = User::find(&db, 42).await?;
    /// # let _ = user; Ok(())
    /// # }
    /// ```
    fn find<E: Executor + Send>(
        executor: E,
        pk: impl crate::value::BindValue + Send,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        Self: Sized,
    {
        async move {
            Self::query()
                .filter(Expr::binary(
                    Expr::column(Self::TABLE, Self::PRIMARY_KEY),
                    BinaryOp::Eq,
                    Expr::value(pk.to_value()),
                ))
                .one(executor)
                .await
        }
    }

    /// Finds a single row by its primary key, returning `None` when it does not
    /// exist.
    ///
    /// Errors with [`ErrorKind::MultipleFound`](crate::ErrorKind::MultipleFound) if
    /// more than one row matches (which should never happen for a proper primary
    /// key, but the check is there for safety).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// if let Some(user) = User::get_or_none(&db, 42).await? {
    ///     println!("found user {:?}", user.primary_key_value());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    fn get_or_none<E: Executor + Send>(
        executor: E,
        pk: impl crate::value::BindValue + Send,
    ) -> impl std::future::Future<Output = crate::Result<Option<Self>>> + Send
    where
        Self: Sized,
    {
        async move {
            Self::query()
                .filter(Expr::binary(
                    Expr::column(Self::TABLE, Self::PRIMARY_KEY),
                    BinaryOp::Eq,
                    Expr::value(pk.to_value()),
                ))
                .one_or_none(executor)
                .await
        }
    }

    /// Inserts `value` and returns the stored row, including any
    /// database-assigned columns (such as an auto-increment primary key).
    fn create<E: Executor + Send + Sync>(
        executor: E,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        Self: Sized,
    {
        async move {
            // Clone so the mutating `before_create` hook can adjust the row to insert.
            let mut value = value.clone();
            value.apply_client_defaults();
            value.before_create();
            let pairs = value.insert_values();
            let columns: Vec<&'static str> = pairs.iter().map(|(name, _)| *name).collect();
            let row: Vec<Value> = pairs.into_iter().map(|(_, value)| value).collect();
            let supports_returning = executor.dialect().supports_returning();
            let returning: Vec<&'static str> = if supports_returning {
                Self::COLUMNS.iter().map(|column| column.name).collect()
            } else {
                Vec::new()
            };

            let statement = InsertStatement {
                table: Self::TABLE,
                columns,
                rows: vec![row],
                returning,
                on_conflict: OnConflict::None,
            };
            let (sql, params) = render_insert(executor.dialect(), &statement);

            let stored = if supports_returning {
                let rows = executor.fetch_all(sql, params).await?;
                let row = rows.first().ok_or_else(|| {
                    OrmError::query("insert with RETURNING produced no row")
                })?;
                Self::from_row(row)?
            } else {
                // Fallback for backends without RETURNING: insert, then re-select
                // the row by its primary key.
                let inserted = executor.execute(sql, params).await?;
                // A non-integer primary key (a UUID or string) is supplied by the
                // application, not assigned by the database, so reload by its actual
                // value. `last_insert_rowid` only matches an integer primary key.
                let pk_value = match value.primary_key_value() {
                    pk @ (Value::Text(_) | Value::Uuid(_)) => pk,
                    _ => Value::Int(inserted.last_insert_rowid),
                };
                let projection = Self::COLUMNS
                    .iter()
                    .map(|column| SelectItem::Column {
                        table: Self::TABLE,
                        column: column.name,
                    })
                    .collect();
                let mut select = SelectStatement::new(Self::TABLE, projection);
                select.filters.push(Expr::binary(
                    Expr::column(Self::TABLE, Self::PRIMARY_KEY),
                    BinaryOp::Eq,
                    Expr::value(pk_value),
                ));
                select.limit = Some(1);
                let (sql, params) = render_select(executor.dialect(), &select);
                let rows = executor.fetch_all(sql, params).await?;
                let row = rows
                    .first()
                    .ok_or_else(|| OrmError::query("inserted row could not be reloaded"))?;
                Self::from_row(row)?
            };
            stored.after_create(&executor).await?;
            Ok(stored)
        }
    }

    /// Inserts many values in one statement, returning the number inserted.
    ///
    /// An empty slice inserts nothing and returns zero.
    fn bulk_create<E: Executor + Send>(
        executor: E,
        values: &[Self],
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        async move {
            if values.is_empty() {
                return Ok(0);
            }
            let columns: Vec<&'static str> = values[0]
                .insert_values()
                .iter()
                .map(|(name, _)| *name)
                .collect();
            let rows: Vec<Vec<Value>> = values
                .iter()
                .map(|value| value.insert_values().into_iter().map(|(_, v)| v).collect())
                .collect();

            // A single multi-row INSERT binds `rows * columns` parameters, which
            // can exceed the backend's bind-parameter ceiling (for example
            // SQLite's `too many SQL variables`). Split the rows into chunks that
            // each stay within `Dialect::max_bind_params`, running one INSERT per
            // chunk. Pass a transaction executor if the whole insert must be atomic.
            let column_count = columns.len().max(1);
            let rows_per_chunk = (executor.dialect().max_bind_params() / column_count).max(1);

            let mut affected = 0u64;
            for chunk in rows.chunks(rows_per_chunk) {
                let statement = InsertStatement {
                    table: Self::TABLE,
                    columns: columns.clone(),
                    rows: chunk.to_vec(),
                    returning: Vec::new(),
                    on_conflict: OnConflict::None,
                };
                let (sql, params) = render_insert(executor.dialect(), &statement);
                affected += executor.execute(sql, params).await?.rows_affected;
            }
            Ok(affected)
        }
    }

    /// Inserts `value`, or updates the existing row when it conflicts on the
    /// primary key.
    ///
    /// A convenience over [`upsert_on`](Model::upsert_on) using the primary key as
    /// the conflict target. For an auto-increment primary key (which never collides
    /// on insert) target a unique business key with `upsert_on` instead.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let updated = User::upsert(&db, &User { /* ... */ }).await?;
    /// # let _ = updated; Ok(())
    /// # }
    /// ```
    fn upsert<E: Executor + Send + Sync>(
        executor: E,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        Self: Sized,
    {
        Self::upsert_on(executor, value, &[Self::PRIMARY_KEY])
    }

    /// Inserts `value`, or updates the existing row when it conflicts on the
    /// `conflict_target` columns (a unique or primary key).
    ///
    /// Renders portable `INSERT ... ON CONFLICT (target) DO UPDATE SET ...`, setting
    /// every non-target column to its inserted value. Returns the stored row.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// // Insert, or update the existing row with the same email.
    /// let saved = User::upsert_on(&db, &User { /* ... */ }, &["email"]).await?;
    /// # let _ = saved; Ok(())
    /// # }
    /// ```
    fn upsert_on<E: Executor + Send + Sync>(
        executor: E,
        value: &Self,
        conflict_target: &'static [&'static str],
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        Self: Sized,
    {
        async move {
            // Clone so `before_create` can mutate the row to insert.
            let mut value = value.clone();
            value.apply_client_defaults();
            value.before_create();
            let pairs = value.insert_values();
            let columns: Vec<&'static str> = pairs.iter().map(|(name, _)| *name).collect();
            let row: Vec<Value> = pairs.into_iter().map(|(_, v)| v).collect();
            // Update every inserted column that is not part of the conflict target,
            // setting it to the would-be-inserted (EXCLUDED) value.
            let updates: Vec<Assignment> = columns
                .iter()
                .copied()
                .filter(|column| !conflict_target.contains(column))
                .map(|column| Assignment::new(column, Expr::excluded(column)))
                .collect();
            let supports_returning = executor.dialect().supports_returning();
            let returning: Vec<&'static str> = if supports_returning {
                Self::COLUMNS.iter().map(|c| c.name).collect()
            } else {
                Vec::new()
            };
            let statement = InsertStatement {
                table: Self::TABLE,
                columns,
                rows: vec![row],
                returning,
                on_conflict: OnConflict::Update {
                    constraint: conflict_target.to_vec(),
                    updates,
                },
            };
            let (sql, params) = render_insert(executor.dialect(), &statement);

            let stored = if supports_returning {
                let rows = executor.fetch_all(sql, params).await?;
                let row = rows.first().ok_or_else(|| {
                    OrmError::query("upsert with RETURNING produced no row")
                })?;
                Self::from_row(row)?
            } else {
                executor.execute(sql, params).await?;
                // Without RETURNING, re-select by the conflict-target values.
                let projection = Self::COLUMNS
                    .iter()
                    .map(|column| SelectItem::Column {
                        table: Self::TABLE,
                        column: column.name,
                    })
                    .collect();
                let mut select = SelectStatement::new(Self::TABLE, projection);
                let lookup = value.insert_values();
                for target in conflict_target {
                    if let Some((_, v)) = lookup.iter().find(|(name, _)| name == target) {
                        select.filters.push(Expr::binary(
                            Expr::column(Self::TABLE, target),
                            BinaryOp::Eq,
                            Expr::value(v.clone()),
                        ));
                    }
                }
                select.limit = Some(1);
                let (sql, params) = render_select(executor.dialect(), &select);
                let rows = executor.fetch_all(sql, params).await?;
                let row = rows
                    .first()
                    .ok_or_else(|| OrmError::query("upserted row could not be reloaded"))?;
                Self::from_row(row)?
            };
            stored.after_create(&executor).await?;
            Ok(stored)
        }
    }

    /// Tries to find a row matching `filter`, creating it with `value` if none
    /// exists.
    ///
    /// Returns `(row, true)` if a new row was created, or `(row, false)` if an
    /// existing row was found. The lookup uses
    /// [`one_or_none`](crate::QuerySet::one_or_none) so it errors when the filter
    /// matches more than one row.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let (user, created) = User::get_or_create(
    ///     &db,
    ///     |q| q.filter(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar)),
    ///     &User { /* ... */ },
    /// ).await?;
    /// # let _ = (user, created); Ok(())
    /// # }
    /// ```
    fn get_or_create<E, F>(
        executor: E,
        filter: F,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<(Self, bool)>> + Send
    where
        E: Executor + Send + Sync,
        F: FnOnce(QuerySet<Self>) -> QuerySet<Self> + Send,
        Self: Sized,
    {
        async move {
            match filter(Self::query()).one_or_none(&executor).await? {
                Some(row) => Ok((row, false)),
                None => Self::create(executor, value).await.map(|row| (row, true)),
            }
        }
    }

    /// Tries to find a row matching `filter`, updating it with `value`'s fields
    /// if found, or creating it if not.
    ///
    /// Returns `(row, true)` if a new row was created, or `(row, false)` if an
    /// existing row was found and updated.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let (user, created) = User::update_or_create(
    ///     &db,
    ///     |q| q.filter(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar)),
    ///     &User { /* ... */ },
    /// ).await?;
    /// # let _ = (user, created); Ok(())
    /// # }
    /// ```
    fn update_or_create<E, F>(
        executor: E,
        filter: F,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<(Self, bool)>> + Send
    where
        E: Executor + Send + Sync,
        F: FnOnce(QuerySet<Self>) -> QuerySet<Self> + Send,
        Self: Sized,
    {
        async move {
            match filter(Self::query()).one_or_none(&executor).await? {
                Some(found) => {
                    let assignments: Vec<Assignment> = value
                        .insert_values()
                        .into_iter()
                        .map(|(column, v)| Assignment::new(column, Expr::value(v)))
                        .collect();
                    let pk_col = Expr::column(Self::TABLE, Self::PRIMARY_KEY);
                    let pk_val = Expr::value(found.primary_key_value());
                    let statement = UpdateStatement {
                        table: Self::TABLE,
                        assignments,
                        filters: vec![Expr::binary(pk_col, BinaryOp::Eq, pk_val)],
                        returning: Vec::new(),
                    };
                    let (sql, params) = crate::dialect::render_update(executor.dialect(), &statement);
                    executor.execute(sql, params).await?;
                    let updated = Self::find(executor, found.primary_key_value()).await?;
                    Ok((updated, false))
                }
                None => Self::create(executor, value).await.map(|row| (row, true)),
            }
        }
    }

    /// Tries to find the first row matching `filter`, creating it with `value`
    /// if none exists.
    ///
    /// Like [`get_or_create`](Self::get_or_create) but uses
    /// [`first`](crate::QuerySet::first) for the lookup, so the filter matching
    /// multiple rows silently returns the first one.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # #[derive(Clone)] struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } } impl tork_orm_core::ModelHooks for User {}
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let user = User::first_or_create(
    ///     &db,
    ///     |q| q.filter(User::query().into_statement().filters.is_empty().then_some(tork_orm_core::Expr::CountStar).unwrap_or(tork_orm_core::Expr::CountStar)),
    ///     &User { /* ... */ },
    /// ).await?;
    /// # let _ = user; Ok(())
    /// # }
    /// ```
    fn first_or_create<E, F>(
        executor: E,
        filter: F,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        E: Executor + Send + Sync,
        F: FnOnce(QuerySet<Self>) -> QuerySet<Self> + Send,
        Self: Sized,
    {
        async move {
            match filter(Self::query()).first(&executor).await? {
                Some(row) => Ok(row),
                None => Self::create(executor, value).await,
            }
        }
    }

    /// Writes this instance's current field values to the row with its primary
    /// key, returning the number of rows changed (zero if no such row exists).
    ///
    /// Takes `&mut self` so the `before_save` hook can mutate the instance (e.g.
    /// bump `updated_at`) and the caller sees the change.
    fn save<E: Executor + Send + Sync>(
        &mut self,
        executor: E,
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        async move {
            self.before_save();
            let mut assignments: Vec<Assignment> = self
                .insert_values()
                .into_iter()
                .map(|(column, value)| Assignment::new(column, Expr::value(value)))
                .collect();

            // Auto-touch the `updated_at` column with the database's current time,
            // overriding whatever the struct carried.
            if let Some(column) = Self::UPDATED_AT {
                assignments.retain(|assignment| assignment.column != column);
                assignments.push(Assignment::new(column, Expr::raw("CURRENT_TIMESTAMP")));
            }

            let mut filters = vec![Expr::binary(
                Expr::column(Self::TABLE, Self::PRIMARY_KEY),
                BinaryOp::Eq,
                Expr::value(self.primary_key_value()),
            )];

            // Optimistic locking: only update the row whose version still matches,
            // and bump the version in the same statement.
            if let (Some(column), Some(current)) = (Self::VERSION, self.version_value()) {
                filters.push(Expr::binary(
                    Expr::column(Self::TABLE, column),
                    BinaryOp::Eq,
                    Expr::value(current),
                ));
                assignments.retain(|assignment| assignment.column != column);
                assignments.push(Assignment::new(
                    column,
                    Expr::binary(
                        Expr::column(Self::TABLE, column),
                        BinaryOp::Add,
                        Expr::value(Value::Int(1)),
                    ),
                ));
            }

            let statement = UpdateStatement {
                table: Self::TABLE,
                assignments,
                filters,
                returning: Vec::new(),
            };
            let (sql, params) = crate::dialect::render_update(executor.dialect(), &statement);
            let changed = executor.execute(sql, params).await?.rows_affected;

            // With a version column, zero rows changed means the version no longer
            // matched: the row was deleted or updated by someone else.
            if Self::VERSION.is_some() && changed == 0 {
                return Err(crate::OrmError::conflict(format!(
                    "optimistic lock failed for `{}`: the row was modified or removed \
                     by another transaction",
                    Self::TABLE
                )));
            }
            if Self::VERSION.is_some() {
                self.bump_version();
            }

            self.after_save(&executor).await?;
            Ok(changed)
        }
    }

    /// Deletes the row identified by this instance's primary key, returning the
    /// number of rows removed (zero if no row with that key exists).
    fn delete<E: Executor + Send + Sync>(
        &self,
        executor: E,
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        async move {
            self.before_delete();
            let removed = if let Some(column) = Self::DELETED_AT {
                // Soft delete: stamp the row's deleted_at instead of removing it.
                let statement = UpdateStatement {
                    table: Self::TABLE,
                    assignments: vec![Assignment::new(column, Expr::raw("CURRENT_TIMESTAMP"))],
                    filters: vec![self.primary_key_filter()],
                    returning: Vec::new(),
                };
                let (sql, params) = crate::dialect::render_update(executor.dialect(), &statement);
                executor.execute(sql, params).await?.rows_affected
            } else {
                let statement = DeleteStatement {
                    table: Self::TABLE,
                    filters: vec![self.primary_key_filter()],
                    returning: Vec::new(),
                };
                let (sql, params) = crate::dialect::render_delete(executor.dialect(), &statement);
                executor.execute(sql, params).await?.rows_affected
            };
            self.after_delete(&executor).await?;
            Ok(removed)
        }
    }

    /// Permanently removes this row, bypassing soft-delete. Identical to
    /// [`delete`](Self::delete) for models without a soft-delete column.
    fn force_delete<E: Executor + Send + Sync>(
        &self,
        executor: E,
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        async move {
            self.before_delete();
            let statement = DeleteStatement {
                table: Self::TABLE,
                filters: vec![self.primary_key_filter()],
                returning: Vec::new(),
            };
            let (sql, params) = crate::dialect::render_delete(executor.dialect(), &statement);
            let removed = executor.execute(sql, params).await?.rows_affected;
            self.after_delete(&executor).await?;
            Ok(removed)
        }
    }

    /// Clears this row's soft-delete mark (`deleted_at = NULL`), returning the
    /// number of rows restored. A no-op (returns `Ok(0)`) for models without a
    /// soft-delete column.
    fn restore<E: Executor + Send + Sync>(
        &self,
        executor: E,
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        async move {
            let Some(column) = Self::DELETED_AT else {
                return Ok(0);
            };
            let statement = UpdateStatement {
                table: Self::TABLE,
                assignments: vec![Assignment::new(column, Expr::value(Value::Null))],
                filters: vec![self.primary_key_filter()],
                returning: Vec::new(),
            };
            let (sql, params) = crate::dialect::render_update(executor.dialect(), &statement);
            Ok(executor.execute(sql, params).await?.rows_affected)
        }
    }

    /// Builds the `primary_key = <value>` predicate for this instance.
    fn primary_key_filter(&self) -> Expr
    where
        Self: Sized,
    {
        Expr::binary(
            Expr::column(Self::TABLE, Self::PRIMARY_KEY),
            BinaryOp::Eq,
            Expr::value(self.primary_key_value()),
        )
    }
}
