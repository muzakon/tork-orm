//! Rendering the DDL AST to SQL.
//!
//! These functions turn the backend-neutral DDL AST into SQL strings for a given
//! [`Dialect`]. Identifiers are quoted through the dialect; the few constructs that
//! differ between backends (notably auto-increment columns and type spelling)
//! branch on [`Dialect::kind`]. Rendering is deterministic — output depends only on
//! the input AST and the dialect — which is what lets a migration's rendered DDL be
//! hashed into a stable checksum.

use crate::dialect::{Dialect, DialectKind, SqlType, predicate_sql, quote_string_literal};
use crate::index::IndexDef;
use crate::error::OrmError;

use super::ddl::{AlterAction, AlterTable, ColumnSpec, DefaultValue, ForeignKeySpec, TableDef};

/// Renders a `CREATE TABLE`, returning the table statement followed by any index
/// statements declared alongside it.
///
/// Returns an error if a declared index uses a feature the dialect does not
/// support (such as an index method on a backend that lacks one).
pub fn create_table(dialect: &dyn Dialect, table: &TableDef) -> crate::Result<Vec<String>> {
    let mut statements = Vec::new();
    let mut sql = String::from("CREATE TABLE ");
    if table.if_not_exists {
        sql.push_str("IF NOT EXISTS ");
    }
    dialect.quote_identifier(&table.name, &mut sql);
    sql.push_str(" (");

    let auto_pk = auto_increment_pk(table);
    for (index, column) in table.columns.iter().enumerate() {
        if index != 0 {
            sql.push_str(", ");
        }
        let is_auto_pk = auto_pk == Some(column.name.as_str());
        render_column(dialect, column, is_auto_pk, &mut sql);
    }

    // A composite or non-auto primary key is a table-level constraint. The
    // single auto-increment primary key is already inline on its column.
    if auto_pk.is_none() {
        let pk = primary_key_columns(table);
        if !pk.is_empty() {
            sql.push_str(", PRIMARY KEY (");
            render_identifier_list(dialect, &pk, &mut sql);
            sql.push(')');
        }
    }

    for foreign_key in &table.foreign_keys {
        sql.push_str(", ");
        render_foreign_key(dialect, foreign_key, &mut sql);
    }

    sql.push(')');
    statements.push(sql);

    for index in &table.indexes {
        statements.push(create_index(dialect, &table.name, index, table.if_not_exists)?);
    }
    Ok(statements)
}

/// Renders a `DROP TABLE`.
pub fn drop_table(dialect: &dyn Dialect, name: &str, if_exists: bool) -> String {
    let mut sql = String::from("DROP TABLE ");
    if if_exists {
        sql.push_str("IF EXISTS ");
    }
    dialect.quote_identifier(name, &mut sql);
    sql
}

