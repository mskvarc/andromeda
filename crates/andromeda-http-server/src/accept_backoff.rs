//! Riding out a listener-level accept failure without ending the accept loop.

use std::{io, time::Duration};
use tokio::time::sleep;

/// How long the accept loop backs off after a listener-level failure, such as
/// running out of file descriptors, before trying again.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// Treat a listener-level accept failure the way axum's own TCP listener does.
///
/// A connection that was reset or aborted before it could be accepted is the
/// peer's business and is skipped. Anything else, typically `EMFILE`, is
/// logged and waited out, in the hope that closing connections frees what the
/// listener needs.
pub(crate) async fn back_off_after(error: io::Error) {
    // `io::ErrorKind` is non-exhaustive, so the skipped kinds are tested for
    // rather than matched exhaustively.
    if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset
    ) {
        return;
    }
    tracing::error!(event = "listener_accept_failed", error = %error, "backing off before accepting again");
    sleep(ACCEPT_ERROR_BACKOFF).await;
}
