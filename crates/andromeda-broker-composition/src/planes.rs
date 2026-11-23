//! The data plane, the management plane and the local probe plane, assembled
//! per the configured topology.

use andromeda_configuration::schema::management::ManagementMode;
use andromeda_health::aggregate::Health;
use andromeda_http_api::{
    api::HttpApi,
    management::{ManagementApi, ManagementRoutes},
    routes::ApiRoutes,
};
use andromeda_http_health::routes::HealthRoutes;
use andromeda_http_stack::stack::HttpStack;
use andromeda_http_telemetry::routes::TelemetryRoutes;

/// The assembled HTTP services the broker serves.
pub(crate) struct Planes {
    /// The data plane, carrying the management endpoints in `shared-port` mode.
    pub(crate) api: HttpApi,
    /// The standalone management plane, present only in `separate-port` mode.
    pub(crate) management: Option<ManagementApi>,
    /// The liveness and readiness probes the local socket serves in every
    /// mode, for the `health` command; metrics are served on the network only.
    pub(crate) local_probes: ManagementApi,
}

/// Assemble the data plane, the management plane when it stands alone, and
/// the local probe plane, per the configured topology.
///
/// `separate-port` isolates the observability endpoints behind their own
/// listener; `shared-port` mounts them at the root of the data-plane listener;
/// `disabled` serves no management plane on the network, and validation has
/// already forced metrics off in that case. Every mode answers the probes on
/// the local socket from the same indicators.
pub(crate) fn assemble_planes(mode: ManagementMode, routes: ApiRoutes, health: Health, metrics: Option<TelemetryRoutes>, stack: &HttpStack) -> Planes {
    let health = HealthRoutes::new(health);
    let local_probes = ManagementApi::assemble(
        ManagementRoutes {
            health: health.clone(),
            metrics: None,
        },
        stack,
    );
    let management_routes = ManagementRoutes { health, metrics };
    let (api, management) = match mode {
        ManagementMode::SeparatePort => (HttpApi::assemble(routes, None, stack), Some(ManagementApi::assemble(management_routes, stack))),
        ManagementMode::SharedPort => (HttpApi::assemble(routes, Some(management_routes), stack), None),
        ManagementMode::Disabled => (HttpApi::assemble(routes, None, stack), None),
    };
    Planes { api, management, local_probes }
}
