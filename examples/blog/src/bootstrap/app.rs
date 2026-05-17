//! Application bootstrap. Wires middleware, routes, services.

use anvil::prelude::*;
use anvil_core::Application;

use crate::routes;

pub async fn build(container: Container) -> anyhow::Result<Application> {
    // Build a registry pre-populated with framework defaults.
    let app = Application::builder()
        .container(|_b| {
            // The container is already constructed by main; we use the supplied one
            // indirectly via the closure below.
            anvil_core::container::ContainerBuilder::from_env()
                .pool(container.pool().clone())
        })
        .middleware(|registry| {
            registry.register("require_auth", crate::routes::middleware::require_auth_passthrough);
        })
        .web(routes::web::register)
        .api(routes::api::register)
        .build();

    Ok(app)
}
