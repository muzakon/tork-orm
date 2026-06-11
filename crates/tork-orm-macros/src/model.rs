//! The `#[derive(Model)]` macro.
//!
//! Turns a struct into a database model by generating its table metadata, a
//! [`FromRow`] implementation that reads each field from its like-named column,
//! and the insert/primary-key value accessors the query layer uses.

use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::visit_mut::VisitMut;
use syn::{
    Data, DeriveInput, Expr, Fields, Ident, LitInt, LitStr, Path, PathSegment, Token,
    parse_macro_input,
};

use crate::common::{krate, option_inner, sql_type_for, to_snake};

/// Container options parsed from `#[table(...)]`.
#[derive(Default)]
struct TableArgs {
    name: Option<LitStr>,
    indexes: Vec<TableIndex>,
}

/// One entry of `#[table(indexes = [...])]`.
struct TableIndex {
    unique: bool,
    name: Option<LitStr>,
    fields: Vec<IndexFieldEntry>,
    predicate: Option<Expr>,
    method: Option<LitStr>,
    include: Vec<Ident>,
}

/// One entry inside an index's `fields = [...]` list: a plain column or an
/// `expr(...)` functional expression, with its optional ordering, null placement,
/// collation, and operator class.
struct IndexFieldEntry {
    target: FieldTarget,
    descending: bool,
    nulls: Option<NullsSetting>,
    collation: Option<LitStr>,
    opclass: Option<LitStr>,
}

/// What an index entry indexes.
enum FieldTarget {
    /// A model field, referenced by name.
    Column(Ident),
    /// A functional expression, such as `lower(email)`.
    Expression(Expr),
}

/// The `NULLS FIRST`/`NULLS LAST` placement requested for an entry.
#[derive(Clone, Copy)]
enum NullsSetting {
    First,
    Last,
}

impl Parse for TableArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = TableArgs::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "name" => {
                    input.parse::<Token![=]>()?;
                    args.name = Some(input.parse()?);
                }
                "indexes" => {
                    input.parse::<Token![=]>()?;
                    let content;
                    syn::bracketed!(content in input);
                    let entries: Punctuated<TableIndex, Token![,]> =
                        content.parse_terminated(TableIndex::parse, Token![,])?;
                    args.indexes = entries.into_iter().collect();
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown table option `{other}`"),
                    ));
                }
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(args)
    }
}

impl Parse for TableIndex {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let kind: Ident = input.parse()?;
        let unique = match kind.to_string().as_str() {
            "index" => false,
            "unique" => true,
            other => {
                return Err(syn::Error::new(
                    kind.span(),
                    format!("expected `index` or `unique`, found `{other}`"),
                ));
            }
        };

        let content;
        syn::parenthesized!(content in input);

        let mut index = TableIndex {
            unique,
            name: None,
            fields: Vec::new(),
            predicate: None,
            method: None,
            include: Vec::new(),
        };

        while !content.is_empty() {
            // `where` is a keyword, so it cannot be parsed as an identifier.
            if content.peek(Token![where]) {
                content.parse::<Token![where]>()?;
                content.parse::<Token![=]>()?;
                index.predicate = Some(content.parse()?);
            } else {
                let key: Ident = content.parse()?;
                match key.to_string().as_str() {
                    "name" => {
                        content.parse::<Token![=]>()?;
                        index.name = Some(content.parse()?);
                    }
                    "fields" => {
                        content.parse::<Token![=]>()?;
                        let inner;
                        syn::bracketed!(inner in content);
                        let entries: Punctuated<IndexFieldEntry, Token![,]> =
                            inner.parse_terminated(IndexFieldEntry::parse, Token![,])?;
                        index.fields = entries.into_iter().collect();
                    }
                    "using" => {
                        content.parse::<Token![=]>()?;
                        index.method = Some(content.parse()?);
                    }
                    "include" => {
                        content.parse::<Token![=]>()?;
                        let inner;
                        syn::bracketed!(inner in content);
                        let entries: Punctuated<Ident, Token![,]> =
                            inner.parse_terminated(Ident::parse, Token![,])?;
                        index.include = entries.into_iter().collect();
                    }
                    "expr" => {
                        return Err(syn::Error::new(
                            key.span(),
                            "use a per-column `expr(...)` entry inside `fields = [...]` \
                             instead of a top-level `expr`",
                        ));
                    }
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!("unknown index option `{other}`"),
                        ));
                    }
                }
            }
            if content.is_empty() {
                break;
            }
            content.parse::<Token![,]>()?;
        }

        if index.fields.is_empty() {
            return Err(syn::Error::new(
                kind.span(),
                "an index requires a non-empty `fields = [...]`",
            ));
        }
        Ok(index)
    }
}

