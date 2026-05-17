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
}

impl Router {
    pub fn new(registry: MiddlewareRegistry) -> Self {
        Self {
            inner: AxumRouter::new(),
            registry,
            middleware_stack: Vec::new(),
            prefix: String::new(),
        }
    }

    pub fn with_state(self) -> AxumRouter<Container> {
        self.inner
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
        };
        let built = build(inner_router);
        self.inner = self.inner.merge(built.inner);
        self
    }

    pub fn merge(mut self, other: Router) -> Self {
        self.inner = self.inner.merge(other.inner);
        self
    }

    pub fn nest(mut self, prefix: &str, other: Router) -> Self {
        self.inner = self.inner.nest(prefix, other.inner);
        self
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
