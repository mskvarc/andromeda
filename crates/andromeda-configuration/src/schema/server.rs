//! The `[server]` table: the data-plane listener, the address clients reach it
//! at, its drain and its HTTP/3 sibling.

use crate::{
    human_duration::HumanDuration,
    schema::{http3::Http3Config, listener_transport::ListenerTransport},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use url::Url;

/// The data-plane listener.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "The data-plane listener that serves the NGSI-LD API over HTTP/1.1 and HTTP/2, and its HTTP/3 sibling.")]
pub struct ServerConfig {
    /// Listener address.
    #[schemars(description = "The address the API listens on. The `--host` command-line flag overrides it.")]
    pub host: IpAddr,
    /// Listener port.
    #[schemars(description = "The TCP port the API listens on. The `--port` command-line flag overrides it.")]
    pub port: u16,
    /// Whether the listener serves plaintext or terminates TLS: `plaintext` or `tls`.
    #[schemars(
        description = "Whether the API listener serves plaintext or terminates TLS with the `[tls]` certificate. One port speaks one or the other, never both."
    )]
    pub transport: ListenerTransport,
    /// The `apiRoot` (clause 6.2) clients and peers reach the API at, path
    /// prefix included, such as `https://broker.example.org/context-broker`.
    ///
    /// Every absolute URL the broker mints is built on it, and subscription
    /// forwarding (clauses 5.8.1.4 and 5.8.6) names it as the endpoint of every
    /// subscription the broker places at a context source. `None` builds URLs
    /// on the origin each request reveals, with no prefix, and leaves
    /// subscription forwarding off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "The address clients and peers reach the API at, path prefix included, such as `https://broker.example.org/context-broker`. Every absolute URL the broker hands out is built on it, including stored `@context` URLs, and forwarded subscriptions name it as the endpoint their notifications come back to. Left out, URLs are built on the origin each request reveals, with no prefix, and subscriptions are not forwarded to context sources."
    )]
    pub public_url: Option<Url>,
    /// How long accepted connections may take to drain once shutdown begins,
    /// in human-readable form (`"30s"`).
    #[schemars(
        description = "How long open connections get to finish their requests once shutdown begins. Connections still open after it are closed. The whole stop takes at most this plus `notifications.evaluation.drain_timeout` from the stop signal, 60 seconds by default, however many tenants are loaded: the tenants and the background tasks share what the connections leave, and whatever still runs then is abandoned and logged. Give the broker the two together plus 10 seconds to stop before your orchestrator kills it; Docker and Podman wait only 10 seconds unless told otherwise."
    )]
    pub graceful_drain_timeout: HumanDuration,
    /// How long a connection stays half-closed, unread, after a response
    /// announced `Connection: close` (an HTTP/1.1 refusal of an upload the
    /// broker did not read), so the client reads the response before the
    /// connection is closed, in human-readable form (`"1s"`). Shorter than
    /// `graceful_drain_timeout`.
    #[schemars(
        description = "How long an HTTP/1.1 connection stays open, unread, after a response that announced `Connection: close`, so the client reads the response before the connection goes. Such a response refuses an upload the broker did not read."
    )]
    pub announced_close_timeout: HumanDuration,
    /// The HTTP/3 listener sharing this listener's host and port.
    #[schemars(description = "The optional HTTP/3 listener on the same host and port as the API.")]
    pub http3: Http3Config,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1056,
            transport: ListenerTransport::Plaintext,
            public_url: None,
            graceful_drain_timeout: HumanDuration::from_secs(30),
            announced_close_timeout: HumanDuration::from_secs(1),
            http3: Http3Config::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::{listener_transport::ListenerTransport, server::ServerConfig},
    };
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::{error::Error, str::FromStr};

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
    fn no_public_url_is_set_by_default() {
        assert_eq!(ServerConfig::default().public_url, None);
    }

    #[test]
    fn a_public_url_keeps_its_path_prefix() -> Result<(), Box<dyn Error>> {
        let config: ServerConfig = toml::from_str(&format!(
            "public_url = \"https://broker.example.org/context-broker\"\n{}",
            toml::to_string(&ServerConfig::default())?
        ))?;
        assert_eq!(
            config.public_url.map(String::from),
            Some(String::from("https://broker.example.org/context-broker"))
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