impl Parse for IndexFieldEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // An `expr( <expression> [, modifier]* )` entry indexes an expression; a
        // bare ident (optionally followed by `( modifier... )`) indexes a column.
        // The two are told apart by the literal `expr` keyword before the parens.
        let is_expression = input.peek(Ident)
            && input.peek2(syn::token::Paren)
            && input
                .fork()
                .parse::<Ident>()
                .map(|ident| ident == "expr")
                .unwrap_or(false);

        if is_expression {
            let _keyword: Ident = input.parse()?;
            let inner;
            syn::parenthesized!(inner in input);
            let expression: Expr = inner.parse()?;
            let mut entry = IndexFieldEntry {
                target: FieldTarget::Expression(expression),
                descending: false,
                nulls: None,
                collation: None,
                opclass: None,
            };
            while inner.peek(Token![,]) {
                inner.parse::<Token![,]>()?;
                if inner.is_empty() {
                    break;
                }
                parse_index_modifier(&inner, &mut entry)?;
            }
            return Ok(entry);
        }

        let ident: Ident = input.parse()?;
        let mut entry = IndexFieldEntry {
            target: FieldTarget::Column(ident),
            descending: false,
            nulls: None,
            collation: None,
            opclass: None,
        };
        if input.peek(syn::token::Paren) {
            let inner;
            syn::parenthesized!(inner in input);
            while !inner.is_empty() {
                parse_index_modifier(&inner, &mut entry)?;
                if inner.is_empty() {
                    break;
                }
                inner.parse::<Token![,]>()?;
            }
        }
        Ok(entry)
    }
}

/// Parses one `asc`/`desc`/`nulls_first`/`nulls_last`/`opclass = ".."`/
/// `collate = ".."` modifier into `entry`.
fn parse_index_modifier(inner: ParseStream, entry: &mut IndexFieldEntry) -> syn::Result<()> {
    let modifier: Ident = inner.parse()?;
    match modifier.to_string().as_str() {
        "asc" => entry.descending = false,
        "desc" => entry.descending = true,
        "nulls_first" => entry.nulls = Some(NullsSetting::First),
        "nulls_last" => entry.nulls = Some(NullsSetting::Last),
        "opclass" => {
            inner.parse::<Token![=]>()?;
            entry.opclass = Some(inner.parse()?);
        }
        "collate" => {
            inner.parse::<Token![=]>()?;
            entry.collation = Some(inner.parse()?);
        }
        other => {
            return Err(syn::Error::new(
                modifier.span(),
                format!("unknown column modifier `{other}`"),
            ));
        }
    }
    Ok(())
}

/// Rewrites bare field identifiers in an index predicate into `Self::<ident>` so
/// they resolve to the generated `Column` constants. For example `status.eq(x)`
/// becomes `Self::status.eq(x)`. Anything that is not a single-segment path
/// matching a field name is left untouched.
struct FieldPathRewriter {
    fields: HashSet<String>,
}

