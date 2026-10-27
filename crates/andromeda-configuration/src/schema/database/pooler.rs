//! The `[database.pooler]` table: a transaction-pooling proxy on the request
//! path.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use toml_comment::TomlComment;
use url::Url;
use url_macro::url;

/// A transaction-pooling proxy (pgbouncer) fronting PostgreSQL on the request path.
///
/// Its presence is the one switch between a deployment that connects straight
/// to PostgreSQL and one that routes the request path through the proxy:
/// provisioning, migration and maintenance keep using `database.url`
/// directly, since they hold session state across transactions.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PoolerConfig {
    /// The proxy's PostgreSQL-protocol URL, used by the request path in place
    /// of `database.url`. The proxy forwards to the same PostgreSQL, so
    /// credentials and database are normally identical and only the host and
    /// port differ.
    #[toml_comment(inline)]
    pub url: Url,
    /// The connections a named tenant's serving pool opens through the proxy.
    /// Small: the proxy multiplexes every tenant's client connections onto
    /// its own bounded set of backends, so a tenant needs only enough for its
    /// concurrent in-flight requests, and a working set larger than the
    /// runtime cache does not exhaust the host's ephemeral ports on cold
    /// loads.
    #[toml_comment(inline)]
    pub max_connections_per_tenant: NonZeroU32,
}

impl Default for PoolerConfig {
    // Manual because `url::Url` has no `Default`. The value is pgbouncer's
    // default port on localhost and two connections, which overlap a read
    // with a write while the proxy does the real backend pooling.
    fn default() -> PoolerConfig {
        PoolerConfig {
            url: url!("postgres://postgres:postgres@localhost:6432/postgres"),
            max_connections_per_tenant: NonZeroU32::MIN.saturating_add(1),
        }
    }
}
