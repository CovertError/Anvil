//! Application builder. Mirrors Laravel 11's `bootstrap/app.rs`.

use std::net::SocketAddr;

use axum::Router as AxumRouter;
use tower_http::trace::TraceLayer;

use crate::container::{Container, ContainerBuilder};
use crate::middleware::{install_defaults, MiddlewareRegistry};
use crate::route::{RouteInfo, Router};
use crate::server_config::ServerConfig;
use crate::shutdown::ShutdownHandle;

pub struct Application {
    pub container: Container,
    pub registry: MiddlewareRegistry,
    pub web: AxumRouter<Container>,
    pub api: AxumRouter<Container>,
    pub shutdown: ShutdownHandle,
    pub server_config: ServerConfig,
    routes: Vec<RouteInfo>,
}

pub struct ApplicationBuilder {
    container_builder: ContainerBuilder,
    registry: MiddlewareRegistry,
    web_routes: Option<Box<dyn FnOnce(Router) -> Router>>,
    api_routes: Option<Box<dyn FnOnce(Router) -> Router>>,
    server_config: ServerConfig,
}

impl Default for ApplicationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationBuilder {
    pub fn new() -> Self {
        let registry = MiddlewareRegistry::new();
        install_defaults(&registry);
        Self {
            container_builder: ContainerBuilder::from_env(),
            registry,
            web_routes: None,
            api_routes: None,
            server_config: ServerConfig::default().apply_env_overrides(),
        }
    }

    pub fn container<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(ContainerBuilder) -> ContainerBuilder,
    {
        self.container_builder = configure(self.container_builder);
        self
    }

    pub fn middleware<F>(self, configure: F) -> Self
    where
        F: FnOnce(&MiddlewareRegistry),
    {
        configure(&self.registry);
        self
    }

    pub fn web<F>(mut self, build: F) -> Self
    where
        F: FnOnce(Router) -> Router + 'static,
    {
        self.web_routes = Some(Box::new(build));
        self
    }

    pub fn api<F>(mut self, build: F) -> Self
    where
        F: FnOnce(Router) -> Router + 'static,
    {
        self.api_routes = Some(Box::new(build));
        self
    }

    /// Set the production HTTP serving config (TLS, body limits, compression,
    /// rate limits, static file mounts, access logs).
    pub fn server_config(mut self, cfg: ServerConfig) -> Self {
        self.server_config = cfg;
        self
    }

    /// Load `config/anvil.toml` (or the given path) into the builder. Missing
    /// files are silently ignored — env-derived defaults still apply.
    pub fn server_config_file(mut self, path: impl AsRef<std::path::Path>) -> Self {
        self.server_config = ServerConfig::from_file_or_default(path);
        self
    }

    pub fn build(self) -> Application {
        let container = self.container_builder.build();
        let registry = self.registry;
        let server_config = self.server_config;

        let mut all_routes: Vec<RouteInfo> = Vec::new();

        let web_router = self.web_routes.map(|f| {
            let router = Router::new(registry.clone());
            let built = f(router);
            let (axum_router, routes) = built.finish();
            all_routes.extend(routes);
            axum_router
        });

        let api_router = self.api_routes.map(|f| {
            let router = Router::new(registry.clone()).prefix("/api");
            let built = f(router);
            let (axum_router, routes) = built.finish();
            all_routes.extend(routes);
            axum_router
        });

        Application {
            container,
            registry,
            web: web_router.unwrap_or_else(AxumRouter::new),
            api: api_router.unwrap_or_else(AxumRouter::new),
            shutdown: ShutdownHandle::new(),
            server_config,
            routes: all_routes,
        }
    }
}

impl Application {
    pub fn builder() -> ApplicationBuilder {
        ApplicationBuilder::new()
    }

    /// Every route registered against the app's web + api routers, in
    /// declaration order. Used by `anvil routes` to print a table.
    pub fn routes(&self) -> &[RouteInfo] {
        &self.routes
    }

    /// Combine web + api into a single state-applied router. Production layers
    /// (compression, body limits, rate limits, static files, access logs) are
    /// applied via `into_router_with_config`.
    pub fn into_router(self) -> AxumRouter {
        let cfg = self.server_config.clone();
        let combined = self.web.merge(self.api);
        let combined = crate::server::apply_layers(combined, &cfg);
        combined
            .layer(TraceLayer::new_for_http())
            .with_state(self.container.clone())
    }

    /// Run the app on the address taken from `server_config.bind`, honoring
    /// TLS, limits, compression, static files, and rate limits.
    ///
    /// This is the preferred entry point — `serve(addr)` is retained for
    /// backward compatibility but always serves plain HTTP.
    pub async fn run(self) -> Result<(), crate::Error> {
        let shutdown_handle = self.shutdown.clone().install();
        let cfg = self.server_config.clone();
        let container = self.container.clone();
        let combined = self.web.merge(self.api);
        let layered = crate::server::apply_layers(combined, &cfg)
            .layer(TraceLayer::new_for_http())
            .with_state(container);

        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            shutdown_handle.wait().await;
            let _ = tx.send(());
        });

        crate::server::serve(layered, &cfg, rx).await
    }

    /// Backward-compatible entry point: serve plain HTTP on `addr`, ignoring
    /// the server_config's bind address.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), crate::Error> {
        let mut cfg = self.server_config.clone();
        cfg.bind = addr.to_string();
        cfg.tls = None;
        let app_with_cfg = Application {
            server_config: cfg,
            ..self
        };
        app_with_cfg.run().await
    }
}
