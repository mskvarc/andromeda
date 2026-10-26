//! Listener, graceful-drain and connection-close configuration.

use crate::{header_section_limit::HeaderSectionLimit, human_duration::HumanDuration, schema::listener_transport::ListenerTransport};
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
    /// A zero delay closes a connection at once after a response that
    /// announced the close, which resets a client still sending before it has
    /// read the response.
    #[error("server.announced_close_delay must be greater than 0")]
    ZeroAnnouncedCloseDelay,
    /// A connection delaying its close must never outlive the shutdown drain.
    #[error("server.announced_close_delay must be shorter than server.graceful_drain")]
    AnnouncedCloseDelayNotBelowGracefulDrain,
}

/// Address and graceful-drain policy for inbound HTTP execution.
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
    /// Largest request header section accepted, from `"8 KiB"` to `"32 KiB"`,
    /// on HTTP/1.1, HTTP/2 and HTTP/3 alike: every field, the request line's
    /// method, target and authority included, costs its name, its value and
    /// 32 bytes. A request over it is answered 431. The HTTP/3 listener and
    /// the management listener apply the same value.
    #[toml_comment(inline)]
    pub max_request_header_section: HeaderSectionLimit,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1056,
            transport: ListenerTransport::Plaintext,
            graceful_drain: HumanDuration::from_secs(30),
            announced_close_delay: HumanDuration::from_secs(1),
            max_request_header_section: HeaderSectionLimit::MAXIMUM,
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
}

#[cfg(test)]
mod tests {
    use crate::{
        header_section_limit::HeaderSectionLimit,
        human_duration::HumanDuration,
        schema::{
            listener_transport::ListenerTransport,
            server::{ServerConfig, ServerConfigError},
        },
    };
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::{error::Error, net::IpAddr, str::FromStr};

    #[test]
    fn zero_listener_port_is_rejected() {
        let config = ServerConfig {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 0,
            transport: ListenerTransport::Plaintext,
            graceful_drain: HumanDuration::from_secs(30),
            announced_close_delay: HumanDuration::from_secs(1),
            max_request_header_section: HeaderSectionLimit::MAXIMUM,
        };
        assert_eq!(config.validate(), Err(ServerConfigError::ZeroPort));
    }

    #[test]
    fn the_default_listener_configuration_is_valid() {
        assert_eq!(ServerConfig::default().validate(), Ok(()));
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
    fn the_request_header_section_defaults_to_32_kib_and_parses_human_readable() -> Result<(), Box<dyn Error>> {
        assert_eq!(ServerConfig::default().max_request_header_section.octets(), 32_768);
        let rendered =
            toml::to_string(&ServerConfig::default())?.replace("max_request_header_section = \"32.0 KiB\"", "max_request_header_section = \"8 KiB\"");
        assert_eq!(
            toml::from_str::<ServerConfig>(&rendered)?.max_request_header_section,
            HeaderSectionLimit::MINIMUM
        );
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