impl VisitMut for FieldPathRewriter {
    fn visit_expr_path_mut(&mut self, node: &mut syn::ExprPath) {
        if node.qself.is_none()
            && node.path.leading_colon.is_none()
            && node.path.segments.len() == 1
        {
            let segment = &node.path.segments[0];
            if segment.arguments.is_empty() && self.fields.contains(&segment.ident.to_string()) {
                let ident = segment.ident.clone();
                node.path = syn::parse_quote!(Self::#ident);
            }
        }
        syn::visit_mut::visit_expr_path_mut(self, node);
    }
}

/// Per-field options parsed from `#[field(...)]`.
#[derive(Default)]
struct FieldArgs {
    primary_key: bool,
    auto: bool,
    unique: bool,
    /// `index` requests a single-column index; `index = false` on a foreign key
    /// suppresses its automatic index. `None` means the attribute was absent.
    index: Option<bool>,
    varchar_len: Option<u32>,
    foreign_key: Option<Path>,
    column: Option<String>,
}

impl Parse for FieldArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = FieldArgs::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let name = key.to_string();
            match name.as_str() {
                "primary_key" => args.primary_key = true,
                "auto" => args.auto = true,
                "unique" => args.unique = true,
                "index" => {
                    // Bare `index` enables a single-column index; `index = false`
                    // (or `= true`) toggles a foreign key's automatic index.
                    if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        let lit: syn::LitBool = input.parse()?;
                        args.index = Some(lit.value);
                    } else {
                        args.index = Some(true);
                    }
                }
                "varchar" => {
                    // varchar(length = N)
                    let content;
                    syn::parenthesized!(content in input);
                    let length_key: Ident = content.parse()?;
                    if length_key != "length" {
                        return Err(syn::Error::new(
                            length_key.span(),
                            "expected `length` inside `varchar(...)`",
                        ));
                    }
                    content.parse::<Token![=]>()?;
                    let lit: LitInt = content.parse()?;
                    args.varchar_len = Some(lit.base10_parse()?);
                }
                "foreign_key" => {
                    input.parse::<Token![=]>()?;
                    args.foreign_key = Some(input.parse()?);
                }
                "column" => {
                    input.parse::<Token![=]>()?;
                    let lit: LitStr = input.parse()?;
                    args.column = Some(lit.value());
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown field option `{other}`"),
                    ));
                }
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(args)
    }
}

