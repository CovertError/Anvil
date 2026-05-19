//! Routing DSL — a thin Laravel-shaped layer over Axum's `Router`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request};
use axum::routing::{any, delete, get, patch, post, put, MethodRouter};
use axum::Router as AxumRouter;

use crate::container::Container;
use crate::middleware::MiddlewareRegistry;

pub struct Router {
    inner: AxumRouter<Container>,
    registry: MiddlewareRegistry,
    middleware_stack: Vec<String>,
    prefix: String,
    routes: Vec<RouteInfo>,
}

/// Description of a single registered route. Captured at registration time so
/// `Application::routes()` (and `anvil routes`) can list them without poking
/// at Axum's internals.
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub method: Method,
    pub path: String,
    pub middleware: Vec<String>,
}

impl Router {
    pub fn new(registry: MiddlewareRegistry) -> Self {
        Self {
            inner: AxumRouter::new(),
            registry,
            middleware_stack: Vec::new(),
            prefix: String::new(),
            routes: Vec::new(),
        }
    }

    /// Borrow the route registry collected during construction.
    pub fn route_infos(&self) -> &[RouteInfo] {
        &self.routes
    }

    pub fn with_state(self) -> AxumRouter<Container> {
        self.inner
    }

    /// Same as `with_state`, but also returns the captured `RouteInfo` list so
    /// the Application can hold onto it for `anvil routes`.
    pub fn finish(self) -> (AxumRouter<Container>, Vec<RouteInfo>) {
        (self.inner, self.routes)
    }

    fn record(&mut self, method: Method, path: &str) {
        self.routes.push(RouteInfo {
            method,
            path: self.full_path(path),
            middleware: self.middleware_stack.clone(),
        });
    }

