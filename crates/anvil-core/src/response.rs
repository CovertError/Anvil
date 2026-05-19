//! Response builders. View rendering, redirects, JSON helpers.

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A response that carries a Forge-rendered view body.
pub struct ViewResponse {
    pub status: StatusCode,
    pub body: String,
    pub headers: HeaderMap,
}

impl ViewResponse {
    pub fn new(body: impl Into<String>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        Self {
            status: StatusCode::OK,
            body: body.into(),
            headers,
        }
    }

    pub fn status(mut self, s: StatusCode) -> Self {
        self.status = s;
        self
    }
}

impl IntoResponse for ViewResponse {
    fn into_response(self) -> Response {
        (self.status, self.headers, self.body).into_response()
    }
}

/// A redirect response.
pub struct Redirect {
    pub status: StatusCode,
    pub location: String,
    pub flash: Option<(String, String)>,
}

impl Redirect {
    pub fn to(location: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SEE_OTHER,
            location: location.into(),
            flash: None,
        }
    }

    pub fn back() -> Self {
        Self::to("/")
    }

    pub fn permanent(location: impl Into<String>) -> Self {
        Self {
            status: StatusCode::MOVED_PERMANENTLY,
            location: location.into(),
            flash: None,
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.flash = Some((key.into(), value.into()));
        self
    }
}

impl IntoResponse for Redirect {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::LOCATION,
            HeaderValue::from_str(&self.location).unwrap_or_else(|_| HeaderValue::from_static("/")),
        );
        (self.status, headers).into_response()
    }
}

/// Trait for "smart" responders — types that know how to render themselves.
/// Implemented for the common types so handlers can `Ok(json(...))` etc.
pub trait Responder {
    fn respond(self) -> Response;
}

impl<T: IntoResponse> Responder for T {
    fn respond(self) -> Response {
        self.into_response()
    }
}

pub fn json<T: Serialize>(value: T) -> axum::Json<T> {
    axum::Json(value)
}

pub fn no_content() -> Response {
    (StatusCode::NO_CONTENT).into_response()
}
