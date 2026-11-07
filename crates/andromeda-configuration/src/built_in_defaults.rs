//! The built-in defaults: the layer every other configuration layer is laid
//! over.

use crate::schema::{
    database::DatabaseDefaults,
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
use serde::Serialize;

/// Every key that has a built-in default, at it, in the shape of the
/// configuration. The database connections have none, so the file, the
/// environment or the caller of `BrokerConfig::new` names them.
#[derive(Serialize, Default, Debug)]
pub(crate) struct BuiltInDefaults {
    /// The data-plane listener.
    pub(crate) server: ServerConfig,
    /// The TLS identity.
    pub(crate) tls: TlsConfig,
    /// The operational endpoints.
    pub(crate) management: ManagementConfig,
    /// What one request may cost.
    pub(crate) request_limits: RequestLimitsConfig,
    /// The inbound HTTP stack.
    pub(crate) http: HttpStackConfig,
    /// The `[database]` keys other than the connection.
    pub(crate) database: DatabaseDefaults,
    /// Named tenants.
    pub(crate) tenancy: TenancyConfig,
    /// The expiry sweeps.
    pub(crate) expiry: ExpiryConfig,
    /// `@context` resolution.
    pub(crate) jsonld: JsonLdConfig,
    /// Entity batches, side effects and the spatial index.
    pub(crate) entity: EntityConfig,
    /// Temporal Evolutions.
    pub(crate) temporal: TemporalConfig,
    /// Notifications.
    pub(crate) notifications: NotificationsConfig,
    /// Forwarding to Context Sources.
    pub(crate) federation: FederationConfig,
    /// Snapshots.
    pub(crate) snapshot: SnapshotConfig,
    /// Features beyond the specification.
    pub(crate) extensions: ExtensionsConfig,
    /// Logging and metrics.
    pub(crate) observability: ObservabilityConfig,
}
