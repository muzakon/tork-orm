//! Generating a migration from the difference between models and the database.
//!
//! `migrate generate` reflects each model's intended schema (its columns and
//! indexes), reads the live database, and emits the statements that reconcile them.
//! For existing tables the diff covers: columns added to the model (`ADD COLUMN`),
//! columns removed from the model (`DROP COLUMN`), and informational notes for
//! type or nullability changes that require a table rebuild on SQLite. New tables
//! are created in full; their indexes are emitted alongside the `CREATE TABLE`.
//!
//! Because the diff needs the application's Rust model types, generate is an
//! application-embedded call, not part of the standalone migration binary.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::OrmError;
use crate::executor::Executor;
use crate::registry::{registered_models, TableSchema};

use super::ddl::{
    AlterAction, AlterTable, ColumnSpec, DefaultValue, ForeignKeySpec, TableDef,
};
use super::introspect::ExistingColumn;
use super::{files, introspect, render};

/// The statements that reconcile the models with the database, in both directions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaChange {
    /// Statements to apply the change.
    pub up: Vec<String>,
    /// Statements to revert it, in reverse order.
    pub down: Vec<String>,
}

impl SchemaChange {
    /// Returns `true` if there are no executable statements to apply.
    ///
    /// SQL comment lines (starting with `--`) are not counted as executable,
    /// so a diff that emits only informational notes is still considered empty
    /// and will not produce a migration file.
    pub fn is_empty(&self) -> bool {
        !self
            .up
            .iter()
            .any(|s| !s.trim_start().starts_with("--"))
    }
}

