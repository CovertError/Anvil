//! `#[derive(FormRequest)]` — generates an Axum extractor wrapping `ValidatedForm<Self>`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_g, ty_g, where_g) = input.generics.split_for_impl();

    Ok(quote! {
        // Re-export the FromRequest extractor by wrapping in ValidatedForm.
        #[::anvilforge::async_trait::async_trait]
        impl #impl_g ::anvilforge::axum::extract::FromRequest<::anvilforge::Container> for #name #ty_g
            #where_g
        {
            type Rejection = ::anvilforge::Error;

            async fn from_request(
                req: ::anvilforge::axum::extract::Request,
                state: &::anvilforge::Container,
            ) -> ::std::result::Result<Self, Self::Rejection> {
                let ::anvilforge::validation::ValidatedForm(value) =
                    ::anvilforge::validation::ValidatedForm::<Self>::from_request(req, state)
                        .await?;
                Ok(value)
            }
        }
    })
}
