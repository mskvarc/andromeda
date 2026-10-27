//! The `[server]` table: the data-plane listener, its drain and its HTTP/3
//! sibling.

use crate::{
    human_duration::HumanDuration,
    schema::{http3::Http3Config, listener_transport::ListenerTransport},
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid listener configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ServerConfigError {
    /// TCP port zero is reserved for dynamic binding and is not accepted for broker execution.
    #[error("server.port must be greater than 0")]
    ZeroPort,
    /// A zero timeout closes a connection at once after a response that
    /// announced the close, which resets a client still sending before it has
    /// read the response.
    #[error("server.announced_close_timeout must be greater than 0")]
    ZeroAnnouncedCloseTimeout,
    /// A connection delaying its close must never outlive the shutdown drain.
    #[error("server.announced_close_timeout must be shorter than server.graceful_drain_timeout")]
    AnnouncedCloseNotBelowGracefulDrain,
}

/// The data-plane listener.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Listener address.
    #[toml_comment(inline)]
    pub host: IpAddr,
    /// Listener port.
    pub port: u16,
    /// Whether the listener serves plaintext or terminates TLS: `plaintext` or `tls`.
    #[toml_comment(inline)]
    pub transport: ListenerTransport,
    /// How long accepted connections may take to drain once shutdown begins,
    /// in human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub graceful_drain_timeout: HumanDuration,
    /// How long a connection stays half-closed, unread, after a response
    /// announced `Connection: close` (an HTTP/1.1 refusal of an upload the
    /// broker did not read), so the client reads the response before the
    /// connection is closed, in human-readable form (`"1s"`). Shorter than
    /// `graceful_drain_timeout`.
    #[toml_comment(inline)]
    pub announced_close_timeout: HumanDuration,
    /// The HTTP/3 listener sharing this listener's host and port.
    pub http3: Http3Config,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1056,
            transport: ListenerTransport::Plaintext,
            graceful_drain_timeout: HumanDuration::from_secs(30),
            announced_close_timeout: HumanDuration::from_secs(1),
            http3: Http3Config::default(),
        }
    }
}

impl ServerConfig {
    /// Validate the externally visible listener configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError::ZeroPort`] when the configured port is zero, which is
    /// reserved for dynamic binding and cannot be advertised as a broker listener,
    /// [`ServerConfigError::ZeroAnnouncedCloseTimeout`] when the announced-close timeout is zero, and
    /// [`ServerConfigError::AnnouncedCloseNotBelowGracefulDrain`] when it is not shorter than
    /// the graceful drain.
    pub const fn validate(&self) -> Result<(), ServerConfigError> {
        if self.port == 0 {
            Err(ServerConfigError::ZeroPort)
        } else if self.announced_close_timeout.is_zero() {
            Err(ServerConfigError::ZeroAnnouncedCloseTimeout)
        } else if self.announced_close_timeout.duration().as_nanos() >= self.graceful_drain_timeout.duration().as_nanos() {
            Err(ServerConfigError::AnnouncedCloseNotBelowGracefulDrain)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::{
            listener_transport::ListenerTransport,
            server::{ServerConfig, ServerConfigError},
        },
    };
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::{error::Error, str::FromStr};

    #[test]
    fn zero_listener_port_is_rejected() {
        let config = ServerConfig {
            port: 0,
            ..ServerConfig::default()
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroPort));
    }

    #[test]
    fn the_default_listener_configuration_is_valid() {
        assert_eq!(ServerConfig::default().validate(), Ok(()));
    }

    #[test]
    fn a_zero_announced_close_timeout_is_rejected() {
        let config = ServerConfig {
            announced_close_timeout: HumanDuration::from_secs(0),
            ..ServerConfig::default()
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroAnnouncedCloseTimeout));
    }

    #[test]
    fn an_announced_close_timeout_not_below_the_graceful_drain_is_rejected() {
        for timeout in [HumanDuration::from_secs(30), HumanDuration::from_secs(31)] {
            let config = ServerConfig {
                announced_close_timeout: timeout,
                ..ServerConfig::default()
            };
            assert_eq!(config.validate(), Err(ServerConfigError::AnnouncedCloseNotBelowGracefulDrain));
        }
    }

    #[test]
    fn the_announced_close_timeout_parses_human_readable() -> Result<(), Box<dyn Error>> {
        /// Just the announced-close timeout, as the `[server]` table spells it.
        #[derive(Deserialize)]
        struct TimeoutHolder {
            /// The timeout under test.
            announced_close_timeout: HumanDuration,
        }
        let holder: TimeoutHolder = toml::from_str("announced_close_timeout = \"750ms\"")?;
        assert_eq!(holder.announced_close_timeout, HumanDuration::from_millis(750));
        Ok(())
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
