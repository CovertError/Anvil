//! `#[derive(Migration)]` — registers a migration struct with the inventory collector.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let name_str = name.to_string();

    Ok(quote! {
        ::anvilforge::inventory::submit! {
            ::anvilforge::cast::migration::MigrationRegistration {
                builder: || -> ::std::boxed::Box<dyn ::anvilforge::cast::Migration> {
                    ::std::boxed::Box::new(#name)
                },
            }
        }

        impl #name {
            pub const MIGRATION_NAME: &'static str = #name_str;
        }
    })
}