/// Renders a `CREATE INDEX` for an index on `table`.
///
/// Covers per-column ordering and operator class, partial `WHERE` predicates
/// (rendered with inline literals, since DDL cannot bind parameters), index method
/// (`USING`), and covering columns (`INCLUDE`). The Postgres-only features are
/// validated against the dialect's capabilities and produce an error rather than
/// being silently dropped.
pub fn create_index(
    dialect: &dyn Dialect,
    table: &str,
    index: &IndexDef,
    if_not_exists: bool,
) -> crate::Result<String> {
    validate_index(dialect, index)?;

    let mut sql = String::from("CREATE ");
    if index.unique {
        sql.push_str("UNIQUE ");
    }
    sql.push_str("INDEX ");
    if if_not_exists {
        sql.push_str("IF NOT EXISTS ");
    }
    dialect.quote_identifier(&index.name, &mut sql);
    sql.push_str(" ON ");
    dialect.quote_identifier(table, &mut sql);
    if let Some(method) = &index.method {
        sql.push_str(" USING ");
        sql.push_str(method);
    }
    sql.push_str(" (");
    for (position, column) in index.columns.iter().enumerate() {
        if position != 0 {
            sql.push_str(", ");
        }
        match &column.expression {
            Some(expression) => {
                sql.push('(');
                sql.push_str(&predicate_sql(dialect, expression));
                sql.push(')');
            }
            None => dialect.quote_identifier(&column.name, &mut sql),
        }
        if let Some(collation) = &column.collation {
            sql.push_str(" COLLATE ");
            sql.push_str(collation);
        }
        if let Some(opclass) = &column.opclass {
            sql.push(' ');
            sql.push_str(opclass);
        }
        if column.descending {
            sql.push_str(" DESC");
        }
        match column.nulls {
            Some(crate::index::NullsOrder::First) => sql.push_str(" NULLS FIRST"),
            Some(crate::index::NullsOrder::Last) => sql.push_str(" NULLS LAST"),
            None => {}
        }
    }
    sql.push(')');
    if !index.include.is_empty() {
        sql.push_str(" INCLUDE (");
        render_identifier_list(dialect, &index.include, &mut sql);
        sql.push(')');
    }
    if let Some(predicate) = &index.predicate {
        sql.push_str(" WHERE ");
        sql.push_str(&predicate_sql(dialect, predicate));
    }
    Ok(sql)
}

/// Rejects an index whose features the dialect does not support.
fn validate_index(dialect: &dyn Dialect, index: &IndexDef) -> crate::Result<()> {
    if let Some(method) = &index.method {
        if !dialect.supports_index_method() {
            return Err(OrmError::configuration(format!(
                "{} does not support index method `{method}`",
                dialect.name()
            )));
        }
    }
    if !index.include.is_empty() && !dialect.supports_index_include() {
        return Err(OrmError::configuration(format!(
            "{} does not support covering index columns (INCLUDE)",
            dialect.name()
        )));
    }
    if index.columns.iter().any(|c| c.opclass.is_some()) && !dialect.supports_index_opclass() {
        return Err(OrmError::configuration(format!(
            "{} does not support index operator classes",
            dialect.name()
        )));
    }
    Ok(())
}

/// Renders a `DROP INDEX`.
pub fn drop_index(dialect: &dyn Dialect, name: &str, if_exists: bool) -> String {
    let mut sql = String::from("DROP INDEX ");
    if if_exists {
        sql.push_str("IF EXISTS ");
    }
    dialect.quote_identifier(name, &mut sql);
    sql
}

/// Renders an `ALTER TABLE ... RENAME TO`.
pub fn rename_table(dialect: &dyn Dialect, from: &str, to: &str) -> String {
    let mut sql = String::from("ALTER TABLE ");
    dialect.quote_identifier(from, &mut sql);
    sql.push_str(" RENAME TO ");
    dialect.quote_identifier(to, &mut sql);
    sql
}

/// Renders an `ALTER TABLE`, one statement per action.
pub fn alter_table(dialect: &dyn Dialect, alter: &AlterTable) -> Vec<String> {
    alter
        .actions
        .iter()
        .map(|action| {
            let mut sql = String::from("ALTER TABLE ");
            dialect.quote_identifier(&alter.table, &mut sql);
            match action {
                AlterAction::AddColumn(column) => {
                    sql.push_str(" ADD COLUMN ");
                    render_column(dialect, column, false, &mut sql);
                }
                AlterAction::DropColumn(name) => {
                    sql.push_str(" DROP COLUMN ");
                    dialect.quote_identifier(name, &mut sql);
                }
                AlterAction::RenameColumn { from, to } => {
                    sql.push_str(" RENAME COLUMN ");
                    dialect.quote_identifier(from, &mut sql);
                    sql.push_str(" TO ");
                    dialect.quote_identifier(to, &mut sql);
                }
            }
            sql
        })
        .collect()
}

