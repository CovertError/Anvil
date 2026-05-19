//! `#[derive(Seeder)]` — registers a seeder struct with the inventory collector
//! and adds a fn-bridge into the `Seeder` trait method.
//!
//! Typical use:
//!
//! ```ignore
//! #[derive(Seeder)]
//! pub struct RolesSeeder;
//!
//! #[async_trait]
//! impl Seeder for RolesSeeder {
//!     async fn run(&self, c: &Container) -> Result<()> { ... }
//! }
//! ```
//!
//! The derive adds an `inventory::submit!` entry. `SeederRegistry::from_inventory()`
//! discovers every registered seeder; `smith db:seed --class=RolesSeeder` resolves
//! by struct name via the registry.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let name_str = name.to_string();

    Ok(quote! {
        ::anvilforge::inventory::submit! {
            ::anvilforge::seeder::SeederRegistration {
                name: #name_str,
                builder: || -> ::std::sync::Arc<dyn ::anvilforge::seeder::Seeder> {
                    ::std::sync::Arc::new(#name)
                },
            }
        }

        impl #name {
            pub const SEEDER_NAME: &'static str = #name_str;
        }
    })
}
