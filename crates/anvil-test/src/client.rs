//! HTTP test client wrapping a tower::Service constructed from an Application.

use std::convert::Infallible;

use anvil_core::Application;
use axum::body::Body;
use axum::Router;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

pub struct TestClient {
    router: Router,
}

impl TestClient {
    pub async fn new(app: Application) -> Self {
        Self {
            router: app.into_router(),
        }
    }

    pub fn from_router(router: Router) -> Self {
        Self { router }
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        self.request(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: serde_json::Value) -> TestResponse {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: serde_json::Value) -> TestResponse {
        self.request(Method::PUT, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> TestResponse {
        self.request(Method::DELETE, path, None).await
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> TestResponse {
        let mut req = Request::builder().method(method).uri(path);
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&v).unwrap())
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map(|c| c.to_bytes())
            .unwrap_or_default();

        TestResponse {
            status,
            body: bytes.to_vec(),
        }
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

impl TestResponse {
    pub fn assert_status(&self, expected: u16) -> &Self {
        assert_eq!(
            self.status.as_u16(),
            expected,
            "expected status {expected}, got {} — body: {}",
            self.status,
            self.body_text()
        );
        self
    }

    pub fn assert_ok(&self) -> &Self {
        assert!(
            self.status.is_success(),
            "expected success, got {} — body: {}",
            self.status,
            self.body_text()
        );
        self
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).expect("response was not valid JSON")
    }

    pub fn assert_contains(&self, needle: &str) -> &Self {
        let body = self.body_text();
        assert!(
            body.contains(needle),
            "expected response body to contain '{needle}', got: {body}"
        );
        self
    }
}

// Silence unused import warning in deps-only mode.
fn _force_link() {
    let _ = std::any::type_name::<Infallible>();
}
