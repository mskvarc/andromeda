//! The root of the configuration: every top-level section.

use crate::schema::{
    database::DatabaseConfig,
    entity::EntityConfig,
    extensions::ExtensionsConfig,
    federation::FederationConfig,
    http::HttpStackConfig,
    http3::Http3Config,
    identity::InfoConfig,
    jsonld::JsonLdConfig,
    management::ManagementConfig,
    notifications::NotificationsConfig,
    observability::ObservabilityConfig,
    pagination::PaginationConfig,
    server::ServerConfig,
    snapshot::SnapshotConfig,
    spatial::SpatialConfig,
    temporal::TemporalConfig,
    tenancy::TenancyConfig,
    tls::TlsConfig,
};
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// The whole broker configuration: one section per top-level table.
///
/// The broker composition reads it and hands each crate the one section that
/// crate reads.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct BrokerConfig {
    /// HTTP listener and graceful-drain configuration.
    pub server: ServerConfig,

    /// Process-wide inbound HTTP stack configuration.
    pub http: HttpStackConfig,

    /// Opt-in HTTP/3 (QUIC) listener configuration, sharing the TCP port.
    pub http3: Http3Config,

    /// The TLS identity every TLS-terminating listener presents; read only
    /// when some listener terminates TLS.
    pub tls: TlsConfig,

    /// Primary and tenant PostgreSQL connection configuration.
    pub database: DatabaseConfig,

    /// JSON-LD processing and context-loading configuration.
    pub jsonld: JsonLdConfig,

    /// Temporal history, paging and intent-drain configuration.
    pub temporal: TemporalConfig,

    /// Tenant creation and runtime-cache configuration.
    pub tenancy: TenancyConfig,

    /// Context Source identity configuration.
    pub identity: InfoConfig,

    /// Entity batching, expiry, and durable side-effect configuration.
    pub entity: EntityConfig,

    /// Distributed Context Source forwarding configuration.
    pub federation: FederationConfig,

    /// Snapshot operation configuration.
    pub snapshot: SnapshotConfig,

    /// Durable and live notification delivery configuration.
    pub notifications: NotificationsConfig,

    /// Spatial projection and reconciliation configuration.
    pub spatial: SpatialConfig,

    /// Composition-owned tracing configuration.
    pub observability: ObservabilityConfig,

    /// Management-plane topology and metrics-exposure configuration.
    pub management: ManagementConfig,

    /// Opt-in, non-standard extension configuration (off by default, but for
    /// cursor pagination).
    pub extensions: ExtensionsConfig,

    /// How a collection request that names neither `offset` nor `cursor` is
    /// paged.
    pub pagination: PaginationConfig,
}
