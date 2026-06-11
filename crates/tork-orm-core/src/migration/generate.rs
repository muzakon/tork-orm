//! Generating a migration from the difference between models and the database.
//!
//! `migrate generate` reflects each model's intended schema (its columns and
//! indexes), reads the live database, and emits the statements that reconcile them.
//! The scope is index-centric: it creates a wholly missing table (with its indexes)
//! and reconciles indexes on existing tables. Column-level changes on an existing
//! table (added, dropped, or retyped columns) are out of scope and left to a
//! hand-written migration.
//!
//! Because the diff needs the application's Rust model types, generate is an
//! application-embedded call, not part of the standalone migration binary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::OrmError;
use crate::executor::Executor;
use crate::registry::{registered_models, TableSchema};

use super::ddl::{ColumnSpec, ForeignKeyAction, ForeignKeySpec, TableDef};
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
    /// Returns `true` if there is nothing to apply.
    pub fn is_empty(&self) -> bool {
        self.up.is_empty()
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

        let existing = introspect::existing_indexes(executor, schema.table).await?;
        let existing_names: HashSet<&str> = existing.iter().map(|i| i.name.as_str()).collect();
        let model_names: HashSet<&str> = schema.indexes.iter().map(|i| i.name.as_str()).collect();

        // Indexes the model declares but the database lacks.
        for index in &schema.indexes {
            if !existing_names.contains(index.name.as_str()) {
                up.push(render::create_index(dialect, schema.table, index, false)?);
                down.push(render::drop_index(dialect, &index.name, true));
            }
        }
        // Indexes the database has but the model no longer declares.
        for index in &existing {
            if !model_names.contains(index.name.as_str()) {
                up.push(render::drop_index(dialect, &index.name, true));
                down.push(format!(
                    "-- cannot recreate dropped index \"{}\" (its definition is unknown)",
                    index.name
                ));
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
fn table_def_from_schema(schema: &TableSchema) -> TableDef {
    let mut def = TableDef::new(schema.table);
    for column in &schema.columns {
        let mut spec = ColumnSpec::new(column.name, column.sql_type);
        spec.nullable = column.nullable;
        spec.primary_key = column.primary_key;
        spec.auto_increment = column.auto;
        def.columns.push(spec);
        if let Some(foreign_key) = column.foreign_key {
            def.foreign_keys.push(ForeignKeySpec {
                columns: vec![column.name.to_string()],
                ref_table: foreign_key.table.to_string(),
                ref_columns: vec![foreign_key.column.to_string()],
                on_delete: ForeignKeyAction::NoAction,
                on_update: ForeignKeyAction::NoAction,
            });
        }
    }
    def.indexes = schema.indexes.clone();
    def
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
