//! The `#[migration]` attribute macro.
//!
//! Applied to an `impl` block, it generates the [`MigrationTrait`] implementation
//! from plain methods: `revision`/`name` return ids, and `up`/`down` are written as
//! natural `async fn`s. The macro wraps their bodies in the boxed future the trait
//! expects, so migrations read as ordinary async code with no extra dependency.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ImplItem, ImplItemFn, ItemImpl, Pat, parse_macro_input};

use crate::common::krate;

/// Expands `#[migration]` over an impl block.
pub fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    match expand_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_impl(input: ItemImpl) -> syn::Result<TokenStream2> {
    let self_ty = &input.self_ty;
    let krate = krate();

    let mut revision = None;
    let mut name = None;
    let mut up = None;
    let mut down = None;
    let mut transaction = None;
    let mut passthrough: Vec<&ImplItem> = Vec::new();

    for item in &input.items {
        let ImplItem::Fn(method) = item else {
            passthrough.push(item);
            continue;
        };
        match method.sig.ident.to_string().as_str() {
            "revision" => revision = Some(method),
            "name" => name = Some(method),
            "up" => up = Some(method),
            "down" => down = Some(method),
            "transaction" => transaction = Some(method),
            _ => passthrough.push(item),
        }
    }

    let revision = revision.ok_or_else(|| missing("revision", &input))?;
    let name = name.ok_or_else(|| missing("name", &input))?;
    let up = up.ok_or_else(|| missing("up", &input))?;
    let down = down.ok_or_else(|| missing("down", &input))?;

    let up_method = schema_method(&krate, "up", up)?;
    let down_method = schema_method(&krate, "down", down)?;
    let transaction_method = transaction.map(|method| quote!(#method));

    Ok(quote! {
        impl #self_ty {
            #revision
            #name
            #(#passthrough)*
        }

        impl #krate::migration::MigrationTrait for #self_ty {
            fn revision(&self) -> &'static str {
                Self::revision()
            }

            fn name(&self) -> &'static str {
                Self::name()
            }

            #up_method
            #down_method
            #transaction_method
        }
    })
}

/// Generates an `up`/`down` trait method that boxes the user's async body.
fn schema_method(
    krate: &TokenStream2,
    which: &str,
    method: &ImplItemFn,
) -> syn::Result<TokenStream2> {
    let ident = syn::Ident::new(which, method.sig.ident.span());
    let schema = schema_param_ident(method)?;
    let body = &method.block;

    Ok(quote! {
        fn #ident<'__tork_mig>(
            &'__tork_mig self,
            #schema: &'__tork_mig mut #krate::migration::SchemaManager<'_>,
        ) -> #krate::migration::BoxFuture<'__tork_mig, #krate::Result<()>> {
            ::std::boxed::Box::pin(async move #body)
        }
    })
}

/// Returns the identifier of the schema parameter of an `up`/`down` method.
fn schema_param_ident(method: &ImplItemFn) -> syn::Result<syn::Ident> {
    let first = method.sig.inputs.first().ok_or_else(|| {
        syn::Error::new_spanned(
            &method.sig,
            "a migration `up`/`down` takes a `&mut SchemaManager` parameter",
        )
    })?;
    let FnArg::Typed(typed) = first else {
        return Err(syn::Error::new_spanned(
            first,
            "a migration `up`/`down` must not take `self`",
        ));
    };
    let Pat::Ident(pat) = typed.pat.as_ref() else {
        return Err(syn::Error::new_spanned(
            &typed.pat,
            "the schema parameter must be a plain identifier",
        ));
    };
    Ok(pat.ident.clone())
}

/// Builds a "missing method" error.
fn missing(method: &str, input: &ItemImpl) -> syn::Error {
    syn::Error::new_spanned(input, format!("#[migration] requires a `{method}` method"))
}