/// Returns the name of the single auto-increment primary key column, if the table
/// has exactly one (and no explicit composite primary key).
fn auto_increment_pk(table: &TableDef) -> Option<&str> {
    if !table.primary_key.is_empty() {
        return None;
    }
    let mut found = None;
    for column in &table.columns {
        if column.primary_key && column.auto_increment {
            if found.is_some() {
                return None; // more than one — not the simple rowid case
            }
            found = Some(column.name.as_str());
        }
    }
    found
}

/// Collects the primary key column names (composite list, else inline-marked).
fn primary_key_columns(table: &TableDef) -> Vec<String> {
    if !table.primary_key.is_empty() {
        return table.primary_key.clone();
    }
    table
        .columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.clone())
        .collect()
}

/// Renders one column. When `is_auto_pk`, emits the dialect's auto-increment
/// primary key form instead of the normal type and constraints.
fn render_column(dialect: &dyn Dialect, column: &ColumnSpec, is_auto_pk: bool, out: &mut String) {
    dialect.quote_identifier(&column.name, out);
    out.push(' ');

    if is_auto_pk {
        match dialect.kind() {
            // SQLite aliases the rowid only for a literal `INTEGER PRIMARY KEY`.
            DialectKind::Sqlite => out.push_str("INTEGER PRIMARY KEY AUTOINCREMENT"),
            // Reserved: future backends use their own identity syntax.
            DialectKind::Postgres => out.push_str("BIGSERIAL PRIMARY KEY"),
            DialectKind::Mysql => out.push_str("BIGINT AUTO_INCREMENT PRIMARY KEY"),
        }
        return;
    }

    dialect.map_sql_type(column.ty, out);
    if !column.nullable {
        out.push_str(" NOT NULL");
    }
    if column.unique {
        out.push_str(" UNIQUE");
    }
    if let Some(default) = &column.default {
        out.push_str(" DEFAULT ");
        render_default(dialect, default, out);
    }
}

/// Returns the rendered column type for a dialect as an owned string.
///
/// Used by `generate` to compare the model's expected type against the live
/// type string returned by database introspection. Delegates to
/// [`Dialect::map_sql_type`] so the comparison uses the same spelling the DDL
/// would emit.
pub(crate) fn column_type_str(dialect: &dyn Dialect, ty: SqlType) -> String {
    let mut out = String::new();
    dialect.map_sql_type(ty, &mut out);
    out
}

/// Renders a default value as a SQL literal.
fn render_default(dialect: &dyn Dialect, default: &DefaultValue, out: &mut String) {
    match default {
        DefaultValue::Bool(value) => out.push_str(dialect.bool_literal(*value)),
        DefaultValue::Int(value) => out.push_str(&value.to_string()),
        DefaultValue::Real(value) => out.push_str(&value.to_string()),
        DefaultValue::Text(value) => quote_string_literal(value, out),
        DefaultValue::CurrentTimestamp => out.push_str("CURRENT_TIMESTAMP"),
        DefaultValue::Null => out.push_str("NULL"),
        DefaultValue::Raw(sql) => out.push_str(sql),
    }
}

/// Renders a foreign key constraint clause.
fn render_foreign_key(dialect: &dyn Dialect, fk: &ForeignKeySpec, out: &mut String) {
    out.push_str("FOREIGN KEY (");
    render_identifier_list(dialect, &fk.columns, out);
    out.push_str(") REFERENCES ");
    dialect.quote_identifier(&fk.ref_table, out);
    out.push_str(" (");
    render_identifier_list(dialect, &fk.ref_columns, out);
    out.push(')');
    if let Some(action) = fk.on_delete.as_sql() {
        out.push_str(" ON DELETE ");
        out.push_str(action);
    }
    if let Some(action) = fk.on_update.as_sql() {
        out.push_str(" ON UPDATE ");
        out.push_str(action);
    }
}

/// Renders a comma-separated list of quoted identifiers.
fn render_identifier_list(dialect: &dyn Dialect, names: &[String], out: &mut String) {
    for (index, name) in names.iter().enumerate() {
        if index != 0 {
            out.push_str(", ");
        }
        dialect.quote_identifier(name, out);
    }
}
