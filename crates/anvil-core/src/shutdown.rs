//! Graceful shutdown signal handling for `smith serve` and queue workers.

use std::time::Duration;
use tokio::signal;
use tokio_util::sync::CancellationToken;

/// Returns a future that completes when the process receives SIGINT or SIGTERM.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C, beginning graceful shutdown"),
        _ = terminate => tracing::info!("received SIGTERM, beginning graceful shutdown"),
    }
}

/// A cancellation handle that subsystems (queue workers, schedulers) can subscribe to.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    token: CancellationToken,
}

impl Default for ShutdownHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownHandle {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn trigger(&self) {
        self.token.cancel();
    }

    pub fn is_shutdown(&self) -> bool {
        self.token.is_cancelled()
    }

    pub async fn wait(&self) {
        self.token.cancelled().await
    }

    /// Spawn the OS-signal listener; trigger this handle on SIGINT/SIGTERM.
    pub fn install(self) -> Self {
        let trigger = self.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            trigger.trigger();
        });
        self
    }
}

pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
