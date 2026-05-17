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
            pub async fn dispatch(self) -> ::anvilforge::Result<()> {
                let container = ::anvilforge::container::current();
                let data = ::anvilforge::serde_json::to_value(&self)
                    .map_err(|e| ::anvilforge::Error::Internal(e.to_string()))?;
                ::anvilforge::queue::dispatch_payload(&container, #name_str, data).await
            }

            pub async fn dispatch_with(self, container: &::anvilforge::Container) -> ::anvilforge::Result<()> {
                let data = ::anvilforge::serde_json::to_value(&self)
                    .map_err(|e| ::anvilforge::Error::Internal(e.to_string()))?;
                ::anvilforge::queue::dispatch_payload(container, #name_str, data).await
            }
        }

        fn #runner_fn() -> ::anvilforge::queue::JobRunner {
            ::std::sync::Arc::new(
                |container: &::anvilforge::Container,
                 payload: &::anvilforge::queue::QueuePayload|
                 -> ::anvilforge::futures::future::BoxFuture<'_, ::anvilforge::Result<()>> {
                    Box::pin(async move {
                        let job: #name = ::anvilforge::serde_json::from_value(payload.data.clone())
                            .map_err(|e| ::anvilforge::Error::Queue(e.to_string()))?;
                        job.handle(container).await
                    })
                },
            )
        }

        ::anvilforge::inventory::submit! {
            ::anvilforge::queue::JobRegistration {
                name: #name_str,
                runner: #runner_fn,
            }
        }
    })
}
