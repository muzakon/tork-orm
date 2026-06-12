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
use crate::query::write::{Assignment, InsertStatement, OnConflict, UpdateStatement};
use crate::row::Row;
use crate::value::Value;

/// A foreign key reference recorded on a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignKeyDef {
    /// The referenced table.
    pub table: &'static str,
    /// The referenced column in that table.
    pub column: &'static str,
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
pub trait Model: FromRow + Send + Sync + 'static {
    /// The table this model maps to.
    const TABLE: &'static str;
    /// The description of every column, in declaration order.
    const COLUMNS: &'static [ColumnDef];
    /// The name of the primary key column.
    const PRIMARY_KEY: &'static str;

    /// Returns the column-name and value pairs to write on insert.
    ///
    /// Auto-assigned columns (such as an auto-increment primary key) are omitted
    /// so the database fills them in.
    fn insert_values(&self) -> Vec<(&'static str, Value)>;

    /// Returns the value of the primary key column for this instance.
    fn primary_key_value(&self) -> Value;

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

    /// Inserts `value` and returns the stored row, including any
    /// database-assigned columns (such as an auto-increment primary key).
    fn create<E: Executor + Send>(
        executor: E,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        Self: Sized,
    {
        async move {
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

            if supports_returning {
                let rows = executor.fetch_all(sql, params).await?;
                let row = rows.first().ok_or_else(|| {
                    OrmError::query("insert with RETURNING produced no row")
                })?;
                return Self::from_row(row);
            }

            // Fallback for backends without RETURNING: insert, then re-select the
            // row by the id the insert assigned.
            let inserted = executor.execute(sql, params).await?;
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
                Expr::value(Value::Int(inserted.last_insert_rowid)),
            ));
            select.limit = Some(1);
            let (sql, params) = render_select(executor.dialect(), &select);
            let rows = executor.fetch_all(sql, params).await?;
            let row = rows
                .first()
                .ok_or_else(|| OrmError::query("inserted row could not be reloaded"))?;
            Self::from_row(row)
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
            let statement = InsertStatement {
                table: Self::TABLE,
                columns,
                rows,
                returning: Vec::new(),
                on_conflict: OnConflict::None,
            };
            let (sql, params) = render_insert(executor.dialect(), &statement);
            Ok(executor.execute(sql, params).await?.rows_affected)
        }
    }

    /// Inserts `value`, replacing any existing row that conflicts on a unique key.
    ///
    /// Uses `INSERT OR REPLACE INTO` (SQLite) which deletes the conflicting row
    /// and then inserts the new one. Returns the stored row including any columns
    /// assigned by the database (such as a new auto-increment primary key, since
    /// the old row was deleted first).
    ///
    /// For a "skip on conflict" strategy use [`Model::create`] with [`OnConflict::Ignore`]
    /// directly on an [`InsertStatement`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use tork_orm_core::{Database, Model, Value};
    /// # struct User; impl tork_orm_core::FromRow for User { fn from_row(_: &tork_orm_core::Row) -> tork_orm_core::Result<Self> { Ok(User) } } impl Model for User { const TABLE: &'static str = "users"; const COLUMNS: &'static [tork_orm_core::ColumnDef] = &[]; const PRIMARY_KEY: &'static str = "id"; fn insert_values(&self) -> Vec<(&'static str, Value)> { vec![] } fn primary_key_value(&self) -> Value { Value::Null } }
    /// # async fn run(db: Database) -> tork_orm_core::Result<()> {
    /// let updated = User::upsert(&db, &User { /* ... */ }).await?;
    /// # let _ = updated; Ok(())
    /// # }
    /// ```
    fn upsert<E: Executor + Send>(
        executor: E,
        value: &Self,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send
    where
        Self: Sized,
    {
        async move {
            let pairs = value.insert_values();
            let columns: Vec<&'static str> = pairs.iter().map(|(name, _)| *name).collect();
            let row: Vec<Value> = pairs.into_iter().map(|(_, v)| v).collect();
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
                on_conflict: OnConflict::Replace,
            };
            let (sql, params) = render_insert(executor.dialect(), &statement);

            if supports_returning {
                let rows = executor.fetch_all(sql, params).await?;
                let row = rows.first().ok_or_else(|| {
                    OrmError::query("upsert with RETURNING produced no row")
                })?;
                return Self::from_row(row);
            }

            let inserted = executor.execute(sql, params).await?;
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
                Expr::value(Value::Int(inserted.last_insert_rowid)),
            ));
            select.limit = Some(1);
            let (sql, params) = render_select(executor.dialect(), &select);
            let rows = executor.fetch_all(sql, params).await?;
            let row = rows
                .first()
                .ok_or_else(|| OrmError::query("upserted row could not be reloaded"))?;
            Self::from_row(row)
        }
    }

    /// Writes this instance's current field values to the row with its primary
    /// key, returning the number of rows changed (zero if no such row exists).
    fn save<E: Executor + Send>(
        &self,
        executor: E,
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        async move {
            let assignments: Vec<Assignment> = self
                .insert_values()
                .into_iter()
                .map(|(column, value)| Assignment::new(column, Expr::value(value)))
                .collect();
            let statement = UpdateStatement {
                table: Self::TABLE,
                assignments,
                filters: vec![Expr::binary(
                    Expr::column(Self::TABLE, Self::PRIMARY_KEY),
                    BinaryOp::Eq,
                    Expr::value(self.primary_key_value()),
                )],
                returning: Vec::new(),
            };
            let (sql, params) = crate::dialect::render_update(executor.dialect(), &statement);
            Ok(executor.execute(sql, params).await?.rows_affected)
        }
    }

    /// Deletes the row identified by this instance's primary key, returning the
    /// number of rows removed (zero if no row with that key exists).
    fn delete<E: Executor + Send>(
        &self,
        executor: E,
    ) -> impl std::future::Future<Output = crate::Result<u64>> + Send
    where
        Self: Sized,
    {
        let pk = self.primary_key_value();
        async move {
            let filter = Expr::binary(
                Expr::column(Self::TABLE, Self::PRIMARY_KEY),
                BinaryOp::Eq,
                Expr::value(pk),
            );
            Self::query().filter(filter).delete(executor).await
        }
    }
}
