//! Application bootstrap. Wires middleware, routes, services.

use anvil_core::Application;
use anvilforge::prelude::*;

use crate::routes;

pub async fn build(container: Container) -> anyhow::Result<Application> {
    // Force-link the Spark components so their `#[spark_component]` inventory
    // submissions are included in the binary. Without referring to the module
    // here, the linker would drop it as dead code.
    let _force_link = std::any::type_name::<crate::app::spark::Counter>();

    let app = Application::builder()
        .container(|_b| {
            anvil_core::container::ContainerBuilder::from_env().driver_pool(container.driver_pool())
        })
        .middleware(|registry| {
            registry.register(
                "require_auth",
                crate::routes::middleware::require_auth_passthrough,
            );
        })
        .web(::spark::install(routes::web::register))
        .api(routes::api::register)
        .server_config_file("config/anvil.toml")
        .build();

    Ok(app)
}
