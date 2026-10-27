//! The root of the configuration: every top-level table.

use crate::schema::{
    database::DatabaseConfig,
    entity::EntityConfig,
    expiry::ExpiryConfig,
    extensions::ExtensionsConfig,
    federation::FederationConfig,
    http::HttpStackConfig,
    jsonld::JsonLdConfig,
    management::ManagementConfig,
    notifications::NotificationsConfig,
    observability::ObservabilityConfig,
    request_limits::RequestLimitsConfig,
    server::ServerConfig,
    snapshot::SnapshotConfig,
    temporal::TemporalConfig,
    tenancy::TenancyConfig,
    tls::TlsConfig,
};
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// The whole broker configuration: one table per concern an operator
/// configures.
///
/// The broker composition reads it and hands each crate the one table that
/// crate reads.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct BrokerConfig {
    /// The data-plane listener and its HTTP/3 sibling.
    pub server: ServerConfig,

    /// The identity every TLS-terminating listener presents; read only when
    /// some listener terminates TLS.
    pub tls: TlsConfig,

    /// Where the broker serves its operational endpoints.
    pub management: ManagementConfig,

    /// What one request may cost, identical on every replica.
    pub request_limits: RequestLimitsConfig,

    /// How the inbound HTTP stack treats a request it accepts.
    pub http: HttpStackConfig,

    /// The PostgreSQL the broker stores in, and how it connects.
    pub database: DatabaseConfig,

    /// Named tenants: whether they are served, where their data lives, how
    /// many stay open.
    pub tenancy: TenancyConfig,

    /// The sweeps that delete what is past its `expiresAt`.
    pub expiry: ExpiryConfig,

    /// How `@context` documents are resolved.
    pub jsonld: JsonLdConfig,

    /// Entity batches, their deferred side effects, and the spatial index.
    pub entity: EntityConfig,

    /// Reading, recording and storing Temporal Evolutions.
    pub temporal: TemporalConfig,

    /// Delivering notifications, evaluating live changes, and draining
    /// periodic subscriptions.
    pub notifications: NotificationsConfig,

    /// Forwarding to Context Sources and this broker's identity among them.
    pub federation: FederationConfig,

    /// Snapshot lifetimes, quotas and preparation.
    pub snapshot: SnapshotConfig,

    /// Andromeda's opt-in, non-standard features.
    pub extensions: ExtensionsConfig,

    /// Logging and metrics.
    pub observability: ObservabilityConfig,
}
