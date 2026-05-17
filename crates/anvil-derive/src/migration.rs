//! `#[derive(Migration)]` — registers a migration struct with the inventory collector.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let name_str = name.to_string();

    Ok(quote! {
        ::anvil_core::inventory::submit! {
            ::anvil_core::cast_core::migration::MigrationRegistration {
                builder: || -> ::std::boxed::Box<dyn ::anvil_core::cast_core::Migration> {
                    ::std::boxed::Box::new(#name)
                },
            }
        }

        impl #name {
            pub const MIGRATION_NAME: &'static str = #name_str;
        }
    })
}
