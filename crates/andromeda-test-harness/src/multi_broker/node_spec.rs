//! Declaring one broker of a composed topology from environment overrides.

use crate::{
    configuration::{ConfigurationError, DatabaseEndpoint},
    multi_broker::MultiBrokerError,
};
use andromeda_test_client::broker_endpoint::{BrokerDnsOverride, BrokerEndpoint};
use std::{env, fmt::Debug, net::SocketAddr};
use url::Url;

/// The environment-variable name fragments that select one broker's overrides.
///
/// Every setting is read from `ANDROMEDA_{prefix}_{token}_{suffix}`: the prefix
/// scopes one composed topology, the token names a single broker inside it, and
/// the suffix is fixed per setting.
#[derive(Clone, Copy, Debug)]
pub struct BrokerEnvironmentKey<'a> {
    prefix: &'a str,
    token: &'a str,
}

impl<'a> BrokerEnvironmentKey<'a> {
    /// Bind a topology-wide prefix to the token naming one broker within it.
    #[must_use]
    pub const fn new(prefix: &'a str, token: &'a str) -> BrokerEnvironmentKey<'a> {
        BrokerEnvironmentKey { prefix, token }
    }

    /// Read one suffixed setting, falling back to its compiled-in default when the variable is unset or empty.
    fn value(self, suffix: &str, default: &str) -> String {
        let name = format!("ANDROMEDA_{}_{}_{suffix}", self.prefix, self.token);
        match env::var(name) {
            Ok(value) if !value.is_empty() => value,
            Ok(_) | Err(_) => default.to_owned(),
        }
    }
}

/// The compiled-in endpoints one broker falls back to when nothing overrides them.
///
/// Each field is the default for the matching variable named by a
/// [`BrokerEnvironmentKey`], in the order the specification reads them.
#[derive(Clone, Copy, Debug)]
pub struct BrokerNodeDefaults<'a> {
    client: &'a str,
    peer: &'a str,
    database: &'a str,
    alias: &'a str,
}

impl<'a> BrokerNodeDefaults<'a> {
    /// Collect the host-side client URL, the compose-network peer URL, the database URL, and the source alias.
    #[must_use]
    pub const fn new(client: &'a str, peer: &'a str, database: &'a str, alias: &'a str) -> BrokerNodeDefaults<'a> {
        BrokerNodeDefaults { client, peer, database, alias }
    }
}

/// Declarative configuration for one broker role in a composed topology.
#[derive(Clone)]
pub struct BrokerNodeSpec<R> {
    role: R,
    identity: R,
    client: BrokerEndpoint,
    peer: Url,
    database: DatabaseEndpoint,
    alias: String,
    dns_override: Option<BrokerDnsOverride>,
}

impl<R> BrokerNodeSpec<R>
where
    R: Copy + Debug,
{
    /// Build a broker specification from environment overrides and typed defaults.
    ///
    /// # Errors
    ///
    /// Returns [`MultiBrokerError::Configuration`] when the resolved client URL or database URL is invalid, and [`MultiBrokerError::Endpoint`] when the resolved peer URL does not parse.
    pub fn from_environment(
        role: R,
        identity: R,
        key: BrokerEnvironmentKey<'_>,
        defaults: BrokerNodeDefaults<'_>,
        dns_port: Option<u16>,
    ) -> Result<BrokerNodeSpec<R>, MultiBrokerError> {
        let client_value = key.value("URL", defaults.client);
        let peer_value = key.value("PEER_URL", defaults.peer);
        let database_value = key.value("DATABASE_URL", defaults.database);
        let alias = key.value("ALIAS", defaults.alias);
        let client = BrokerEndpoint::parse(client_value).map_err(|error| MultiBrokerError::Configuration(ConfigurationError::InvalidBrokerEndpoint(error)))?;
        let peer = Url::parse(&peer_value).map_err(|error| MultiBrokerError::Endpoint(error.to_string()))?;
        let database = DatabaseEndpoint::parse(database_value).map_err(MultiBrokerError::Configuration)?;
        let dns_override = dns_port.map(|port| {
            let host = peer.host_str().unwrap_or_default().to_owned();
            BrokerDnsOverride::new(host, SocketAddr::from(([127, 0, 0, 1], port)))
        });
        Ok(BrokerNodeSpec {
            role,
            identity,
            client,
            peer,
            database,
            alias,
            dns_override,
        })
    }

    /// Return this role's value.
    pub const fn role(&self) -> R {
        self.role
    }

    /// Return the identity whose store and source alias this role uses.
    pub const fn identity(&self) -> R {
        self.identity
    }

    /// Borrow the host-side broker endpoint.
    pub const fn client(&self) -> &BrokerEndpoint {
        &self.client
    }

    /// Borrow the compose-network endpoint used in registrations.
    pub const fn peer(&self) -> &Url {
        &self.peer
    }

    /// Borrow this broker's platform database endpoint.
    pub const fn database(&self) -> &DatabaseEndpoint {
        &self.database
    }

    /// Borrow this broker's source alias.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Borrow the optional host-side DNS override.
    pub const fn dns_override(&self) -> Option<&BrokerDnsOverride> {
        self.dns_override.as_ref()
    }
}
