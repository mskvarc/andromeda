//! The infrastructure behind the brokers of the pgbouncer stack, as the suite configuration names it.

use crate::{config::BrokerRole, config_error::ConfigError};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};
use url::Url;

/// The infrastructure behind the brokers of the pgbouncer stack, by broker role: the PostgreSQL server each broker
/// owns, the admin console of the pooler in front of it, and the size of the scale tests' workloads.
#[derive(Debug, Clone, Deserialize)]
pub struct PoolingSettings {
    /// The PostgreSQL server each broker owns, reached directly with the server's administrative login.
    #[serde(default)]
    servers: BTreeMap<BrokerRole, Url>,
    /// The admin console (database `pgbouncer`) of the pooler in front of each pooled broker's server.
    #[serde(default)]
    poolers: BTreeMap<BrokerRole, Url>,
    /// How many tenants a scale test provisions.
    scale_tenants: usize,
    /// How long one load run of a scale test lasts, in seconds.
    load_seconds: u64,
}

impl PoolingSettings {
    /// The PostgreSQL server the broker of `role` owns.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such server.
    pub fn server(&self, role: BrokerRole) -> Result<&Url, ConfigError> {
        self.servers.get(&role).ok_or_else(|| ConfigError::missing("POOLING__SERVERS", role.into()))
    }

    /// The admin console of the pooler in front of the server of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such pooler.
    pub fn pooler(&self, role: BrokerRole) -> Result<&Url, ConfigError> {
        self.poolers.get(&role).ok_or_else(|| ConfigError::missing("POOLING__POOLERS", role.into()))
    }

    /// How many tenants a scale test provisions.
    #[must_use]
    pub const fn scale_tenants(&self) -> usize {
        self.scale_tenants
    }

    /// How long one load run of a scale test lasts.
    #[must_use]
    pub const fn load_duration(&self) -> Duration {
        Duration::from_secs(self.load_seconds)
    }
}
