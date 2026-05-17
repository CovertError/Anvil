//! Application builder. Mirrors Laravel 11's `bootstrap/app.rs`.

use std::net::SocketAddr;

use axum::Router as AxumRouter;
use tower_http::trace::TraceLayer;

use crate::container::{Container, ContainerBuilder};
use crate::middleware::{install_defaults, MiddlewareRegistry};
use crate::route::Router;
use crate::shutdown::ShutdownHandle;

pub struct Application {
    pub container: Container,
    pub registry: MiddlewareRegistry,
    pub web: AxumRouter<Container>,
    pub api: AxumRouter<Container>,
    pub shutdown: ShutdownHandle,
}

pub struct ApplicationBuilder {
    container_builder: ContainerBuilder,
    registry: MiddlewareRegistry,
    web_routes: Option<Box<dyn FnOnce(Router) -> Router>>,
    api_routes: Option<Box<dyn FnOnce(Router) -> Router>>,
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

    pub fn build(self) -> Application {
        let container = self.container_builder.build();
        let registry = self.registry;

        let web_router = self.web_routes.map(|f| {
            let router = Router::new(registry.clone());
            f(router).with_state()
        });

        let api_router = self.api_routes.map(|f| {
            let router = Router::new(registry.clone()).prefix("/api");
            f(router).with_state()
        });

        Application {
            container,
            registry,
            web: web_router.unwrap_or_else(AxumRouter::new),
            api: api_router.unwrap_or_else(AxumRouter::new),
            shutdown: ShutdownHandle::new(),
        }
    }
}

impl Application {
    pub fn builder() -> ApplicationBuilder {
        ApplicationBuilder::new()
    }

    pub fn into_router(self) -> AxumRouter {
        let combined = self
            .web
            .merge(self.api)
            .layer(TraceLayer::new_for_http())
            .with_state(self.container.clone());
        combined
    }

    pub async fn serve(self, addr: SocketAddr) -> Result<(), crate::Error> {
        let shutdown = self.shutdown.clone().install();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(%addr, "anvil listening");

        let router = self.into_router();
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.wait().await })
            .await?;
        Ok(())
    }
}
