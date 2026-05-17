//! Cast derive macros. `#[derive(Model)]` generates the trait impl, columns
//! accessor, FromRow impl, and inherent helpers. Attribute macros for relationships.

mod model;
mod relation;

use proc_macro::TokenStream;

#[proc_macro_derive(Model, attributes(table, primary_key, has_many, has_one, belongs_to, soft_deletes))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match model::expand(&input) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
