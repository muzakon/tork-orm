//! Database-backed enumerations.
//!
//! A [`DbEnum`] is a Rust enum stored in the database as one of a fixed set of
//! text values. Deriving it (`#[derive(DbEnum)]`) is the normal path: the derive
//! fills in the metadata below and also generates [`BindValue`](crate::BindValue)
//! and [`FromValue`](crate::FromValue), so the enum can be used directly as a
//! model field (with `#[field(db_enum)]`), bound as a query parameter, and read
//! back from a result row.
//!
//! The column is rendered as a native `ENUM(...)` on MySQL and as a text column
//! plus a `CHECK (... IN (...))` constraint on dialects without a native enum
//! type, so a bad value is rejected by the database on every backend.

use crate::dialect::SqlType;

/// A Rust enum mapped to a fixed set of stored text values.
pub trait DbEnum: Sized {
    /// The enum's name, used as the type/constraint name in generated DDL.
    const ENUM_NAME: &'static str;

    /// Every allowed stored value, in declaration order.
    const VARIANTS: &'static [&'static str];

    /// The abstract column type for this enum.
    ///
    /// Used by the `Model` derive (via `#[field(db_enum)]`) to record the column's
    /// type, including the variant list the DDL renderer constrains against.
    const SQL_TYPE: SqlType = SqlType::Enum {
        name: Self::ENUM_NAME,
        variants: Self::VARIANTS,
    };

    /// Returns this value's stored text form.
    fn as_db_str(&self) -> &'static str;

    /// Parses a stored text value back into the enum.
    ///
    /// Returns a conversion error when the value is not one of [`VARIANTS`].
    ///
    /// [`VARIANTS`]: DbEnum::VARIANTS
    fn from_db_str(value: &str) -> crate::Result<Self>;
}