/// Expands `#[derive(Model)]`.
pub fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_model(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_model(input: DeriveInput) -> syn::Result<TokenStream2> {
    let ident = &input.ident;

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(Model)] supports only structs",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(Model)] supports only structs with named fields",
        ));
    };

    // Resolve the table name from `#[table(name = "...")]`, defaulting to the
    // snake_case of the struct name.
    let mut table_args = TableArgs::default();
    for attr in &input.attrs {
        if attr.path().is_ident("table") {
            table_args = attr.parse_args()?;
        }
    }
    let table_name = table_args
        .name
        .map(|lit| lit.value())
        .unwrap_or_else(|| to_snake(&ident.to_string()));

    let krate = krate();

    let mut column_defs: Vec<TokenStream2> = Vec::new();
    let mut column_consts: Vec<TokenStream2> = Vec::new();
    let mut from_row_fields: Vec<TokenStream2> = Vec::new();
    let mut insert_entries: Vec<TokenStream2> = Vec::new();
    let mut primary_key: Option<(Ident, String)> = None;
    // Indexes built from the field-level attributes (`unique`/`index`) plus the
    // automatic index on each foreign key.
    let mut index_defs: Vec<TokenStream2> = Vec::new();
    // Maps a field identifier to its database column name (for table-level indexes
    // that reference fields by name) and collects the field names (for rewriting
    // partial-index predicates).
    let mut field_columns: Vec<(String, String)> = Vec::new();
    let mut field_names: HashSet<String> = HashSet::new();

    // A foreign key's automatic index is suppressed when a table-level index already
    // leads with that column, so collect those leading field names up front.
    let table_index_first_idents: HashSet<String> = table_args
        .indexes
        .iter()
        .filter_map(|index| index.fields.first())
        .filter_map(|entry| match &entry.target {
            FieldTarget::Column(ident) => Some(ident.to_string()),
            FieldTarget::Expression(_) => None,
        })
        .collect();

    for field in &named.named {
        let field_ident = field.ident.as_ref().expect("named field");
        let field_ty = &field.ty;

        let mut args = FieldArgs::default();
        for attr in &field.attrs {
            if attr.path().is_ident("field") {
                args = attr.parse_args()?;
            }
        }

        let column_name = args
            .column
            .clone()
            .unwrap_or_else(|| field_ident.to_string());
        field_columns.push((field_ident.to_string(), column_name.clone()));
        field_names.insert(field_ident.to_string());

        let nullable = option_inner(field_ty).is_some();
        let base_ty = option_inner(field_ty).unwrap_or(field_ty);
        let sql_type = match args.varchar_len {
            Some(length) => quote!(#krate::SqlType::Varchar(#length)),
            None => sql_type_for(base_ty),
        };

        let foreign_key = match &args.foreign_key {
            Some(path) => {
                let (ty_path, column) = split_foreign_key(path)?;
                let column_lit = LitStr::new(&column, path.segments.last().unwrap().ident.span());
                quote!(::core::option::Option::Some(#krate::ForeignKeyDef {
                    table: <#ty_path as #krate::Model>::TABLE,
                    column: #column_lit,
                }))
            }
            None => quote!(::core::option::Option::None),
        };

        let primary_key_flag = args.primary_key;
        let auto_flag = args.auto;

        // A `unique` field becomes a unique single-column index; a plain `index`
        // field becomes a non-unique one. A foreign key gets an automatic index
        // unless an explicit index already covers the column or `index = false`
        // opts out.
        let field_indexed = if args.unique {
            index_defs.push(field_index_tokens(&krate, &table_name, &column_name, true));
            true
        } else if args.index == Some(true) {
            index_defs.push(field_index_tokens(&krate, &table_name, &column_name, false));
            true
        } else {
            false
        };
        if args.foreign_key.is_some()
            && !field_indexed
            && args.index != Some(false)
            && !table_index_first_idents.contains(&field_ident.to_string())
        {
            index_defs.push(field_index_tokens(&krate, &table_name, &column_name, false));
        }

        column_defs.push(quote!(#krate::ColumnDef {
            name: #column_name,
            sql_type: #sql_type,
            primary_key: #primary_key_flag,
            auto: #auto_flag,
            nullable: #nullable,
            foreign_key: #foreign_key,
        }));

        // A typed column handle constant, used as `User::is_active`. Nullable
        // columns carry their inner type so they compare against plain values;
        // `is_null` covers the null case.
        column_consts.push(quote!(
            #[allow(non_upper_case_globals)]
            pub const #field_ident: #krate::Column<Self, #base_ty> =
                #krate::Column::new(#table_name, #column_name);
        ));

        from_row_fields.push(quote!(#field_ident: row.get(#column_name)?));

        // Auto-assigned columns are filled by the database, so they are not written.
        if !auto_flag {
            insert_entries.push(quote!(
                (#column_name, #krate::BindValue::to_value(&self.#field_ident))
            ));
        }

        if primary_key_flag {
            if primary_key.is_some() {
                return Err(syn::Error::new_spanned(
                    field_ident,
                    "a model may declare only one primary key column",
                ));
            }
            primary_key = Some((field_ident.clone(), column_name));
        }
    }

    let Some((pk_field, pk_column)) = primary_key else {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(Model)] requires one field marked `#[field(primary_key)]`",
        ));
    };

    // Table-level indexes from `#[table(indexes = [...])]`, appended after the
    // field-derived ones.
    for index in &table_args.indexes {
        index_defs.push(table_index_tokens(
            &krate,
            &table_name,
            index,
            &field_columns,
            &field_names,
        )?);
    }

    // Only override `Model::indexes` when the model actually declares some; an
    // empty list is identical to the trait default.
    let indexes_fn = if index_defs.is_empty() {
        quote!()
    } else {
        quote! {
            fn indexes() -> ::std::vec::Vec<#krate::IndexDef> {
                ::std::vec![ #(#index_defs),* ]
            }
        }
    };

    Ok(quote! {
        impl #ident {
            #(#column_consts)*
        }

        impl #krate::FromRow for #ident {
            fn from_row(row: &#krate::Row) -> #krate::Result<Self> {
                ::core::result::Result::Ok(Self {
                    #(#from_row_fields),*
                })
            }
        }

        impl #krate::Model for #ident {
            const TABLE: &'static str = #table_name;
            const COLUMNS: &'static [#krate::ColumnDef] = &[
                #(#column_defs),*
            ];
            const PRIMARY_KEY: &'static str = #pk_column;

            fn insert_values(&self) -> ::std::vec::Vec<(&'static str, #krate::Value)> {
                ::std::vec![
                    #(#insert_entries),*
                ]
            }

            fn primary_key_value(&self) -> #krate::Value {
                #krate::BindValue::to_value(&self.#pk_field)
            }

            #indexes_fn
        }
    })
}

