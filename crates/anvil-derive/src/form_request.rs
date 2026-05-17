//! `#[derive(FormRequest)]` — generates an Axum extractor wrapping `ValidatedForm<Self>`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_g, ty_g, where_g) = input.generics.split_for_impl();

    Ok(quote! {
        // Re-export the FromRequest extractor by wrapping in ValidatedForm.
        #[::async_trait::async_trait]
        impl #impl_g ::axum::extract::FromRequest<::anvil_core::Container> for #name #ty_g
            #where_g
        {
            type Rejection = ::anvil_core::Error;

            async fn from_request(
                req: ::axum::extract::Request,
                state: &::anvil_core::Container,
            ) -> ::std::result::Result<Self, Self::Rejection> {
                let ::anvil_core::validation::ValidatedForm(value) =
                    ::anvil_core::validation::ValidatedForm::<Self>::from_request(req, state)
                        .await?;
                Ok(value)
            }
        }
    })
}
