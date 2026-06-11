//! The model registry and table-schema reflection used by `migrate generate`.
//!
//! `migrate generate` compares each model's intended schema against the live
//! database. To do that without the caller hand-listing every model, each
//! `#[derive(Model)]` submits a [`ModelSchemaEntry`] into a link-time registry
//! (via the `inventory` crate); [`registered_models`] reads them all back.
//!
//! This module is part of the `migrations` feature. It cannot live in the
//! standalone migration CLI binary, which never sees the application's Rust model
//! types — generate is therefore an application-embedded call.

use crate::index::IndexDef;
use crate::model::ColumnDef;

/// The full intended schema of a model's table: its columns and its indexes.
///
/// Built from a [`Model`](crate::Model) via
/// [`Model::table_schema`](crate::Model::table_schema). It is the input to the
/// generate diff.
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// The table name.
    pub table: &'static str,
    /// The columns, in declaration order.
    pub columns: Vec<ColumnDef>,
    /// The declared indexes.
    pub indexes: Vec<IndexDef>,
}

/// A registry entry contributed by one model.
///
/// Holds a function pointer to the model's schema reflection rather than the schema
/// itself, so registration stays a cheap `const` value.
pub struct ModelSchemaEntry {
    /// The model's table name.
    pub table: &'static str,
    /// Reflects the model's full intended schema.
    pub schema: fn() -> TableSchema,
}

impl ModelSchemaEntry {
    /// Builds a registry entry for a model's table and schema reflector.
    pub const fn new(table: &'static str, schema: fn() -> TableSchema) -> Self {
        Self { table, schema }
    }
}

inventory::collect!(ModelSchemaEntry);

/// Returns the intended schema of every registered model.
///
/// A model is registered automatically by `#[derive(Model)]` when the
/// `migrations` feature is on. Only models linked into the running binary appear.
pub fn registered_models() -> Vec<TableSchema> {
    inventory::iter::<ModelSchemaEntry>
        .into_iter()
        .map(|entry| (entry.schema)())
        .collect()
}