    fn full_path(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}{}", self.prefix.trim_end_matches('/'), path)
        }
    }

    fn wrap_method_router(&self, mr: MethodRouter<Container>) -> MethodRouter<Container> {
        let mut mr = mr;
        for name in self.middleware_stack.iter().rev() {
            if let Some(mw) = self.registry.get(name) {
                let mw = mw.clone();
                let layer = axum::middleware::from_fn(move |req: Request<Body>, next: axum::middleware::Next| {
                    let mw = mw.clone();
                    async move {
                        crate::middleware::invoke(mw, req, next).await
                    }
                });
                mr = mr.layer(layer);
            } else {
                tracing::warn!(name, "unknown middleware referenced in route; ignoring");
            }
        }
        mr
    }

    pub fn get<H, T>(mut self, path: &str, handler: H) -> Self
    where
        H: axum::handler::Handler<T, Container>,
        T: 'static,
    {
        self.record(Method::GET, path);
        let mr = self.wrap_method_router(get(handler));
        let full = self.full_path(path);
        self.inner = self.inner.route(&full, mr);
        self
    }

    pub fn post<H, T>(mut self, path: &str, handler: H) -> Self
    where
        H: axum::handler::Handler<T, Container>,
        T: 'static,
    {
        self.record(Method::POST, path);
        let mr = self.wrap_method_router(post(handler));
        let full = self.full_path(path);
        self.inner = self.inner.route(&full, mr);
        self
    }

    pub fn put<H, T>(mut self, path: &str, handler: H) -> Self
    where
        H: axum::handler::Handler<T, Container>,
        T: 'static,
    {
        self.record(Method::PUT, path);
        let mr = self.wrap_method_router(put(handler));
        let full = self.full_path(path);
        self.inner = self.inner.route(&full, mr);
        self
    }

    pub fn patch<H, T>(mut self, path: &str, handler: H) -> Self
    where
        H: axum::handler::Handler<T, Container>,
        T: 'static,
    {
        self.record(Method::PATCH, path);
        let mr = self.wrap_method_router(patch(handler));
        let full = self.full_path(path);
        self.inner = self.inner.route(&full, mr);
        self
    }

    pub fn delete<H, T>(mut self, path: &str, handler: H) -> Self
    where
        H: axum::handler::Handler<T, Container>,
        T: 'static,
    {
        self.record(Method::DELETE, path);
        let mr = self.wrap_method_router(delete(handler));
        let full = self.full_path(path);
        self.inner = self.inner.route(&full, mr);
        self
    }

    pub fn any<H, T>(mut self, path: &str, handler: H) -> Self
    where
        H: axum::handler::Handler<T, Container>,
        T: 'static,
    {
        self.record(Method::OPTIONS, path); // sentinel; "any" → display as OPTIONS
        let mr = self.wrap_method_router(any(handler));
        let full = self.full_path(path);
        self.inner = self.inner.route(&full, mr);
        self
    }

    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn middleware<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for name in names {
            self.middleware_stack.push(name.into());
        }
        self
    }

    pub fn group<F>(mut self, build: F) -> Self
    where
        F: FnOnce(Router) -> Router,
    {
        let inner_router = Router {
            inner: AxumRouter::new(),
            registry: self.registry.clone(),
            middleware_stack: self.middleware_stack.clone(),
            prefix: self.prefix.clone(),
            routes: Vec::new(),
        };
        let built = build(inner_router);
        self.routes.extend(built.routes);
        self.inner = self.inner.merge(built.inner);
        self
    }

    pub fn merge(mut self, other: Router) -> Self {
        self.routes.extend(other.routes);
        self.inner = self.inner.merge(other.inner);
        self
    }

    pub fn nest(mut self, prefix: &str, other: Router) -> Self {
        for mut r in other.routes {
            r.path = format!("{}{}", prefix.trim_end_matches('/'), r.path);
            self.routes.push(r);
        }
        self.inner = self.inner.nest(prefix, other.inner);
        self
    }

    /// Replace the captured `RouteInfo` list. Used by `Router::adopt` when
    /// pulling in routes whose metadata is tracked elsewhere.
    pub fn with_route_infos(mut self, infos: Vec<RouteInfo>) -> Self {
        self.routes.extend(infos);
        self
    }

    /// Apply a tower layer to every route on this router. Used by extension
    /// crates (e.g. Spark's `spark.scope` per-request middleware) to wrap all
    /// user routes without each one having to opt in by name.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: tower::Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
        L::Service: tower::Service<axum::http::Request<axum::body::Body>, Response = axum::http::Response<axum::body::Body>, Error = std::convert::Infallible>
            + Clone
            + Send
            + 'static,
        <L::Service as tower::Service<axum::http::Request<axum::body::Body>>>::Future: Send + 'static,
    {
        self.inner = self.inner.layer(layer);
        self
    }

    /// Adopt a raw `axum::Router<Container>` — useful when a crate has already
    /// built its routes with its own layered stack and just wants to merge them in.
    pub fn adopt(self, other: AxumRouter<Container>) -> Self {
        Router {
            inner: self.inner.merge(other),
            registry: self.registry,
            middleware_stack: self.middleware_stack,
            prefix: self.prefix,
            routes: self.routes,
        }
    }
}

/// A single named route declaration — mostly for `route!()` macros and named-route URL generation.
#[derive(Debug, Clone)]
pub struct Route {
    pub name: Option<String>,
    pub method: Method,
    pub path: String,
}

/// Named-route registry for URL generation: `route::url("posts.show", [42])`.
#[derive(Default, Clone)]
pub struct NamedRoutes {
    routes: Arc<parking_lot::RwLock<indexmap::IndexMap<String, Route>>>,
}

impl NamedRoutes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, route: Route) {
        if let Some(name) = route.name.clone() {
            self.routes.write().insert(name, route);
        }
    }

    pub fn url(&self, name: &str, params: &[&str]) -> Option<String> {
        let routes = self.routes.read();
        let route = routes.get(name)?;
        let mut path = route.path.clone();
        for p in params {
            if let Some(start) = path.find('{') {
                if let Some(end) = path[start..].find('}') {
                    path.replace_range(start..=start + end, p);
                }
            }
        }
        Some(path)
    }
}
