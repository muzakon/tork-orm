//! Procedural macros for the Tork ORM.
//!
//! Every macro here emits code that refers to the ORM's public API through the
//! `tork-orm` facade crate (for example `::tork_orm::Value`), never through
//! `tork-orm-core` directly, so generated code compiles inside user crates that
//! depend only on `tork-orm`.

use proc_macro::TokenStream;

mod common;
mod db_enum;
mod migration;
mod model;
mod query_result;
mod relations;

/// Derives the [`Model`] trait for a struct that maps to a database table.
///
/// Generates the table metadata, a `FromRow` implementation, and the insert and
/// primary-key value accessors.
///
/// # Container attribute
///
/// - `#[table(name = "users")]` sets the table name (defaults to the struct name
///   in `snake_case`).
///
/// # Field attributes (`#[field(...)]`)
///
/// - `primary_key` marks the primary key column (exactly one is required)
/// - `auto` marks a database-assigned value, omitted on insert
/// - `varchar(length = N)` records a bounded text type
/// - `foreign_key = Other::column` records a foreign key reference
/// - `column = "name"` overrides the column name (defaults to the field name)
///
/// # Example
///
/// ```ignore
/// #[derive(Debug, Clone, Model)]
/// #[table(name = "users")]
/// pub struct User {
///     #[field(primary_key, auto)]
///     pub id: i64,
///     #[field(varchar(length = 50))]
///     pub username: String,
///     pub is_active: bool,
/// }
/// ```
#[proc_macro_derive(Model, attributes(table, field))]
pub fn derive_model(item: TokenStream) -> TokenStream {
    model::expand(item)
}

/// Derives [`FromRow`] for a projection result type.
///
/// Each field is read from the result column of the same name, so it pairs with a
/// `select(...)` whose items are aliased to those names.
///
/// # Example
///
/// ```ignore
/// #[derive(QueryResult)]
/// pub struct UserPostStats {
///     pub user_id: i64,
///     pub post_count: i64,
/// }
/// // ... .select((User::id.as_("user_id"), Post::id.count().as_("post_count")))
/// //     .all_as::<UserPostStats>(&db)
/// ```
#[proc_macro_derive(QueryResult)]
pub fn derive_query_result(item: TokenStream) -> TokenStream {
    query_result::expand(item)
}

/// Derives [`DbEnum`] for a unit-only enum stored as text.
///
/// Generates the [`DbEnum`] metadata plus `BindValue`/`FromValue`, so the enum can
/// be a model field (with `#[field(db_enum)]`), a bound parameter, and a value read
/// back from a row. Variants map to their `snake_case` name by default.
///
/// # Attributes
///
/// - `#[db_enum(name = "...")]` overrides the enum name (defaults to the type's
///   `snake_case`)
/// - `#[db_enum(rename_all = "...")]` sets the whole-enum casing (`snake_case`,
///   `SCREAMING_SNAKE_CASE`, `kebab-case`, `lowercase`, `UPPERCASE`, `PascalCase`,
///   `camelCase`)
/// - `#[db_enum(rename = "...")]` on a variant overrides its stored value
///
/// # Example
///
/// ```ignore
/// #[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
/// pub enum Status {
///     Active,                                // -> 'active'
///     #[db_enum(rename = "on_hold")] OnHold, // -> 'on_hold'
/// }
/// ```
#[proc_macro_derive(DbEnum, attributes(db_enum))]
pub fn derive_db_enum(item: TokenStream) -> TokenStream {
    db_enum::expand(item)
}

/// Declares the relations of a model on an `impl` block.
///
/// Each method names a relation and is rewritten into an accessor returning a
/// [`Relation`] descriptor used by `QuerySet::join` (and, later, preloading).
///
/// # Method attributes
///
/// - `#[has_many(Other, foreign_key = Other::this_id)]` — a one-to-many where the
///   other model carries this model's key
/// - `#[belongs_to(Other, foreign_key = Self::other_id)]` — a many-to-one where
///   this model carries the other model's key
///
/// # Example
///
/// ```ignore
/// #[relations]
/// impl User {
///     #[has_many(Post, foreign_key = Post::user_id)]
///     pub fn posts() {}
/// }
/// // `User::posts()` now returns a `Relation<User, Post>`.
/// ```
#[proc_macro_attribute]
pub fn relations(_attr: TokenStream, item: TokenStream) -> TokenStream {
    relations::expand(item)
}

/// Implements [`MigrationTrait`] from plain migration methods.
///
/// Applied to an `impl` block with `fn revision()`/`fn name()` returning ids and
/// `async fn up`/`async fn down` taking `&mut SchemaManager`, it generates the
/// trait implementation (boxing the async bodies). An optional
/// `fn transaction(&self) -> MigrationTransaction` is passed through.
///
/// # Example
///
/// ```ignore
/// #[migration]
/// impl Migration {
///     fn revision() -> &'static str { "20260611_143000" }
///     fn name() -> &'static str { "create_users" }
///     async fn up(schema: &mut SchemaManager<'_>) -> Result<()> { /* ... */ Ok(()) }
///     async fn down(schema: &mut SchemaManager<'_>) -> Result<()> { /* ... */ Ok(()) }
/// }
/// ```
#[proc_macro_attribute]
pub fn migration(_attr: TokenStream, item: TokenStream) -> TokenStream {
    migration::expand(item)
}
