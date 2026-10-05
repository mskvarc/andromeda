//! Construction of one tenant's notification transports.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    http::config::HttpTransportConfig,
    mqtt::config::MqttPoolConfig,
    stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage},
};
use andromeda_http_headers::link::field_layout::LinkFieldLayout;
use std::{sync::Arc, time::Duration};

/// Notification transports and connection registries owned by one tenant.
pub(super) struct TenantTransports {
    pub(super) dispatcher: Arc<TransportDispatcher>,
    pub(super) sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
    pub(super) ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
}

/// Construct all transports for one tenant.
///
/// `request_timeout` bounds every outbound notification POST; a subscription's
/// own Table 5.2.15-1 `timeout` may only shorten it. `link_field` lays out the
/// `Link` field lines of every HTTP notification as it does every response.
pub(super) fn construct_transports(request_timeout: Duration, link_field: LinkFieldLayout) -> Result<TenantTransports, TenantAssemblyError> {
    let sse_registry = Arc::new(ConnectionRegistry::new());
    let ws_registry = Arc::new(ConnectionRegistry::new());
    let dispatcher = Arc::new(
        TransportDispatcher::new(
            HttpTransportConfig {
                timeout: request_timeout,
                link_field,
                ..HttpTransportConfig::default()
            },
            MqttPoolConfig::default(),
            Arc::clone(&sse_registry),
            Arc::clone(&ws_registry),
        )
        .map_err(AssemblyInvariant::from)?,
    );
    Ok(TenantTransports {
        dispatcher,
        sse_registry,
        ws_registry,
    })
}
