//! Listener, graceful-drain and connection-close configuration.

use crate::close_policy::ClosePolicy;
use andromeda_configuration_scalars::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid listener configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ServerConfigError {
    /// TCP port zero is reserved for dynamic binding and is not accepted for broker execution.
    #[error("server.port must be greater than 0")]
    ZeroPort,
    /// A zero delay closes a connection at once after a response that
    /// announced the close, which resets a client still sending before it has
    /// read the response.
    #[error("server.announced_close_delay must be greater than 0")]
    ZeroAnnouncedCloseDelay,
    /// A connection delaying its close must never outlive the shutdown drain.
    #[error("server.announced_close_delay must be shorter than server.graceful_drain")]
    AnnouncedCloseDelayNotBelowGracefulDrain,
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
    /// How long a connection stays half-closed, unread, after a response
    /// announced `Connection: close` (an HTTP/1.1 refusal of an upload the
    /// broker did not read), so the client reads the response before the
    /// connection is closed, in human-readable form (`"1s"`). Shorter than
    /// `graceful_drain`.
    #[toml_comment(inline)]
    pub announced_close_delay: HumanDuration,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1056,
            transport: ListenerTransport::Plaintext,
            graceful_drain: HumanDuration::from_secs(30),
            announced_close_delay: HumanDuration::from_secs(1),
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
    /// [`ServerConfigError::ZeroAnnouncedCloseDelay`] when the announced-close delay is zero, and
    /// [`ServerConfigError::AnnouncedCloseDelayNotBelowGracefulDrain`] when it is not shorter than
    /// the graceful drain.
    pub const fn validate(&self) -> Result<(), ServerConfigError> {
        if self.port == 0 {
            Err(ServerConfigError::ZeroPort)
        } else if self.announced_close_delay.is_zero() {
            Err(ServerConfigError::ZeroAnnouncedCloseDelay)
        } else if self.announced_close_delay.duration().as_nanos() >= self.graceful_drain.duration().as_nanos() {
            Err(ServerConfigError::AnnouncedCloseDelayNotBelowGracefulDrain)
        } else {
            Ok(())
        }
    }

    /// Return how the listener's connections come to an end.
    #[must_use]
    pub const fn close_policy(&self) -> ClosePolicy {
        ClosePolicy {
            drain_deadline: self.graceful_drain.duration(),
            announced_close_delay: self.announced_close_delay.duration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        close_policy::ClosePolicy,
        config::{ListenerTransport, ServerConfig, ServerConfigError},
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::{error::Error, net::IpAddr, str::FromStr, time::Duration};

    #[test]
    fn zero_listener_port_is_rejected() {
        let config = ServerConfig {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 0,
            transport: ListenerTransport::Plaintext,
            graceful_drain: HumanDuration::from_secs(30),
            announced_close_delay: HumanDuration::from_secs(1),
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroPort));
    }

    #[test]
    fn the_default_listener_configuration_is_valid() {
        assert_eq!(ServerConfig::default().validate(), Ok(()));
    }

    #[test]
    fn the_default_policy_delays_an_announced_close_by_one_second_and_drains_for_thirty() {
        assert_eq!(
            ServerConfig::default().close_policy(),
            ClosePolicy {
                drain_deadline: Duration::from_secs(30),
                announced_close_delay: Duration::from_secs(1),
            }
        );
    }

    #[test]
    fn a_zero_announced_close_delay_is_rejected() {
        let config = ServerConfig {
            announced_close_delay: HumanDuration::from_secs(0),
            ..ServerConfig::default()
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroAnnouncedCloseDelay));
    }

    #[test]
    fn an_announced_close_delay_not_below_the_graceful_drain_is_rejected() {
        for delay in [HumanDuration::from_secs(30), HumanDuration::from_secs(31)] {
            let config = ServerConfig {
                announced_close_delay: delay,
                ..ServerConfig::default()
            };
            assert_eq!(config.validate(), Err(ServerConfigError::AnnouncedCloseDelayNotBelowGracefulDrain));
        }
    }

    #[test]
    fn the_announced_close_delay_parses_human_readable() -> Result<(), Box<dyn Error>> {
        /// Just the announced-close delay, as the `[server]` table spells it.
        #[derive(Deserialize)]
        struct DelayHolder {
            /// The delay under test.
            announced_close_delay: HumanDuration,
        }
        let holder: DelayHolder = toml::from_str("announced_close_delay = \"750ms\"")?;
        assert_eq!(holder.announced_close_delay, HumanDuration::from_millis(750));
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
