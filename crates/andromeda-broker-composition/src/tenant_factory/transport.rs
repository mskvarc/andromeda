//! Construction of one tenant's notification transports.

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    http::config::HttpTransportConfig,
    mqtt::config::MqttPoolConfig,
    stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage},
};
use std::sync::Arc;

/// Notification transports and connection registries owned by one tenant.
pub(super) struct TenantTransports {
    pub(super) dispatcher: Arc<TransportDispatcher>,
    pub(super) sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
    pub(super) ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
}

/// Construct all transports for one tenant.
pub(super) fn construct_transports() -> Result<TenantTransports, TenantAssemblyError> {
    let sse_registry = Arc::new(ConnectionRegistry::new());
    let ws_registry = Arc::new(ConnectionRegistry::new());
    let dispatcher = Arc::new(
        TransportDispatcher::new(
            HttpTransportConfig::default(),
            MqttPoolConfig::default(),
            Arc::clone(&sse_registry),
            Arc::clone(&ws_registry),
        )
        .map_err(|error| TenantAssemblyError::invariant(format!("Failed to create transport dispatcher: {error}")))?,
    );
    Ok(TenantTransports {
        dispatcher,
        sse_registry,
        ws_registry,
    })
}
