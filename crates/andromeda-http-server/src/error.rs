//! Typed listener and connection-serving failures.

use std::{io, net::SocketAddr, time::Duration};

/// Failures owned by generic HTTP server execution.
#[derive(Debug, thiserror::Error)]
pub enum HttpServerError {
    /// The configured listener address could not be bound.
    #[error("failed to bind HTTP listener at {address}")]
    Bind {
        /// Address requested by composition.
        address: SocketAddr,
        /// Operating-system listener failure.
        #[source]
        source: io::Error,
    },
    /// The bound listener's effective local address could not be read.
    #[error("failed to read the bound HTTP listener address")]
    LocalAddress {
        /// Operating-system socket failure.
        #[source]
        source: io::Error,
    },
    /// Accepted connections did not drain before the configured deadline.
    #[error("HTTP connections did not drain within {deadline:?}")]
    DrainDeadlineExceeded {
        /// Configured maximum drain duration.
        deadline: Duration,
    },
}
