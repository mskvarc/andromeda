//! The root of the configuration: every top-level table.

use crate::{
    built_in_defaults::BuiltInDefaults,
    schema::{
        database::{DatabaseConfig, connection::PostgresConnection},
        entity::EntityConfig,
        expiry::ExpiryConfig,
        extensions::ExtensionsConfig,
        federation::FederationConfig,
        http::HttpStackConfig,
        jsonld::JsonLdConfig,
        management::ManagementConfig,
        notifications::NotificationsConfig,
        observability::ObservabilityConfig,
        posted_query::PostedQueryConfig,
        request_limits::RequestLimitsConfig,
        server::ServerConfig,
        snapshot::SnapshotConfig,
        temporal::TemporalConfig,
        tenancy::TenancyConfig,
        tls::TlsConfig,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The whole broker configuration: one table per concern an operator
/// configures.
///
/// The broker composition reads it and hands each crate the one table that
/// crate reads. It has no default: the database connection has none.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[schemars(deny_unknown_fields)]
#[schemars(
    title = "Andromeda configuration",
    description = "The whole broker configuration, one table per concern. Each table is independent of the others except where a key names another by its dotted path."
)]
pub struct BrokerConfig {
    /// The data-plane listener and its HTTP/3 sibling.
    #[schemars(description = "The data-plane listener that serves the NGSI-LD API, and its HTTP/3 sibling.")]
    pub server: ServerConfig,

    /// The identity every TLS-terminating listener presents; read only when
    /// some listener terminates TLS.
    #[schemars(
        description = "The certificate, private key and handshake policy every TLS-terminating listener shares. Read only when some listener terminates TLS."
    )]
    pub tls: TlsConfig,

    /// Where the broker serves its operational endpoints.
    #[schemars(description = "Where the broker serves its health probes and metrics.")]
    pub management: ManagementConfig,

    /// What one request may cost, identical on every replica.
    #[schemars(description = "What one request may cost the broker. Every replica of one deployment runs the same values.")]
    pub request_limits: RequestLimitsConfig,

    /// How the inbound HTTP stack treats a request it accepts.
    #[schemars(
        description = "The web-facing layers in front of the API: forwarded-field trust, `Link` layout, body decoding, CORS, compression, `Server-Timing`, security headers and CSRF."
    )]
    pub http: HttpStackConfig,

    /// The PostgreSQL the broker stores in, and how it connects.
    #[schemars(description = "The PostgreSQL database the broker stores in, and how it connects.")]
    pub database: DatabaseConfig,

    /// Named tenants: whether they are served, where their data lives, how
    /// many stay open.
    #[schemars(description = "Named tenants: whether they are served, where their data lives, and how many stay open.")]
    pub tenancy: TenancyConfig,

    /// The sweeps that delete what is past its `expiresAt`.
    #[schemars(description = "The sweeps that delete what is past its `expiresAt`.")]
    pub expiry: ExpiryConfig,

    /// How `@context` documents are resolved.
    #[schemars(description = "How `@context` documents are resolved.")]
    pub jsonld: JsonLdConfig,

    /// Entity batches, their deferred side effects, and the spatial index.
    #[schemars(description = "Entity batches, their deferred side effects, and the spatial index.")]
    pub entity: EntityConfig,

    /// Reading, recording and storing Temporal Evolutions.
    #[schemars(description = "Reading, recording and storing Temporal Evolutions.")]
    pub temporal: TemporalConfig,

    /// Delivering notifications, evaluating live changes, and draining
    /// periodic subscriptions.
    #[schemars(description = "Delivering notifications, evaluating live changes, and draining periodic subscriptions.")]
    pub notifications: NotificationsConfig,

    /// Forwarding to Context Sources and this broker's identity among them.
    #[schemars(description = "Forwarding to Context Sources, and this broker's identity among them.")]
    pub federation: FederationConfig,

    /// Snapshot lifetimes, quotas and preparation.
    #[schemars(description = "Snapshot lifetimes, quotas and preparation.")]
    pub snapshot: SnapshotConfig,

    /// How long a paged query sent by POST is held for its page links, and
    /// how many one tenant keeps.
    #[schemars(description = "How long a paged query sent by POST is kept for the page links of its result, and how many one tenant keeps.")]
    pub posted_query: PostedQueryConfig,

    /// Andromeda's features beyond the NGSI-LD specification.
    #[schemars(description = "Andromeda's features beyond the NGSI-LD specification.")]
    pub extensions: ExtensionsConfig,

    /// Logging and metrics.
    #[schemars(description = "Logging and metrics.")]
    pub observability: ObservabilityConfig,
}

impl BrokerConfig {
    /// The configuration of a broker storing in `connection`, every other key
    /// at its built-in default.
    #[must_use]
    pub fn new(connection: PostgresConnection) -> BrokerConfig {
        let BuiltInDefaults {
            server,
            tls,
            management,
            request_limits,
            http,
            database,
            tenancy,
            expiry,
            jsonld,
            entity,
            temporal,
            notifications,
            federation,
            snapshot,
            posted_query,
            extensions,
            observability,
        } = BuiltInDefaults::default();
        BrokerConfig {
            server,
            tls,
            management,
            request_limits,
            http,
            database: DatabaseConfig::new(connection, database),
            tenancy,
            expiry,
            jsonld,
            entity,
            temporal,
            notifications,
            federation,
            snapshot,
            posted_query,
            extensions,
            observability,
        }
    }

    /// The built-in defaults storing in a local PostgreSQL, for tests that
    /// change one key and never connect.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn local() -> BrokerConfig {
        BrokerConfig::new(PostgresConnection::new(url_macro::url!("postgres://andromeda@localhost/andromeda")))
    }
}
