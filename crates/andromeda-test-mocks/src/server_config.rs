//! Bind and broker-visible addressing for callback servers.

use std::{env, net::SocketAddr};
use thiserror::Error;
use url::Host;

/// Environment variable overriding the host the broker uses for callbacks.
pub const CALLBACK_HOST_ENV: &str = "ANDROMEDA_TEST_CALLBACK_HOST";

/// Default callback host for a broker running in Docker Desktop.
pub const DEFAULT_CALLBACK_HOST: &str = "host.docker.internal";

/// Typed callback server addressing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockServerConfig {
    bind_address: SocketAddr,
    visible_host: Host<String>,
}

impl MockServerConfig {
    /// Read the broker-visible host and use an explicit wildcard ephemeral bind address.
    ///
    /// # Errors
    ///
    /// Returns [`MockServerConfigError::InvalidVisibleHost`] when the value of `ANDROMEDA_TEST_CALLBACK_HOST`
    /// is neither a valid DNS name nor a valid IP address.
    pub fn from_environment() -> Result<MockServerConfig, MockServerConfigError> {
        let visible = env::var(CALLBACK_HOST_ENV).unwrap_or_else(|_| DEFAULT_CALLBACK_HOST.to_owned());
        Ok(MockServerConfig::new(
            SocketAddr::from(([0, 0, 0, 0], 0)),
            Host::parse(&visible).map_err(MockServerConfigError::InvalidVisibleHost)?,
        ))
    }

    /// Construct callback server addressing from validated values.
    #[must_use]
    pub const fn new(bind_address: SocketAddr, visible_host: Host<String>) -> MockServerConfig {
        MockServerConfig { bind_address, visible_host }
    }

    /// Socket address on which the test process listens.
    #[must_use]
    pub const fn bind_address(&self) -> SocketAddr {
        self.bind_address
    }

    /// Host the broker uses to reach the test process.
    #[must_use]
    pub const fn visible_host(&self) -> &Host<String> {
        &self.visible_host
    }
}

/// Construct a broker-visible callback URL from runtime configuration.
///
/// # Errors
///
/// Returns [`MockServerConfigError::InvalidVisibleHost`] when the value of `ANDROMEDA_TEST_CALLBACK_HOST`
/// is neither a valid DNS name nor a valid IP address.
pub fn callback_url(port: u16, path: &str) -> Result<String, MockServerConfigError> {
    let config = MockServerConfig::from_environment()?;
    Ok(format!("http://{}:{port}{path}", config.visible_host()))
}

/// Invalid callback server configuration.
#[derive(Debug, Error)]
pub enum MockServerConfigError {
    /// The broker-visible host is not a valid DNS name or IP address.
    #[error("invalid broker-visible callback host: {0}")]
    InvalidVisibleHost(#[source] url::ParseError),
}
