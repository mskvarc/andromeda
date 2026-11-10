//! The PostgreSQL servers behind the brokers of a stack, as the suite configuration names them, each reached directly
//! with the server's administrative login.

use crate::{config::BrokerRole, config_error::ConfigError, database_name::DatabaseName};
use serde::Deserialize;
use std::collections::BTreeMap;
use url::Url;

/// The PostgreSQL server each broker a test reaches the database of owns, by broker role.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct ServerSettings {
    /// Each server, as the platform database of the broker names it.
    servers: BTreeMap<BrokerRole, Url>,
}

impl ServerSettings {
    /// The server the broker of `role` owns, as its platform database names it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such server.
    pub fn server(&self, role: BrokerRole) -> Result<&Url, ConfigError> {
        self.servers.get(&role).ok_or_else(|| ConfigError::missing("SERVERS", role.into()))
    }

    /// The database `database` on the server the broker of `role` owns.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such server.
    pub fn database(&self, role: BrokerRole, database: &DatabaseName) -> Result<Url, ConfigError> {
        let mut url = self.server(role)?.clone();
        url.set_path(database.as_str());
        Ok(url)
    }
}
