//! Listener and graceful-drain configuration.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid listener configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ServerConfigError {
    /// TCP port zero is reserved for dynamic binding and is not accepted for broker execution.
    #[error("server.port must be greater than 0")]
    ZeroPort,
}

/// What a TCP listener speaks on the wire.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.2 requires the API to support HTTP over TLS
/// and calls plaintext HTTP "not recommended". A listener either terminates
/// that TLS itself, from the `[tls]` identity, or serves plaintext behind a
/// reverse proxy that terminates it (`docs/tls-deployment.md`); it never does
/// both on one port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum ListenerTransport {
    /// Plaintext HTTP/1.1 and HTTP/2, for deployment behind a TLS terminator.
    #[default]
    Plaintext,
    /// HTTP/1.1 and HTTP/2 over TLS terminated in process with the `[tls]` identity.
    Tls,
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
    /// Whether the listener serves plaintext or terminates TLS: `plaintext` or `tls`.
    #[toml_comment(inline)]
    pub transport: ListenerTransport,
    /// Maximum time allowed for accepted connections to drain after shutdown
    /// begins, in human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub graceful_drain: HumanDuration,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1056,
            transport: ListenerTransport::Plaintext,
            graceful_drain: HumanDuration::from_secs(30),
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
        self.graceful_drain.duration()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{ListenerTransport, ServerConfig, ServerConfigError};
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;
    use std::{error::Error, net::IpAddr, str::FromStr};

    #[test]
    fn zero_listener_port_is_rejected() {
        let config = ServerConfig {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 0,
            transport: ListenerTransport::Plaintext,
            graceful_drain: HumanDuration::from_secs(30),
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroPort));
    }

    #[test]
    fn transport_defaults_to_plaintext() {
        assert_eq!(ServerConfig::default().transport, ListenerTransport::Plaintext);
    }

    #[test]
    fn transport_round_trips_kebab_case() -> Result<(), Box<dyn Error>> {
        for (transport, token) in [(ListenerTransport::Plaintext, "plaintext"), (ListenerTransport::Tls, "tls")] {
            assert_eq!(transport.to_string(), token);
            assert_eq!(ListenerTransport::from_str(token)?, transport);
            let rendered = toml::to_string(&ServerConfig {
                transport,
                ..ServerConfig::default()
            })?;
            assert!(rendered.contains(&format!("transport = \"{token}\"")), "{rendered}");
            assert_eq!(toml::from_str::<ServerConfig>(&rendered)?.transport, transport);
        }
        Ok(())
    }
}
