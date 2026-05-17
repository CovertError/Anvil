//! Form request + validation. Wraps garde.

use axum::async_trait;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::header;
use garde::Validate;
use serde::de::DeserializeOwned;

use crate::Error;

/// An extractor that deserializes the body (JSON or form-urlencoded)
/// and runs `garde::Validate` on the result before returning the typed struct.
pub struct ValidatedForm<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<S> for ValidatedForm<T>
where
    T: DeserializeOwned + Validate + Send + 'static,
    T::Context: Default,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let content_type = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| Error::bad_request(e.to_string()))?;

        let value: T = if content_type.starts_with("application/json") {
            serde_json::from_slice(&bytes).map_err(|e| Error::bad_request(e.to_string()))?
        } else if content_type.starts_with("application/x-www-form-urlencoded") {
            serde_urlencoded::from_bytes(&bytes).map_err(|e| Error::bad_request(e.to_string()))?
        } else if bytes.is_empty() {
            return Err(Error::bad_request("empty request body"));
        } else {
            // Try JSON as fallback for unspecified content types
            serde_json::from_slice(&bytes).map_err(|e| Error::bad_request(e.to_string()))?
        };

        value.validate_with(&Default::default())?;
        Ok(ValidatedForm(value))
    }
}
