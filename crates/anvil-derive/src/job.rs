//! `#[derive(Job)]` — generates `dispatch()` + registers a runner with `inventory`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let name_str = name.to_string();
    let runner_fn = format_ident!("__anvil_job_runner_{}", name);

    Ok(quote! {
        impl #name {
            pub async fn dispatch(self) -> ::anvil_core::Result<()> {
                let container = ::anvil_core::container::current();
                let data = ::anvil_core::serde_json::to_value(&self)
                    .map_err(|e| ::anvil_core::Error::Internal(e.to_string()))?;
                ::anvil_core::queue::dispatch_payload(&container, #name_str, data).await
            }

            pub async fn dispatch_with(self, container: &::anvil_core::Container) -> ::anvil_core::Result<()> {
                let data = ::anvil_core::serde_json::to_value(&self)
                    .map_err(|e| ::anvil_core::Error::Internal(e.to_string()))?;
                ::anvil_core::queue::dispatch_payload(container, #name_str, data).await
            }
        }

        fn #runner_fn() -> ::anvil_core::queue::JobRunner {
            ::std::sync::Arc::new(
                |container: &::anvil_core::Container,
                 payload: &::anvil_core::queue::QueuePayload|
                 -> ::anvil_core::futures::future::BoxFuture<'_, ::anvil_core::Result<()>> {
                    Box::pin(async move {
                        let job: #name = ::anvil_core::serde_json::from_value(payload.data.clone())
                            .map_err(|e| ::anvil_core::Error::Queue(e.to_string()))?;
                        job.handle(container).await
                    })
                },
            )
        }

        ::anvil_core::inventory::submit! {
            ::anvil_core::queue::JobRegistration {
                name: #name_str,
                runner: #runner_fn,
            }
        }
    })
}
