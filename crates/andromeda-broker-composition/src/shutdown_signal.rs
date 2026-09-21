//! Waiting for the process shutdown signal (Ctrl-C or, on Unix, SIGTERM).

use std::io::Error as IoError;

/// Wait for Ctrl-C or SIGTERM, whichever arrives first.
pub(crate) async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::{
            ctrl_c,
            unix::{SignalKind, signal},
        };

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = ctrl_c() => report_ctrl_c(result),
                    received = terminate.recv() => report_sigterm(received),
                }
            }
            Err(error) => {
                tracing::error!(error = %error, "SIGTERM signal listener failed; waiting for Ctrl-C");
                report_ctrl_c(ctrl_c().await);
            }
        }
    }

    #[cfg(not(unix))]
    {
        report_ctrl_c(tokio::signal::ctrl_c().await);
    }
}

/// Report the terminal result of the Ctrl-C listener.
fn report_ctrl_c(result: Result<(), IoError>) {
    match result {
        Ok(()) => tracing::info!(signal = "ctrl-c", event = "shutdown_signal_received"),
        Err(error) => tracing::error!(error = %error, "Ctrl-C signal listener failed"),
    }
}

/// Report whether the SIGTERM stream produced a signal before closing.
#[cfg(unix)]
fn report_sigterm(received: Option<()>) {
    if let Some(()) = received {
        tracing::info!(signal = "sigterm", event = "shutdown_signal_received");
    } else {
        tracing::error!("SIGTERM signal stream ended before receiving a signal");
    }
}