/// Builds the tokens for a single-column [`IndexDef`] over `column`, auto-named
/// from the table and column.
fn field_index_tokens(
    krate: &TokenStream2,
    table: &str,
    column: &str,
    unique: bool,
) -> TokenStream2 {
    let name = auto_index_name(table, std::slice::from_ref(&column.to_string()), unique);
    quote!({
        let mut __index = #krate::IndexDef::new(#name);
        __index.columns = ::std::vec![ #krate::IndexColumn::new(#column) ];
        __index.unique = #unique;
        __index
    })
}

/// Builds the tokens for a table-level [`IndexDef`], resolving each referenced
/// field to its column name and lowering a partial-index predicate.
fn table_index_tokens(
    krate: &TokenStream2,
    table: &str,
    index: &TableIndex,
    field_columns: &[(String, String)],
    field_names: &HashSet<String>,
) -> syn::Result<TokenStream2> {
    let lookup = |ident: &Ident| -> syn::Result<String> {
        field_columns
            .iter()
            .find(|(name, _)| name == &ident.to_string())
            .map(|(_, column)| column.clone())
            .ok_or_else(|| {
                syn::Error::new(ident.span(), format!("unknown field `{ident}` in index"))
            })
    };

    let mut column_names: Vec<String> = Vec::new();
    let mut column_tokens: Vec<TokenStream2> = Vec::new();
    for entry in &index.fields {
        let mut tokens = match &entry.target {
            FieldTarget::Column(ident) => {
                let column = lookup(ident)?;
                column_names.push(column.clone());
                quote!(#krate::IndexColumn::new(#column))
            }
            FieldTarget::Expression(expression) => {
                // The bare field idents in the expression resolve to the generated
                // `Column` constants, the same lowering used for partial predicates.
                let mut rewritten = expression.clone();
                let mut rewriter = FieldPathRewriter {
                    fields: field_names.clone(),
                };
                rewriter.visit_expr_mut(&mut rewritten);
                column_names.push("expr".to_string());
                quote!(#krate::IndexColumn::expression(#rewritten))
            }
        };
        if entry.descending {
            tokens = quote!(#tokens.desc());
        }
        match entry.nulls {
            Some(NullsSetting::First) => tokens = quote!(#tokens.nulls_first()),
            Some(NullsSetting::Last) => tokens = quote!(#tokens.nulls_last()),
            None => {}
        }
        if let Some(collation) = &entry.collation {
            tokens = quote!(#tokens.collate(#collation));
        }
        if let Some(opclass) = &entry.opclass {
            tokens = quote!(#tokens.opclass(#opclass));
        }
        column_tokens.push(tokens);
    }

    let name = match &index.name {
        Some(lit) => lit.value(),
        None => auto_index_name(table, &column_names, index.unique),
    };
    let unique = index.unique;

    let predicate_assign = match &index.predicate {
        Some(expr) => {
            let mut rewritten = expr.clone();
            let mut rewriter = FieldPathRewriter {
                fields: field_names.clone(),
            };
            rewriter.visit_expr_mut(&mut rewritten);
            quote!(__index.predicate = ::core::option::Option::Some(#rewritten);)
        }
        None => quote!(),
    };

    let method_assign = match &index.method {
        Some(lit) => quote!(__index.method = ::core::option::Option::Some(#lit.to_string());),
        None => quote!(),
    };

    let mut include_columns: Vec<String> = Vec::new();
    for ident in &index.include {
        include_columns.push(lookup(ident)?);
    }
    let include_assign = if include_columns.is_empty() {
        quote!()
    } else {
        quote!(__index.include = ::std::vec![ #(#include_columns.to_string()),* ];)
    };

    Ok(quote!({
        let mut __index = #krate::IndexDef::new(#name);
        __index.columns = ::std::vec![ #(#column_tokens),* ];
        __index.unique = #unique;
        #predicate_assign
        #method_assign
        #include_assign
        __index
    }))
}

/// Auto-names an index from its table and column names: `<table>_<col...>_idx`,
/// or `<table>_<col...>_key` for a unique index.
fn auto_index_name(table: &str, columns: &[String], unique: bool) -> String {
    let mut name = String::from(table);
    for column in columns {
        name.push('_');
        name.push_str(column);
    }
    name.push_str(if unique { "_key" } else { "_idx" });
    name
}

/// Splits a `Type::column` foreign-key path into the referenced type path and the
/// column name. For example `User::id` yields (`User`, `"id"`).
fn split_foreign_key(path: &Path) -> syn::Result<(Path, String)> {
    let count = path.segments.len();
    if count < 2 {
        return Err(syn::Error::new_spanned(
            path,
            "foreign_key must be written as `Type::column`",
        ));
    }
    let column = path.segments[count - 1].ident.to_string();
    let mut segments: Punctuated<PathSegment, Token![::]> = Punctuated::new();
    for segment in path.segments.iter().take(count - 1) {
        segments.push(segment.clone());
    }
    let type_path = Path {
        leading_colon: path.leading_colon,
        segments,
    };
    Ok((type_path, column))
}
