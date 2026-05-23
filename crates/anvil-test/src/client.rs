//! HTTP test client wrapping a `tower::Service` constructed from an `Application`.
//!
//! The API mirrors Laravel's Pest HTTP-testing surface: fluent `assert_*`
//! chains for status, headers, JSON, redirects, body content. Every assertion
//! returns `&Self` so you can stack them.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use anvil_core::Application;
use axum::body::{Body, Bytes};
use axum::Router;
use http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

/// Shared (name, value) cookie jar — `Arc<Mutex<_>>` so a `&self` request
/// can mutate the jar while the cookies-snapshot accessor reads it.
type CookieJar = Arc<Mutex<Vec<(String, String)>>>;

pub struct TestClient {
    router: Router,
    base_headers: HeaderMap,
    /// Cookie jar. `None` (the default) means no cookie handling — every
    /// request is independent, like the original behavior. Calling
    /// [`with_cookie_jar`] opts into persisting Set-Cookie across requests
    /// so multi-step flows (login → CSRF-protected POST, session lifecycle,
    /// etc.) work without manual header juggling.
    ///
    /// [`with_cookie_jar`]: TestClient::with_cookie_jar
    cookies: Option<CookieJar>,
}

impl TestClient {
    pub async fn new(app: Application) -> Self {
        // Belt-and-braces: every `*Config::from_env()` already triggers
        // `load_dotenv()` (idempotent via OnceLock), but a manually-wired
        // Application that never touches a typed config wouldn't. Forcing it
        // here means a test binary picks up `.env` even though it doesn't run
        // `main.rs` — fixing the "tests silently fall back to defaults" trap.
        let _ = anvil_core::config::load_dotenv();
        Self {
            router: app.into_router(),
            base_headers: HeaderMap::new(),
            cookies: None,
        }
    }

    pub fn from_router(router: Router) -> Self {
        let _ = anvil_core::config::load_dotenv();
        Self {
            router,
            base_headers: HeaderMap::new(),
            cookies: None,
        }
    }

    /// Turn on cookie persistence: `Set-Cookie` headers from each response are
    /// stashed and replayed on subsequent requests as a `Cookie:` header.
    /// Enables happy-path multi-step flow testing (login form → CSRF-protected
    /// POST, session lifecycle, etc.) without per-test header juggling.
    ///
    /// ```ignore
    /// let client = TestClient::new(app).await.with_cookie_jar();
    /// client.post_form("/login", &[("email", "a@b.com"), ("password", "...")]).await
    ///     .assert_redirect_to("/dashboard");
    /// client.get("/dashboard").await.assert_ok();  // session cookie carried through
    /// ```
    ///
    /// Cookie semantics are simplified: name/value pairs only — no
    /// `Path` / `Domain` / `Expires` / `Max-Age` honoring (tests run in
    /// sub-second windows where TTL doesn't matter; everything's same-host
    /// since it's all in-process). An empty value clears the cookie, matching
    /// the browser convention for `Set-Cookie: name=; Max-Age=0`.
    pub fn with_cookie_jar(mut self) -> Self {
        self.cookies = Some(Arc::new(Mutex::new(Vec::new())));
        self
    }

    /// Snapshot of the cookie jar's current contents. Useful for assertions
    /// like `assert!(client.cookies().iter().any(|(n, _)| n == "session_id"))`.
    /// Returns an empty Vec if cookie persistence wasn't enabled.
    pub fn cookies(&self) -> Vec<(String, String)> {
        self.cookies
            .as_ref()
            .map(|jar| jar.lock().unwrap().clone())
            .unwrap_or_default()
    }

    /// Wipe the cookie jar mid-test (e.g. to simulate a fresh browser
    /// session). No-op if cookie persistence wasn't enabled.
    pub fn clear_cookies(&self) {
        if let Some(jar) = &self.cookies {
            jar.lock().unwrap().clear();
        }
    }

