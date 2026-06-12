//! The `#[derive(DbEnum)]` macro.
//!
//! Generates a [`DbEnum`] implementation for a unit-only enum plus the
//! `BindValue`/`FromValue` impls that let the enum be used as a model field, a
//! bound parameter, and a value read back from a row. Variants map to their
//! `snake_case` name by default; `#[db_enum(rename_all = "...")]` changes the
//! whole-enum convention and `#[db_enum(rename = "...")]` overrides a single one.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, LitStr, Variant};

use crate::common::{krate, to_snake};

/// Expands `#[derive(DbEnum)]`.
pub fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_db_enum(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Container-level `#[db_enum(...)]` options.
struct ContainerArgs {
    /// An explicit enum name (defaults to the `snake_case` of the type).
    name: Option<String>,
    /// The whole-enum casing convention (defaults to `snake_case`).
    rename_all: Option<String>,
}

fn parse_container(input: &DeriveInput) -> syn::Result<ContainerArgs> {
    let mut args = ContainerArgs { name: None, rename_all: None };
    for attr in &input.attrs {
        if !attr.path().is_ident("db_enum") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                args.name = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("rename_all") {
                args.rename_all = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else {
                Err(meta.error("unknown `db_enum` option (expected `name` or `rename_all`)"))
            }
        })?;
    }
    Ok(args)
}

/// A variant-level `#[db_enum(rename = "...")]` override, if present.
fn parse_variant_rename(variant: &Variant) -> syn::Result<Option<String>> {
    let mut rename = None;
    for attr in &variant.attrs {
        if !attr.path().is_ident("db_enum") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                rename = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else {
                Err(meta.error("unknown `db_enum` variant option (expected `rename`)"))
            }
        })?;
    }
    Ok(rename)
}

/// Applies a `rename_all` convention to a `PascalCase` variant identifier.
fn apply_rename_all(ident: &str, mode: &str, span: Span) -> syn::Result<String> {
    let snake = to_snake(ident);
    Ok(match mode {
        "snake_case" => snake,
        "SCREAMING_SNAKE_CASE" => snake.to_uppercase(),
        "kebab-case" => snake.replace('_', "-"),
        "lowercase" => ident.to_lowercase(),
        "UPPERCASE" => ident.to_uppercase(),
        "PascalCase" => ident.to_string(),
        "camelCase" => {
            let mut chars = ident.chars();
            match chars.next() {
                Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        other => {
            return Err(syn::Error::new(
                span,
                format!(
                    "unknown `rename_all` value `{other}` (expected one of: snake_case, \
                     SCREAMING_SNAKE_CASE, kebab-case, lowercase, UPPERCASE, PascalCase, camelCase)"
                ),
            ));
        }
    })
}

fn expand_db_enum(input: DeriveInput) -> syn::Result<TokenStream2> {
    let ident = &input.ident;

    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(DbEnum)] supports only enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(DbEnum)] requires at least one variant",
        ));
    }

    let container = parse_container(&input)?;
    let rename_all = container.rename_all.as_deref().unwrap_or("snake_case");
    let enum_name = container
        .name
        .unwrap_or_else(|| to_snake(&ident.to_string()));

    let mut variant_idents = Vec::new();
    let mut variant_values = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                "#[derive(DbEnum)] supports only unit variants (no fields)",
            ));
        }
        let value = match parse_variant_rename(variant)? {
            Some(rename) => rename,
            None => apply_rename_all(&variant.ident.to_string(), rename_all, variant.ident.span())?,
        };
        variant_idents.push(variant.ident.clone());
        variant_values.push(value);
    }

    // Reject duplicate stored values, which would make `from_db_str` ambiguous.
    for (index, value) in variant_values.iter().enumerate() {
        if let Some(prior) = variant_values[..index].iter().position(|v| v == value) {
            return Err(syn::Error::new_spanned(
                &variant_idents[index],
                format!(
                    "stored value `{value}` is also used by variant `{}`; \
                     give one a distinct `#[db_enum(rename = \"...\")]`",
                    variant_idents[prior]
                ),
            ));
        }
    }

    let krate = krate();
    let values = &variant_values;
    let as_arms = variant_idents
        .iter()
        .zip(variant_values.iter())
        .map(|(id, value)| quote!(Self::#id => #value));
    let from_arms = variant_idents
        .iter()
        .zip(variant_values.iter())
        .map(|(id, value)| quote!(#value => ::core::result::Result::Ok(Self::#id)));

    Ok(quote! {
        impl #krate::DbEnum for #ident {
            const ENUM_NAME: &'static str = #enum_name;
            const VARIANTS: &'static [&'static str] = &[#(#values),*];

            fn as_db_str(&self) -> &'static str {
                match self {
                    #(#as_arms),*
                }
            }

            fn from_db_str(value: &str) -> #krate::Result<Self> {
                match value {
                    #(#from_arms,)*
                    other => ::core::result::Result::Err(#krate::OrmError::conversion(
                        ::std::format!("invalid value `{}` for enum `{}`", other, #enum_name)
                    )),
                }
            }
        }

        impl #krate::BindValue for #ident {
            fn to_value(&self) -> #krate::Value {
                #krate::Value::Text(::std::string::ToString::to_string(
                    #krate::DbEnum::as_db_str(self)
                ))
            }
        }

        impl #krate::FromValue for #ident {
            fn from_value(value: #krate::Value) -> #krate::Result<Self> {
                match value {
                    #krate::Value::Text(text) => <Self as #krate::DbEnum>::from_db_str(&text),
                    other => ::core::result::Result::Err(#krate::OrmError::conversion(
                        ::std::format!("cannot read enum `{}` from value `{:?}`", #enum_name, other)
                    )),
                }
            }
        }
    })
}