/// Diffs `models` against the live schema, returning the reconciling statements.
pub async fn generate<E: Executor + Sync>(
    executor: &E,
    models: &[TableSchema],
) -> crate::Result<SchemaChange> {
    let dialect = executor.dialect();
    let mut up: Vec<String> = Vec::new();
    let mut down: Vec<String> = Vec::new();

    for schema in models {
        if !introspect::table_exists(executor, schema.table).await? {
            // The table is absent: create it with its indexes.
            let def = table_def_from_schema(schema);
            up.extend(render::create_table(dialect, &def)?);
            down.push(render::drop_table(dialect, schema.table, true));
            continue;
        }

        let existing_indexes = introspect::existing_indexes(executor, schema.table).await?;
        let existing_index_names: HashSet<&str> =
            existing_indexes.iter().map(|i| i.name.as_str()).collect();
        let model_index_names: HashSet<&str> =
            schema.indexes.iter().map(|i| i.name.as_str()).collect();

        let existing_cols = introspect::existing_columns(executor, schema.table).await?;
        let existing_col_map: HashMap<&str, &ExistingColumn> =
            existing_cols.iter().map(|c| (c.name.as_str(), c)).collect();
        let model_col_names: HashSet<&str> = schema.columns.iter().map(|c| c.name).collect();

        // 1. Drop stale indexes (must precede DROP COLUMN on columns they reference).
        for index in &existing_indexes {
            if !model_index_names.contains(index.name.as_str()) {
                up.push(render::drop_index(dialect, &index.name, true));
                down.push(format!(
                    "-- cannot recreate dropped index \"{}\" (its definition is unknown)",
                    index.name
                ));
            }
        }

        // 2. Drop columns removed from the model.
        for col in &existing_cols {
            if !model_col_names.contains(col.name.as_str()) {
                if col.is_pk {
                    up.push(format!(
                        "-- NOTE: column \"{}\" was removed from the model but cannot be \
                         dropped automatically (primary key); rebuild the table manually",
                        col.name
                    ));
                } else {
                    let alter = AlterTable {
                        table: schema.table.to_string(),
                        actions: vec![AlterAction::DropColumn(col.name.clone())],
                    };
                    for stmt in render::alter_table(dialect, &alter) {
                        up.push(stmt);
                    }
                    down.push(restore_column_sql(dialect, schema.table, col));
                }
            }
        }

        // 3. Note columns whose type or nullability changed (SQLite cannot ALTER a
        //    column in place; a table rebuild is needed).
        for model_col in &schema.columns {
            if let Some(existing_col) = existing_col_map.get(model_col.name) {
                let model_type =
                    render::column_type_str(dialect, model_col.sql_type);
                let db_type = existing_col.declared_type.trim().to_uppercase();
                // Skip type and nullability checks for primary key columns.
                // SQLite always stores an auto-increment PK as INTEGER (regardless
                // of the declared Rust type), and the NOT NULL flag is implicit in
                // the PK constraint and therefore not reflected in `notnull`.
                if existing_col.is_pk {
                    continue;
                }
                let type_changed = model_type.to_uppercase() != db_type
                    && !db_type.is_empty();
                // `not_null` in the DB means `NOT NULL`; `nullable` in the model means
                // the column accepts NULL — they're inverses.
                let nullability_changed = model_col.nullable == existing_col.not_null;
                if type_changed || nullability_changed {
                    up.push(format!(
                        "-- NOTE: column \"{}\" definition changed \
                         (model: {} {}, database: {} {}); \
                         rebuild the table to apply the change",
                        model_col.name,
                        model_type,
                        if model_col.nullable { "nullable" } else { "not null" },
                        existing_col.declared_type,
                        if existing_col.not_null { "not null" } else { "nullable" },
                    ));
                }
            }
        }

        // 4. Add columns new to the model.
        for model_col in &schema.columns {
            if !existing_col_map.contains_key(model_col.name) {
                let mut spec = ColumnSpec::new(model_col.name, model_col.sql_type);
                // PRIMARY KEY and AUTO_INCREMENT cannot be used in ADD COLUMN.
                spec.primary_key = false;
                spec.auto_increment = false;
                spec.default = column_default_ddl(model_col.default);
                if !model_col.nullable {
                    // NOT NULL without a default fails on non-empty tables; emit
                    // the column as nullable and let the developer fill values and
                    // add the constraint via a separate table rebuild if needed.
                    spec.nullable = true;
                    up.push(format!(
                        "-- NOTE: column \"{}\" added as nullable; NOT NULL \
                         requires a default value for existing rows",
                        model_col.name
                    ));
                } else {
                    spec.nullable = true;
                }
                let alter = AlterTable {
                    table: schema.table.to_string(),
                    actions: vec![AlterAction::AddColumn(spec)],
                };
                for stmt in render::alter_table(dialect, &alter) {
                    up.push(stmt);
                }
                let drop_alter = AlterTable {
                    table: schema.table.to_string(),
                    actions: vec![AlterAction::DropColumn(model_col.name.to_string())],
                };
                for stmt in render::alter_table(dialect, &drop_alter) {
                    down.push(stmt);
                }
            }
        }

        // 5. Create indexes new to the model (after columns are in place).
        for index in &schema.indexes {
            if !existing_index_names.contains(index.name.as_str()) {
                up.push(render::create_index(dialect, schema.table, index, false)?);
                down.push(render::drop_index(dialect, &index.name, true));
            }
        }
    }

    down.reverse();
    Ok(SchemaChange { up, down })
}

/// Diffs every registered model against the live schema.
pub async fn generate_from_registry<E: Executor + Sync>(
    executor: &E,
) -> crate::Result<SchemaChange> {
    generate(executor, &registered_models()).await
}

/// Generates a migration from the registered models and writes it to `dir`.
///
/// Returns the new file's path, or `None` when the schema already matches and there
/// is nothing to write. The new revision links onto the current head of `dir`.
pub async fn generate_and_write<E: Executor + Sync>(
    executor: &E,
    dir: &Path,
    name: &str,
) -> crate::Result<Option<PathBuf>> {
    let change = generate_from_registry(executor).await?;
    write_migration(dir, name, &change)
}

