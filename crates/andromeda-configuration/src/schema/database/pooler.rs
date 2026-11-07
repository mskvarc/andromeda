//! The `[database.pooler]` table: a transaction-pooling proxy on the request
//! path.

use crate::schema::database::connection::PostgresConnection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// A transaction-pooling proxy (pgbouncer) fronting PostgreSQL on the request path.
///
/// Its presence is the one switch between a deployment that connects straight
/// to PostgreSQL and one that routes the request path through the proxy:
/// provisioning, migration and maintenance keep using `database.connection`
/// directly, since they hold session state across transactions.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[schemars(description = "A transaction-pooling proxy, such as PgBouncer in transaction mode, that request connections go through.")]
#[schemars(deny_unknown_fields)]
pub struct PoolerConfig {
    /// The proxy's PostgreSQL-protocol connection, used by the request path
    /// in place of `database.connection`. The proxy forwards to the same
    /// PostgreSQL, so credentials and database are normally identical and
    /// only the host and port differ.
    #[schemars(
        description = "The proxy's connection, as a URL or as a table of its parts, used by requests in place of `database.connection`. The proxy forwards to the same PostgreSQL, so it normally differs from `database.connection` only in host and port. Tenant creation, migrations and maintenance keep connecting to `database.connection`."
    )]
    pub connection: PostgresConnection,
    /// The connections a named tenant's serving pool opens through the proxy.
    /// Small: the proxy multiplexes every tenant's client connections onto
    /// its own bounded set of backends, so a tenant needs only enough for its
    /// concurrent in-flight requests, and a working set larger than the
    /// runtime cache does not exhaust the host's ephemeral ports on cold
    /// loads.
    #[schemars(
        description = "The connections each named tenant's pool opens through the proxy, in place of `tenancy.databases.max_connections`. Keep it small: the proxy multiplexes every tenant onto its own backends, so a tenant needs only enough for its concurrent requests, and many loaded tenants do not exhaust the host's ports."
    )]
    pub max_connections_per_tenant: NonZeroU32,
}