    /// Attach a header to every subsequent request — e.g. `Authorization`.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        if let (Ok(name), Ok(val)) = (
            http::HeaderName::try_from(name),
            http::HeaderValue::try_from(value),
        ) {
            self.base_headers.insert(name, val);
        }
        self
    }

    /// Shortcut: set `Authorization: Bearer <token>` on every request.
    pub fn with_bearer(self, token: &str) -> Self {
        self.with_header("authorization", &format!("Bearer {token}"))
    }

    /// Shortcut: declare this is an AJAX request (matches Laravel's `->ajax()`).
    pub fn with_ajax(self) -> Self {
        self.with_header("x-requested-with", "XMLHttpRequest")
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        self.request(Method::GET, path, None, &[]).await
    }

    pub async fn post(&self, path: &str, body: serde_json::Value) -> TestResponse {
        self.request(Method::POST, path, Some(body), &[]).await
    }

    pub async fn put(&self, path: &str, body: serde_json::Value) -> TestResponse {
        self.request(Method::PUT, path, Some(body), &[]).await
    }

    pub async fn patch(&self, path: &str, body: serde_json::Value) -> TestResponse {
        self.request(Method::PATCH, path, Some(body), &[]).await
    }

    pub async fn delete(&self, path: &str) -> TestResponse {
        self.request(Method::DELETE, path, None, &[]).await
    }

    /// Send a form-urlencoded POST. Mirrors Laravel's `->post('/login', ['email' => ...])`.
    pub async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> TestResponse {
        let body = serde_urlencoded::to_string(form).unwrap_or_default();
        let req = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        self.send(req).await
    }

    /// Send a raw-bytes POST with an explicit `Content-Type`. Use for binary
    /// protocol endpoints (CBOR, protobuf, msgpack, etc.) — anything the JSON
    /// `post()` helper would mangle.
    pub async fn post_bytes(
        &self,
        path: &str,
        body: impl Into<Bytes>,
        content_type: &str,
    ) -> TestResponse {
        self.bytes_request(Method::POST, path, body.into(), content_type)
            .await
    }

    /// `post_bytes` for PUT.
    pub async fn put_bytes(
        &self,
        path: &str,
        body: impl Into<Bytes>,
        content_type: &str,
    ) -> TestResponse {
        self.bytes_request(Method::PUT, path, body.into(), content_type)
            .await
    }

    /// `post_bytes` for PATCH.
    pub async fn patch_bytes(
        &self,
        path: &str,
        body: impl Into<Bytes>,
        content_type: &str,
    ) -> TestResponse {
        self.bytes_request(Method::PATCH, path, body.into(), content_type)
            .await
    }

    async fn bytes_request(
        &self,
        method: Method,
        path: &str,
        body: Bytes,
        content_type: &str,
    ) -> TestResponse {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap();
        self.send(req).await
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
        extra_headers: &[(&str, &str)],
    ) -> TestResponse {
        let mut req = Request::builder().method(method).uri(path);
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&v).unwrap())
            }
            None => Body::empty(),
        };
        for (n, v) in extra_headers {
            req = req.header(*n, *v);
        }
        let mut http_req = req.body(body).unwrap();
        for (name, value) in &self.base_headers {
            http_req.headers_mut().insert(name.clone(), value.clone());
        }
        self.send(http_req).await
    }

    async fn send(&self, req: Request<Body>) -> TestResponse {
        let mut req = req;
        for (name, value) in &self.base_headers {
            req.headers_mut()
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        // Inject the cookie jar contents as a single `Cookie:` header. We
        // overwrite any caller-supplied `Cookie:` header — multi-step flows
        // want the jar to be authoritative.
        if let Some(jar) = &self.cookies {
            let cookies = jar.lock().unwrap();
            if !cookies.is_empty() {
                let joined = cookies
                    .iter()
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                if let Ok(val) = http::HeaderValue::from_str(&joined) {
                    req.headers_mut().insert("cookie", val);
                }
            }
        }

        let response = self.router.clone().oneshot(req).await.unwrap();

        // Update the jar from `Set-Cookie`. Multiple `Set-Cookie` headers can
        // appear in a single response — handle each.
        if let Some(jar) = &self.cookies {
            let mut cookies = jar.lock().unwrap();
            for raw in response.headers().get_all("set-cookie").iter() {
                let Ok(s) = raw.to_str() else { continue };
                // Just the name=value pair — attribute spec (Path, Domain, etc.)
                // is ignored for simplicity.
                let pair = s.split(';').next().unwrap_or(s);
                let Some((name, value)) = pair.split_once('=') else {
                    continue;
                };
                let name = name.trim().to_string();
                let value = value.trim().to_string();
                // Empty value is the browser-deletion convention.
                cookies.retain(|(n, _)| n != &name);
                if !value.is_empty() {
                    cookies.push((name, value));
                }
            }
        }

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map(|c| c.to_bytes())
            .unwrap_or_default();

        TestResponse {
            status,
            headers,
            body: bytes.to_vec(),
        }
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    /// Raw response body bytes — binary-safe. Prefer the [`body_bytes`] /
    /// [`body_text`] accessors over reading this field directly so test code
    /// reads symmetric with the other helpers.
    ///
    /// [`body_bytes`]: TestResponse::body_bytes
    /// [`body_text`]: TestResponse::body_text
    pub body: Vec<u8>,
}

impl TestResponse {
    // ─── Status helpers ────────────────────────────────────────────────────

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

    pub fn assert_created(&self) -> &Self {
        self.assert_status(201)
    }
    pub fn assert_no_content(&self) -> &Self {
        self.assert_status(204)
    }
    pub fn assert_bad_request(&self) -> &Self {
        self.assert_status(400)
    }
    pub fn assert_unauthorized(&self) -> &Self {
        self.assert_status(401)
    }
    pub fn assert_forbidden(&self) -> &Self {
        self.assert_status(403)
    }
    pub fn assert_not_found(&self) -> &Self {
        self.assert_status(404)
    }
    pub fn assert_unprocessable(&self) -> &Self {
        self.assert_status(422)
    }
    pub fn assert_too_many_requests(&self) -> &Self {
        self.assert_status(429)
    }
    pub fn assert_server_error(&self) -> &Self {
        assert!(
            self.status.is_server_error(),
            "expected 5xx, got {} — body: {}",
            self.status,
            self.body_text()
        );
        self
    }

    pub fn assert_redirect(&self) -> &Self {
        assert!(
            self.status.is_redirection(),
            "expected 3xx redirect, got {} — body: {}",
            self.status,
            self.body_text()
        );
        self
    }

    pub fn assert_redirect_to(&self, location: &str) -> &Self {
        self.assert_redirect();
        let actual = self
            .headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(actual, location, "redirect Location mismatch");
        self
    }

    // ─── Header helpers ────────────────────────────────────────────────────

    pub fn assert_header(&self, name: &str, value: &str) -> &Self {
        let actual = self
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(actual, value, "header `{name}` mismatch");
        self
    }

    pub fn assert_header_present(&self, name: &str) -> &Self {
        assert!(
            self.headers.contains_key(name),
            "expected header `{name}` to be present"
        );
        self
    }

    pub fn assert_header_missing(&self, name: &str) -> &Self {
        assert!(
            !self.headers.contains_key(name),
            "expected header `{name}` NOT to be present"
        );
        self
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok().map(String::from))
    }

    // ─── Body helpers ──────────────────────────────────────────────────────

    /// Raw response body — binary-safe. Use this for CBOR / protobuf /
    /// msgpack / anything else that isn't UTF-8 text. The [`body_text`]
    /// accessor lossy-decodes via `String::from_utf8_lossy` and replaces
    /// invalid sequences with `U+FFFD`, which silently corrupts binary
    /// payloads and breaks downstream decoders.
    ///
    /// [`body_text`]: TestResponse::body_text
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// Assert the raw body equals `expected` byte-for-byte. Use for binary
    /// protocols where `assert_body` (UTF-8) would mangle the comparison.
    pub fn assert_body_bytes(&self, expected: impl AsRef<[u8]>) -> &Self {
        let expected = expected.as_ref();
        assert_eq!(
            self.body.as_slice(),
            expected,
            "body byte mismatch — got {} bytes, expected {} bytes",
            self.body.len(),
            expected.len()
        );
        self
    }

    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).expect("response was not valid JSON")
    }

    pub fn json_value(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }

    pub fn assert_contains(&self, needle: &str) -> &Self {
        let body = self.body_text();
        assert!(
            body.contains(needle),
            "expected response body to contain '{needle}', got: {body}"
        );
        self
    }
    pub fn assert_dont_contain(&self, needle: &str) -> &Self {
        let body = self.body_text();
        assert!(
            !body.contains(needle),
            "expected response body NOT to contain '{needle}', got: {body}"
        );
        self
    }
    /// Laravel-style alias for `assert_contains`.
    pub fn assert_see(&self, text: &str) -> &Self {
        self.assert_contains(text)
    }
    pub fn assert_dont_see(&self, text: &str) -> &Self {
        self.assert_dont_contain(text)
    }

    // ─── JSON helpers ──────────────────────────────────────────────────────

    /// Assert the response is JSON and equals the given value.
    pub fn assert_json(&self, expected: serde_json::Value) -> &Self {
        let actual = self.json_value();
        assert_eq!(actual, expected, "JSON body mismatch");
        self
    }

    /// Assert a dot-path inside the JSON body equals `expected`.
    /// Example: `assert_json_path("data.user.name", json!("Alice"))`.
    pub fn assert_json_path(&self, path: &str, expected: serde_json::Value) -> &Self {
        let actual = json_dig(&self.json_value(), path);
        assert_eq!(
            actual.as_ref(),
            Some(&expected),
            "JSON path `{path}` mismatch — full body: {}",
            self.body_text()
        );
        self
    }

    /// Assert the JSON body contains every key/value in `subset` (recursive
    /// partial match — extra fields are ignored).
    pub fn assert_json_fragment(&self, subset: serde_json::Value) -> &Self {
        let actual = self.json_value();
        assert!(
            json_contains(&actual, &subset),
            "JSON body missing fragment {subset} — got {actual}"
        );
        self
    }

    /// Assert the JSON body's `errors.<field>` array contains an error.
    /// Pairs with Anvilforge's validation error shape.
    pub fn assert_validation_error(&self, field: &str) -> &Self {
        let v = self.json_value();
        let arr = v
            .get("errors")
            .and_then(|e| e.get(field))
            .and_then(|f| f.as_array());
        assert!(
            arr.map(|a| !a.is_empty()).unwrap_or(false),
            "expected validation error on field `{field}` — body: {}",
            self.body_text()
        );
        self
    }
}

