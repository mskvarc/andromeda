//! Listener and graceful-drain configuration.

use confique::Config;
use serde::Serialize;
use std::{net::IpAddr, time::Duration};
use thiserror::Error;

/// Invalid listener configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ServerConfigError {
    /// TCP port zero is reserved for dynamic binding and is not accepted for broker execution.
    #[error("server.port must be greater than 0")]
    ZeroPort,
}

/// Command-line listener overrides applied above other configuration layers.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerOverrides {
    /// Optional listener address override.
    pub host: Option<IpAddr>,
    /// Optional listener port override.
    pub port: Option<u16>,
}

/// Address and graceful-drain policy for inbound HTTP execution.
#[derive(Config, Debug, Clone, Serialize)]
pub struct ServerConfig {
    /// Listener address.
    #[config(env = "ANDROMEDA_SERVER_HOST", default = "0.0.0.0")]
    pub host: IpAddr,
    /// Listener port.
    #[config(env = "ANDROMEDA_SERVER_PORT", default = 1056)]
    pub port: u16,
    /// Maximum time allowed for accepted connections to drain after shutdown begins.
    #[config(env = "ANDROMEDA_SERVER_GRACEFUL_DRAIN_SECS", default = 30)]
    pub graceful_drain_secs: u64,
}

impl ServerConfig {
    /// Validate the externally visible listener configuration.
    pub const fn validate(&self) -> Result<(), ServerConfigError> {
        if self.port == 0 { Err(ServerConfigError::ZeroPort) } else { Ok(()) }
    }

    /// Return the configured graceful-drain deadline.
    #[must_use]
    pub const fn graceful_drain_deadline(&self) -> Duration {
        Duration::from_secs(self.graceful_drain_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::{ServerConfig, ServerConfigError};
    use std::net::IpAddr;

    #[test]
    fn zero_listener_port_is_rejected() {
        let config = ServerConfig {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 0,
            graceful_drain_secs: 30,
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroPort));
    }
}
