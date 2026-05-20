//! Facade-style ambient helpers.
//!
//! Inside a request task, these resolve the request-scoped `Container`
//! installed by `inject_container_mw` and hand you the underlying service.
//! Outside a request (e.g. in `main.rs` before the server starts) they
//! panic — use `Container` directly in that case.
//!
//! Laravel's `Cache::put()`, `Mail::to()`, `DB::connection()` etc. work this
//! way: an ambient container resolves the right concrete implementation
//! without each call site having to plumb a `$container` reference.
//! Anvilforge's version is opt-in — handlers that take `State<Container>`
//! work just as well, and the explicit signature is recommended in
//! library code.

use crate::container::{self, Container};

/// The current request's container. Panics outside a request task.
pub fn app() -> Container {
    container::current()
}

/// Default DB driver pool. `let users = User::query().get(&db()).await?;`
///
/// Multi-driver friendly: returns the `cast::Pool` enum that the user's
/// `DATABASE_URL` resolved to.
pub fn db() -> cast_core::Pool {
    container::current().driver_pool()
}

/// Default cache store.
pub fn cache() -> crate::cache::CacheStore {
    container::current().cache().clone()
}

/// Default queue.
pub fn queue() -> crate::queue::QueueHandle {
    container::current().queue().clone()
}

/// Default mailer.
pub fn mailer() -> crate::mail::MailerHandle {
    container::current().mailer().clone()
}

/// Storage manager.
pub fn storage() -> crate::storage::StorageManager {
    container::current().storage().clone()
}

/// Event bus.
pub fn events() -> crate::event::EventBus {
    container::current().events().clone()
}

/// Application config (`APP_NAME`, `APP_ENV`, `APP_KEY`, `APP_URL`, …).
pub fn config() -> crate::config::AppConfig {
    container::current().app().clone()
}
