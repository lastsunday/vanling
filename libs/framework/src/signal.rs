use std::sync::Arc;

use async_trait::async_trait;
use tokio::signal;

#[async_trait]
pub trait SignalHandler: Send + Sync {
    async fn reload(&self) -> Result<(), anyhow::Error>;
    async fn shutdown(&self) -> Result<(), anyhow::Error>;
    async fn signal(&self, sig: &'static str) -> Result<(), anyhow::Error>;
}

#[cfg(unix)]
#[tracing::instrument(skip_all, level = "info")]
pub async fn handle_signals(handler: Arc<dyn SignalHandler>) {
    use signal::unix;
    use unix::SignalKind;

    let mut quit = unix::signal(SignalKind::quit()).expect("SIGQUIT handler");
    let mut term = unix::signal(SignalKind::terminate()).expect("SIGTERM handler");
    let mut usr1 = unix::signal(SignalKind::user_defined1()).expect("SIGUSR1 handler");
    let mut usr2 = unix::signal(SignalKind::user_defined2()).expect("SIGUSR2 handler");
    loop {
        use tracing::{trace, warn};

        trace!("Installed signal handlers");
        let sig: &'static str;
        tokio::select! {
            _ = signal::ctrl_c() => { sig = "SIGINT"; },
            _ = quit.recv() => { sig = "SIGQUIT"; },
            _ = term.recv() => { sig = "SIGTERM"; },
            _ = usr1.recv() => { sig = "SIGUSR1"; },
            _ = usr2.recv() => { sig = "SIGUSR2"; },
        }

        warn!("Received {sig}");
        let result = if matches!(sig, "SIGQUIT" | "SIGTERM") || (sig == "SIGINT") {
            handler.shutdown().await
        } else {
            handler.signal(sig).await
        };

        if let Err(e) = result {
            use tracing::error;

            error!(%sig, "signal: {e}");
        }
    }
}

#[cfg(not(unix))]
#[tracing::instrument(skip_all, level = "info")]
pub async fn handle_signals(handler: Arc<dyn SignalHandler>) {
    loop {
        use tracing::{debug_error, warn};

        tokio::select! {
            _ = signal::ctrl_c() => {
                warn!("Received Ctrl+C");
                if let Err(e) = handler.signal("SIGINT").await {
                    debug_error!("signal: {e}");
                }
            },
        }
    }
}