/// Recursive partial-match: every leaf in `expected` must equal the same path
/// in `actual`. Extra keys in `actual` are fine.
fn json_contains(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    use serde_json::Value::*;
    match (actual, expected) {
        (Object(a), Object(e)) => e
            .iter()
            .all(|(k, ev)| a.get(k).is_some_and(|av| json_contains(av, ev))),
        (Array(a), Array(e)) => e.iter().all(|ev| a.iter().any(|av| json_contains(av, ev))),
        (a, e) => a == e,
    }
}

/// Dot-path lookup: `"data.user.0.name"` walks objects and arrays.
fn json_dig(v: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = v;
    for segment in path.split('.') {
        current = if let Ok(idx) = segment.parse::<usize>() {
            current.get(idx)?
        } else {
            current.get(segment)?
        };
    }
    Some(current.clone())
}

// Silence unused-import lint when only used through trait bounds.
fn _force_link() {
    let _ = std::any::type_name::<Infallible>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;

    /// Echo a raw request body back as the response body. Exercises both the
    /// prelude-re-exported `Bytes` extractor and the new `post_bytes` client.
    async fn echo(body: Bytes) -> Bytes {
        body
    }

    #[tokio::test]
    async fn post_bytes_round_trips_arbitrary_bytes() {
        let router = Router::new().route("/echo", post(echo));
        let client = TestClient::from_router(router);

        // Real-world payload shape: a 7-byte CBOR map { "ok": true }.
        let cbor = vec![0xA1, 0x62, 0x6F, 0x6B, 0xF5];
        let resp = client
            .post_bytes("/echo", cbor.clone(), "application/cbor")
            .await;

        resp.assert_ok();
        assert_eq!(resp.body, cbor);
    }

    #[tokio::test]
    async fn post_bytes_sets_content_type_header_for_handler_dispatch() {
        // Handler that returns `Content-Type` from the request, to prove the
        // client actually set it correctly.
        async fn ct(headers: http::HeaderMap) -> String {
            headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("missing")
                .to_string()
        }
        let router = Router::new().route("/ct", post(ct));
        let client = TestClient::from_router(router);

        let resp = client
            .post_bytes("/ct", b"x".to_vec(), "application/x-protobuf")
            .await;
        resp.assert_ok();
        assert_eq!(resp.body_text(), "application/x-protobuf");
    }

    #[tokio::test]
    async fn body_bytes_preserves_non_utf8_payload() {
        // Bytes that are not valid UTF-8 — body_text() would replace these
        // with U+FFFD and silently break a downstream CBOR/protobuf decoder.
        // body_bytes() must return them verbatim.
        async fn binary() -> Vec<u8> {
            vec![0xFF, 0xFE, 0xFD, 0x00, 0x80, 0xC0]
        }
        let router = Router::new().route("/bin", axum::routing::get(binary));
        let client = TestClient::from_router(router);

        let resp = client.get("/bin").await;
        resp.assert_ok();

        // assert_body_bytes catches the regression directly.
        resp.assert_body_bytes([0xFF, 0xFE, 0xFD, 0x00, 0x80, 0xC0]);
        assert_eq!(resp.body_bytes(), &[0xFF, 0xFE, 0xFD, 0x00, 0x80, 0xC0]);

        // body_text() is intentionally lossy — confirm the contrast so future
        // refactors don't accidentally remove the binary-safe accessor.
        let text = resp.body_text();
        assert!(text.contains('\u{FFFD}'), "body_text lossy-decodes");
    }

    #[tokio::test]
    async fn put_and_patch_bytes_dispatch_correctly() {
        async fn method_name(method: Method) -> String {
            method.as_str().to_string()
        }
        let router = Router::new()
            .route("/m", axum::routing::put(method_name))
            .route("/m", axum::routing::patch(method_name));
        let client = TestClient::from_router(router);

        let resp = client
            .put_bytes("/m", b"_".to_vec(), "application/octet-stream")
            .await;
        resp.assert_ok();
        assert_eq!(resp.body_text(), "PUT");

        let resp = client
            .patch_bytes("/m", b"_".to_vec(), "application/octet-stream")
            .await;
        resp.assert_ok();
        assert_eq!(resp.body_text(), "PATCH");
    }

    #[tokio::test]
    async fn cookie_jar_persists_set_cookie_across_requests() {
        use axum::http::HeaderMap;
        use axum::response::Response;
        use axum::routing::get;

        async fn set_cookie() -> Response {
            Response::builder()
                .status(200)
                .header("set-cookie", "session_id=abc123; Path=/")
                .body(axum::body::Body::from("set"))
                .unwrap()
        }

        async fn read_cookie(headers: HeaderMap) -> String {
            headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("(none)")
                .to_string()
        }

        let router = Router::new()
            .route("/login", get(set_cookie))
            .route("/me", get(read_cookie));
        let client = TestClient::from_router(router).with_cookie_jar();

        let r1 = client.get("/login").await;
        r1.assert_ok();

        let r2 = client.get("/me").await;
        r2.assert_ok();
        assert_eq!(r2.body_text(), "session_id=abc123");

        // Jar accessor exposes the stored pair.
        let snap = client.cookies();
        assert_eq!(snap, vec![("session_id".to_string(), "abc123".to_string())]);
    }

    #[tokio::test]
    async fn cookie_jar_replaces_same_name_and_deletes_on_empty_value() {
        use axum::response::Response;
        use axum::routing::get;

        async fn rotate() -> Response {
            Response::builder()
                .status(200)
                .header("set-cookie", "session_id=v2")
                .body(axum::body::Body::from(""))
                .unwrap()
        }

        async fn delete() -> Response {
            Response::builder()
                .status(200)
                .header("set-cookie", "session_id=; Max-Age=0")
                .body(axum::body::Body::from(""))
                .unwrap()
        }

        let router = Router::new()
            .route("/rotate", get(rotate))
            .route("/logout", get(delete));
        let client = TestClient::from_router(router).with_cookie_jar();

        client.get("/rotate").await.assert_ok();
        assert_eq!(client.cookies(), vec![("session_id".into(), "v2".into())]);

        // Rotation: same name, new value → existing entry replaced, not duplicated.
        client.get("/rotate").await.assert_ok();
        assert_eq!(client.cookies(), vec![("session_id".into(), "v2".into())]);

        // Empty value = delete (browser convention for cookie expiration).
        client.get("/logout").await.assert_ok();
        assert!(client.cookies().is_empty());
    }

    #[tokio::test]
    async fn cookie_jar_off_by_default_does_not_carry_state() {
        use axum::http::HeaderMap;
        use axum::response::Response;
        use axum::routing::get;

        async fn set_cookie() -> Response {
            Response::builder()
                .status(200)
                .header("set-cookie", "x=1")
                .body(axum::body::Body::from(""))
                .unwrap()
        }
        async fn read_cookie(headers: HeaderMap) -> String {
            headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("(none)")
                .to_string()
        }

        let router = Router::new()
            .route("/set", get(set_cookie))
            .route("/read", get(read_cookie));
        let client = TestClient::from_router(router); // no .with_cookie_jar()

        client.get("/set").await.assert_ok();
        let r2 = client.get("/read").await;
        // Without the jar, the second request should NOT carry the cookie.
        assert_eq!(r2.body_text(), "(none)");
        assert!(client.cookies().is_empty());
    }
}
