//! Request extras — re-exports and helpers around Axum extractors.

pub use axum::extract::{Form, Json, Path, Query, State};
pub use axum::http::{HeaderMap, Method, StatusCode, Uri};

use axum::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;

use crate::container::Container;
use crate::Error;

/// An extractor that yields the container reference, with `?`-friendly error.
pub struct App(pub Container);

#[async_trait]
impl<S> FromRequestParts<S> for App
where
    Container: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(App(Container::from_ref(state)))
    }
}

// The blanket `impl<T: Clone> FromRef<T> for T` in axum_core covers
// `FromRef<Container> for Container` already; we don't need an explicit impl.
