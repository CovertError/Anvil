//! Expansion of `#[derive(Model)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields};

use crate::relation::{collect_relations, RelationDecl};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let struct_name = &input.ident;
    let vis = &input.vis;

    let table_name = extract_table_name(input)?;
    let pk_column = extract_pk_column(input).unwrap_or_else(|| "id".to_string());

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => named.named.iter().collect::<Vec<_>>(),
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Model derive only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "Model derive only supports structs",
            ));
        }
    };

    let column_names: Vec<String> = fields
        .iter()
        .map(|f| f.ident.as_ref().unwrap().to_string())
        .collect();

    let columns_struct_name = format_ident!("{}Columns", struct_name);

    let column_accessors = fields.iter().map(|f| {
        let ident = f.ident.as_ref().unwrap();
        let col_name = ident.to_string();
        let ty = &f.ty;
        quote! {
            pub fn #ident(&self) -> ::anvilforge::cast::Column<#struct_name, #ty> {
                ::anvilforge::cast::Column::new(#col_name)
            }
        }
    });

    let pk_field_ident = fields
        .iter()
        .find(|f| f.ident.as_ref().unwrap() == &pk_column)
        .map(|f| f.ident.clone().unwrap())
        .ok_or_else(|| {
            syn::Error::new_spanned(
                input,
                format!("primary key field '{pk_column}' not found in struct"),
            )
        })?;

    let pk_field_type = fields
        .iter()
        .find(|f| f.ident.as_ref().unwrap() == &pk_column)
        .map(|f| f.ty.clone())
        .unwrap();

    let columns_array = column_names
        .iter()
        .map(|n| quote!(#n))
        .collect::<Vec<_>>();

    let relations = collect_relations(input)?;
    let relation_methods = relations
        .iter()
        .map(|r| expand_relation(struct_name, &pk_field_ident, r));
    let relation_types = relations.iter().map(|r| relation_type_decl(struct_name, r));

    let table_lit = syn::LitStr::new(&table_name, struct_name.span());
    let pk_lit = syn::LitStr::new(&pk_column, struct_name.span());

    let from_row_fields = fields.iter().map(|f| {
        let ident = f.ident.as_ref().unwrap();
        let col_name = ident.to_string();
        quote! {
            #ident: row.try_get(#col_name)?,
        }
    });

    let output = quote! {
        impl ::anvilforge::cast::Model for #struct_name {
            type PrimaryKey = #pk_field_type;
            const TABLE: &'static str = #table_lit;
            const PK_COLUMN: &'static str = #pk_lit;
            const COLUMNS: &'static [&'static str] = &[#(#columns_array),*];

            fn primary_key(&self) -> &Self::PrimaryKey {
                &self.#pk_field_ident
            }
        }

        #[doc(hidden)]
        #vis struct #columns_struct_name;

        impl #columns_struct_name {
            #(#column_accessors)*
        }

        impl #struct_name {
            pub fn columns() -> #columns_struct_name {
                #columns_struct_name
            }

            #(#relation_methods)*
        }

        #(#relation_types)*

        impl<'r> ::anvilforge::cast::sqlx::FromRow<'r, ::anvilforge::cast::sqlx::postgres::PgRow> for #struct_name {
            fn from_row(row: &'r ::anvilforge::cast::sqlx::postgres::PgRow) -> ::anvilforge::cast::sqlx::Result<Self> {
                use ::anvilforge::cast::sqlx::Row as _;
                Ok(Self {
                    #(#from_row_fields)*
                })
            }
        }
    };

    Ok(output)
}

fn extract_table_name(input: &DeriveInput) -> syn::Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("table") {
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                return Ok(lit.value());
            }
        }
    }
    let struct_name = input.ident.to_string();
    Ok(pluralize_snake_case(&struct_name))
}

fn extract_pk_column(input: &DeriveInput) -> Option<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("primary_key") {
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                return Some(lit.value());
            }
        }
    }
    None
}

fn pluralize_snake_case(s: &str) -> String {
    let mut snake = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(ch.to_ascii_lowercase());
    }
    if snake.ends_with('s') {
        snake
    } else if snake.ends_with('y') {
        snake.pop();
        snake.push_str("ies");
        snake
    } else {
        snake.push('s');
        snake
    }
}

fn relation_type_decl(parent: &syn::Ident, rel: &RelationDecl) -> TokenStream {
    let rel_type_name = format_ident!("{}{}Rel", parent, capitalize(&rel.method_name));
    let parent_ident = parent.clone();
    let child = &rel.target;
    let local_key = &rel.local_key;
    let foreign_key = &rel.foreign_key;
    let kind = &rel.kind_token;

    quote! {
        #[doc(hidden)]
        pub struct #rel_type_name;

        impl ::anvilforge::cast::RelationDef for #rel_type_name {
            type Parent = #parent_ident;
            type Child = #child;
            type Kind = ::anvilforge::cast::#kind;
            fn local_key() -> &'static str { #local_key }
            fn foreign_key() -> &'static str { #foreign_key }
        }
    }
}

fn expand_relation(
    _parent: &syn::Ident,
    pk_field: &syn::Ident,
    rel: &RelationDecl,
) -> TokenStream {
    let method = format_ident!("{}", rel.method_name);
    let rel_method = format_ident!("{}_rel", rel.method_name);
    let rel_type_name = format_ident!("{}{}Rel", _parent, capitalize(&rel.method_name));
    let child = &rel.target;
    let foreign_key = &rel.foreign_key;
    let foreign_key_field = syn::Ident::new(foreign_key, proc_macro2::Span::call_site());

    match rel.kind.as_str() {
        "HasMany" | "HasOne" => {
            // For has_many / has_one: parent's PK value is the local value;
            // we filter the child table by `foreign_key = self.pk_field`.
            let load_method = if rel.kind == "HasMany" {
                quote! { pub async fn #method(&self, pool: &::anvilforge::cast::Pool) -> ::anvilforge::cast::Result<Vec<#child>> {
                    use ::anvilforge::cast::Model as _;
                    #child::query()
                        .where_eq(#child::columns().#foreign_key_field(), self.#pk_field.clone())
                        .get(pool).await
                }}
            } else {
                quote! { pub async fn #method(&self, pool: &::anvilforge::cast::Pool) -> ::anvilforge::cast::Result<Option<#child>> {
                    use ::anvilforge::cast::Model as _;
                    #child::query()
                        .where_eq(#child::columns().#foreign_key_field(), self.#pk_field.clone())
                        .first(pool).await
                }}
            };

            quote! {
                pub fn #rel_method() -> #rel_type_name {
                    #rel_type_name
                }

                #load_method
            }
        }
        "BelongsTo" => {
            // For belongs_to: this struct holds a FK column that points at the child's PK.
            // `foreign_key` here names the local FK field.
            quote! {
                pub fn #rel_method() -> #rel_type_name {
                    #rel_type_name
                }

                pub async fn #method(&self, pool: &::anvilforge::cast::Pool) -> ::anvilforge::cast::Result<Option<#child>> {
                    use ::anvilforge::cast::Model as _;
                    #child::find(pool, self.#foreign_key_field.clone()).await
                }
            }
        }
        _ => quote! {},
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
