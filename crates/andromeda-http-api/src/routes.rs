//! Named required route families for complete API construction.

use andromeda_http_component_report::routes::ComponentReportRoutes;
use andromeda_http_context_registry::routes::ContextRegistryRoutes;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_entity_map::routes::EntityMapRoutes;
use andromeda_http_health::routes::HealthRoutes;
use andromeda_http_notification_receiver::routes::NotificationReceiverRoutes;
use andromeda_http_registration::routes::RegistrationRoutes;
use andromeda_http_snapshot::routes::SnapshotRoutes;
use andromeda_http_source_identity::routes::SourceIdentityRoutes;
use andromeda_http_subscription::routes::SubscriptionRoutes;
use andromeda_http_telemetry::routes::TelemetryRoutes;
use andromeda_http_temporal::routes::TemporalRoutes;
use andromeda_http_tenancy::routes::TenancyRoutes;
use typed_builder::TypedBuilder;

/// Every required state-complete route family.
///
/// Omitting a family is a compile-time construction failure:
///
/// ```compile_fail
/// use andromeda_http_api::routes::ApiRoutes;
/// let _incomplete = ApiRoutes::builder().build();
/// ```
#[derive(TypedBuilder)]
pub struct ApiRoutes {
    /// Entity, operation, type and attribute routes.
    pub(crate) entity: EntityRoutes,
    /// Temporal entity and operation routes.
    pub(crate) temporal: TemporalRoutes,
    /// Hosted JSON-LD context routes.
    pub(crate) context_registry: ContextRegistryRoutes,
    /// Context-source registration routes.
    pub(crate) registration: RegistrationRoutes,
    /// Entity and context-source subscription routes.
    pub(crate) subscription: SubscriptionRoutes,
    /// Current and temporal entity-map routes.
    pub(crate) entity_map: EntityMapRoutes,
    /// Snapshot lifecycle routes.
    pub(crate) snapshot: SnapshotRoutes,
    /// Context-source identity route.
    pub(crate) source_identity: SourceIdentityRoutes,
    /// The inbound notification route of forwarded subscriptions.
    pub(crate) notification_receiver: NotificationReceiverRoutes,
    /// Tenant administration routes.
    pub(crate) tenancy: TenancyRoutes,
    /// Aggregate health route.
    pub(crate) health: HealthRoutes,
    /// Active-component report route.
    pub(crate) component_report: ComponentReportRoutes,
    /// Metrics scrape route, mounted only when metrics exposure is enabled.
    #[builder(default = Default::default())]
    pub(crate) telemetry: Option<TelemetryRoutes>,
}
