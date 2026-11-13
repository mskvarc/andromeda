//! The proxy between the outage broker and its PostgreSQL server, driven through its control API. Cut, it closes every
//! connection it carries and stops listening, so the server stops answering the broker as a stopped server does: each
//! connection fails and each new one is refused. Restored, it carries connections again.

use crate::{config::SuiteConfig, config_error::ConfigError};
use http::header::CONTENT_TYPE;
use serde::Serialize;
use url::Url;

/// Why the proxy could not be driven.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseProxyError {
    /// The stack runs no proxy.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The body of the request could not be written.
    #[error(transparent)]
    Body(#[from] serde_json::Error),
    /// The proxy's control API could not be reached.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// The proxy refused the change.
    #[error("the proxy answered {status} when it was to be {state}")]
    Refused {
        /// The state asked for.
        state: ProxyState,
        /// The status the proxy answered.
        status: reqwest::StatusCode,
    },
}

/// Whether the proxy carries connections to the server.
#[derive(Debug, Clone, Copy, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum ProxyState {
    /// It carries connections.
    Carrying,
    /// It refuses every connection.
    Cut,
}

/// The change of the proxy its control API accepts.
#[derive(Debug, Serialize)]
struct ProxyChange {
    /// Whether the proxy listens and carries connections.
    enabled: bool,
}

/// The proxy between the outage broker and its PostgreSQL server.
#[derive(Debug, Clone)]
pub struct DatabaseProxy {
    /// The proxy, as its control API addresses it.
    address: Url,
    /// The client every request goes through.
    http: reqwest::Client,
}

impl DatabaseProxy {
    /// The proxy the stack names.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseProxyError::Config`] when the stack runs no proxy.
    pub fn of() -> Result<DatabaseProxy, DatabaseProxyError> {
        let config = SuiteConfig::get().map_err(ConfigError::clone)?;
        Ok(DatabaseProxy {
            address: config.controls().database_proxy()?.clone(), // clone: the proxy outlives the configuration's borrow
            http: reqwest::Client::new(),
        })
    }

    /// Cuts the broker off its server.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseProxyError`] when the control API cannot be reached or refuses.
    pub async fn cut(&self) -> Result<(), DatabaseProxyError> {
        self.set(ProxyState::Cut).await
    }

    /// Connects the broker to its server again.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseProxyError`] when the control API cannot be reached or refuses.
    pub async fn restore(&self) -> Result<(), DatabaseProxyError> {
        self.set(ProxyState::Carrying).await
    }

    /// Puts the proxy in `state`.
    async fn set(&self, state: ProxyState) -> Result<(), DatabaseProxyError> {
        let change = ProxyChange {
            enabled: match state {
                ProxyState::Carrying => true,
                ProxyState::Cut => false,
            },
        };
        let status = self
            .http
            .post(self.address.clone()) // clone: each request owns its URL
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&change)?)
            .send()
            .await?
            .status();
        if status.is_success() {
            Ok(())
        } else {
            Err(DatabaseProxyError::Refused { state, status })
        }
    }
}
