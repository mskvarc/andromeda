//! Listener and graceful-drain configuration.

use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use thiserror::Error;
use toml_comment::TomlComment;

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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct ServerConfig {
    /// Listener address.
    #[toml_comment(inline)]
    pub host: IpAddr,
    /// Listener port.
    pub port: u16,
    /// Maximum time allowed for accepted connections to drain after shutdown begins.
    pub graceful_drain_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1056,
            graceful_drain_secs: 30,
        }
    }
}

impl ServerConfig {
    /// Validate the externally visible listener configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError::ZeroPort`] when the configured port is zero, which is
    /// reserved for dynamic binding and cannot be advertised as a broker listener.
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
