//! The services of a stack that a test drives to change what a broker meets around it: the signaller, which stops and
//! starts a broker's process on the pgbouncer stack, and the proxy between the outage broker and its PostgreSQL server
//! on the andromeda stack.

use crate::config_error::{ConfigError, ConfigKey};
use serde::Deserialize;
use url::Url;

/// The services of a stack a test drives, each absent on a stack that runs none. Their keys sit at the top level of
/// the configuration, `ANDROMEDA_TEST_SUITE__SIGNALLER` and `ANDROMEDA_TEST_SUITE__DATABASE_PROXY`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StackControls {
    /// The service that signals a broker's process.
    signaller: Option<Url>,
    /// The proxy between the outage broker and its PostgreSQL server, as its control API addresses it.
    database_proxy: Option<Url>,
}

impl StackControls {
    /// The service that signals a broker's process.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that runs none.
    pub fn signaller(&self) -> Result<&Url, ConfigError> {
        self.signaller.as_ref().ok_or(ConfigError::Missing(ConfigKey::Signaller))
    }

    /// The proxy between the outage broker and its PostgreSQL server, as its control API addresses it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that runs none.
    pub fn database_proxy(&self) -> Result<&Url, ConfigError> {
        self.database_proxy.as_ref().ok_or(ConfigError::Missing(ConfigKey::DatabaseProxy))
    }
}
