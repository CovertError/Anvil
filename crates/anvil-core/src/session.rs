//! Session subsystem. Thin wrapper around tower-sessions.

pub use tower_sessions::{Session, SessionManagerLayer, MemoryStore};
pub use tower_sessions::cookie::SameSite;

use crate::config::SessionConfig;

pub fn build_layer(config: &SessionConfig) -> SessionManagerLayer<MemoryStore> {
    let store = MemoryStore::default();
    SessionManagerLayer::new(store)
        .with_name(config.cookie_name.clone())
        .with_secure(config.secure)
        .with_same_site(match config.same_site.as_str() {
            "strict" => SameSite::Strict,
            "none" => SameSite::None,
            _ => SameSite::Lax,
        })
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            tower_sessions::cookie::time::Duration::seconds(config.lifetime_minutes * 60),
        ))
}

pub const FLASH_KEY: &str = "_flash";
pub const ERRORS_KEY: &str = "_errors";

/// Insert a flash message — read by the next request, then cleared.
pub async fn flash(session: &Session, key: &str, value: serde_json::Value) -> Result<(), crate::Error> {
    let mut flashes: std::collections::HashMap<String, serde_json::Value> = session
        .get(FLASH_KEY)
        .await
        .map_err(|e| crate::Error::Internal(e.to_string()))?
        .unwrap_or_default();
    flashes.insert(key.to_string(), value);
    session
        .insert(FLASH_KEY, flashes)
        .await
        .map_err(|e| crate::Error::Internal(e.to_string()))?;
    Ok(())
}

pub async fn take_flash(session: &Session, key: &str) -> Result<Option<serde_json::Value>, crate::Error> {
    let mut flashes: std::collections::HashMap<String, serde_json::Value> = session
        .get(FLASH_KEY)
        .await
        .map_err(|e| crate::Error::Internal(e.to_string()))?
        .unwrap_or_default();
    let value = flashes.remove(key);
    session
        .insert(FLASH_KEY, flashes)
        .await
        .map_err(|e| crate::Error::Internal(e.to_string()))?;
    Ok(value)
}
