//! Typed listener and connection-serving failures.

use andromeda_configuration::validate::config_key::ConfigKey;
use humantime::format_duration;
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
    ///
    /// The message names the deadline and the key that sets it.
    #[error(
        "HTTP connections did not drain within {} ({})",
        format_duration(*.deadline),
        ConfigKey::ServerGracefulDrainTimeout
    )]
    DrainDeadlineExceeded {
        /// Configured maximum drain duration.
        deadline: Duration,
    },
}

#[cfg(test)]
mod tests {
    use crate::error::HttpServerError;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// 90 seconds reads `1m 30s`, which a `Debug` interpolation spells `90s`.
    #[test]
    fn a_missed_drain_deadline_names_the_deadline_and_the_key_that_sets_it() {
        for (deadline, message) in [
            (
                Duration::from_secs(30),
                "HTTP connections did not drain within 30s (server.graceful_drain_timeout)",
            ),
            (
                Duration::from_secs(90),
                "HTTP connections did not drain within 1m 30s (server.graceful_drain_timeout)",
            ),
        ] {
            assert_eq!(HttpServerError::DrainDeadlineExceeded { deadline }.to_string(), message);
        }
    }
}
