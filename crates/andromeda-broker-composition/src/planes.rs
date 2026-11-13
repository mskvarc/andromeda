//! The data plane and the management plane, assembled per the configured
//! topology.

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

/// Assemble the data plane and, when it stands alone, the management plane, per
/// the configured topology.
///
/// `separate-port` isolates the observability endpoints behind their own
/// listener; `merged` mounts them at the root of the data-plane listener;
/// `disabled` serves no management plane and so no probe, and validation has
/// already forced metrics off in that case.
pub(crate) fn assemble_planes(
    mode: ManagementMode,
    routes: ApiRoutes,
    health: Health,
    metrics: Option<TelemetryRoutes>,
    stack: &HttpStack,
) -> (HttpApi, Option<ManagementApi>) {
    let management_routes = ManagementRoutes {
        health: HealthRoutes::new(health),
        metrics,
    };
    match mode {
        ManagementMode::SeparatePort => (HttpApi::assemble(routes, None, stack), Some(ManagementApi::assemble(management_routes, stack))),
        ManagementMode::SharedPort => (HttpApi::assemble(routes, Some(management_routes), stack), None),
        ManagementMode::Disabled => (HttpApi::assemble(routes, None, stack), None),
    }
}
