//! Shared helpers for the ORM derive macros.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{GenericArgument, PathArguments, Type};

/// The path generated code uses to reach the ORM's public API.
///
/// Everything is referenced through the `tork-orm` facade so generated code
/// compiles in user crates that depend only on `tork-orm`.
pub fn krate() -> TokenStream {
    quote!(::tork_orm)
}

/// Returns the inner type of `Option<T>`, or `None` for any other type.
pub fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// Returns `true` if `ty` is the named single-segment type (e.g. `i64`, `String`).
fn is_ident(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == name))
}

/// Returns the inner type of `Vec<T>`, or `None` for any other type.
fn vec_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Vec" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// Returns `true` if `ty` is `Vec<u8>` (a blob, not an array).
fn is_byte_vec(ty: &Type) -> bool {
    vec_inner(ty).is_some_and(|inner| is_ident(inner, "u8"))
}

/// Returns `true` if `ty` is a JSON type: the `tork_orm::Json` alias or
/// `serde_json::Value`.
fn is_json(ty: &Type) -> bool {
    if is_ident(ty, "Json") {
        return true;
    }
    let Type::Path(path) = ty else {
        return false;
    };
    let is_value = path.path.segments.last().is_some_and(|s| s.ident == "Value");
    let from_serde_json = path.path.segments.iter().any(|s| s.ident == "serde_json");
    is_value && from_serde_json
}

/// Returns `true` if `ty` is a UUID type (`uuid::Uuid` or an imported `Uuid`).
fn is_uuid(ty: &Type) -> bool {
    is_ident(ty, "Uuid")
}

/// The category of a column type, for dialect-capability validation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A JSON column (PostgreSQL-only).
    Json,
    /// A UUID column (PostgreSQL-only).
    Uuid,
    /// An array column (PostgreSQL-only).
    Array,
    /// Any other column type, supported everywhere.
    Other,
}

/// Returns `true` if `ty` is a timestamp type (`OffsetDateTime` / `DateTimeUtc`).
pub fn is_timestamp_type(ty: &Type) -> bool {
    is_ident(ty, "OffsetDateTime") || is_ident(ty, "DateTimeUtc")
}

/// Returns `true` if `ty` is a supported integer type.
pub fn is_integer_type(ty: &Type) -> bool {
    ["i32", "i64", "u32", "u64"].iter().any(|name| is_ident(ty, name))
}

/// Classifies a field's (option-unwrapped) type for dialect validation.
pub fn field_kind(ty: &Type) -> FieldKind {
    if is_json(ty) {
        FieldKind::Json
    } else if is_uuid(ty) {
        FieldKind::Uuid
    } else if is_byte_vec(ty) {
        FieldKind::Other
    } else if vec_inner(ty).is_some() {
        FieldKind::Array
    } else {
        FieldKind::Other
    }
}

/// Maps a Rust field type (the inner type for an `Option`) to a `SqlType`
/// expression. Unrecognized types map to `Text`, which suits string-backed
/// custom types such as enums stored by their name.
pub fn sql_type_for(ty: &Type) -> TokenStream {
    let krate = krate();
    if is_ident(ty, "bool") {
        quote!(#krate::SqlType::Boolean)
    } else if is_ident(ty, "i32") || is_ident(ty, "u32") {
        quote!(#krate::SqlType::Integer)
    } else if is_ident(ty, "i64") || is_ident(ty, "u64") {
        quote!(#krate::SqlType::BigInt)
    } else if is_ident(ty, "f64") || is_ident(ty, "f32") {
        quote!(#krate::SqlType::Real)
    } else if is_byte_vec(ty) {
        quote!(#krate::SqlType::Blob)
    } else if is_json(ty) {
        quote!(#krate::SqlType::Json)
    } else if is_uuid(ty) {
        quote!(#krate::SqlType::Uuid)
    } else if let Some(inner) = vec_inner(ty) {
        let inner_type = sql_type_for(inner);
        quote!(#krate::SqlType::Array(&#inner_type))
    } else if is_ident(ty, "OffsetDateTime") || is_ident(ty, "DateTimeUtc") {
        quote!(#krate::SqlType::Timestamp)
    } else {
        quote!(#krate::SqlType::Text)
    }
}

/// Converts a `PascalCase` identifier to `snake_case`.
pub fn to_snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for (index, ch) in input.chars().enumerate() {
        if ch.is_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