/// Writes a migration file for `change` into `dir`, chained onto its current head.
///
/// Returns `None` (writing nothing) when `change` is empty.
pub fn write_migration(
    dir: &Path,
    name: &str,
    change: &SchemaChange,
) -> crate::Result<Option<PathBuf>> {
    if change.is_empty() {
        return Ok(None);
    }
    let down_revision = files::head_revision(dir)?;
    let revision = new_revision();
    let snake = snake_case(name);
    let contents = render_migration_file(&revision, down_revision.as_deref(), &snake, change);
    let path = dir.join(format!("{revision}_{snake}.sql"));
    std::fs::write(&path, contents)
        .map_err(|error| OrmError::configuration(format!("could not write migration file: {error}")))?;
    Ok(Some(path))
}

/// Renders the contents of a migration `.sql` file for `change`.
pub fn render_migration_file(
    revision: &str,
    down_revision: Option<&str>,
    name: &str,
    change: &SchemaChange,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("-- revision: {revision}\n"));
    out.push_str(&format!(
        "-- down_revision: {}\n",
        down_revision.unwrap_or("")
    ));
    out.push_str(&format!("-- name: {name}\n\n"));

    out.push_str("-- migrate:up\n");
    for statement in &change.up {
        push_statement(&mut out, statement);
    }
    out.push_str("\n-- migrate:down\n");
    for statement in &change.down {
        push_statement(&mut out, statement);
    }
    out
}

/// Appends a statement line, terminating SQL with `;` and leaving comments as-is.
fn push_statement(out: &mut String, statement: &str) {
    out.push_str(statement);
    if !statement.trim_start().starts_with("--") {
        out.push(';');
    }
    out.push('\n');
}

/// Builds a `CREATE TABLE` definition from a model's reflected schema.
/// Maps a model column's database-side default to a DDL [`DefaultValue`].
fn column_default_ddl(default: Option<crate::ColumnDefault>) -> Option<DefaultValue> {
    match default? {
        crate::ColumnDefault::CurrentTimestamp => Some(DefaultValue::CurrentTimestamp),
        crate::ColumnDefault::Uuid => Some(DefaultValue::Uuid),
        crate::ColumnDefault::Raw(sql) => Some(DefaultValue::Raw(sql.to_string())),
    }
}

fn table_def_from_schema(schema: &TableSchema) -> TableDef {
    let mut def = TableDef::new(schema.table);
    for column in &schema.columns {
        let mut spec = ColumnSpec::new(column.name, column.sql_type);
        spec.nullable = column.nullable;
        spec.primary_key = column.primary_key;
        spec.auto_increment = column.auto;
        spec.default = column_default_ddl(column.default);
        def.columns.push(spec);
        if let Some(foreign_key) = column.foreign_key {
            def.foreign_keys.push(ForeignKeySpec {
                columns: vec![column.name.to_string()],
                ref_table: foreign_key.table.to_string(),
                ref_columns: vec![foreign_key.column.to_string()],
                on_delete: foreign_key.on_delete,
                on_update: foreign_key.on_update,
            });
        }
    }
    def.indexes = schema.indexes.clone();
    def.checks = schema.checks.iter().map(|check| check.to_string()).collect();
    def
}

/// Renders an `ALTER TABLE ... ADD COLUMN` that restores a dropped column.
///
/// The restored column is always nullable. We do not know the original default
/// or any check constraints, so we emit the minimal form that lets the migration
/// round-trip. Data that existed in the column before the drop is gone.
fn restore_column_sql(
    dialect: &dyn crate::dialect::Dialect,
    table: &str,
    col: &ExistingColumn,
) -> String {
    let mut sql = String::from("ALTER TABLE ");
    dialect.quote_identifier(table, &mut sql);
    sql.push_str(" ADD COLUMN ");
    dialect.quote_identifier(&col.name, &mut sql);
    sql.push(' ');
    if col.declared_type.is_empty() {
        sql.push_str("TEXT");
    } else {
        sql.push_str(&col.declared_type);
    }
    sql
}

/// Generates a fresh 12-character hex revision id.
fn new_revision() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Converts a name to `snake_case`, keeping only ASCII alphanumerics.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}
