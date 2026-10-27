//! Construction of one tenant's notification dispatcher.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_configuration::schema::{link_field_layout::LinkFieldLayout, notifications::delivery::DeliveryConfig};
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use std::sync::Arc;

/// Construct the notification dispatcher for one tenant.
///
/// `delivery.request_timeout` bounds every outbound notification POST; a
/// subscription's own Table 5.2.15-1 `timeout` may only shorten it.
/// `delivery.private_addresses` decides whether an endpoint resolving into
/// private address space is refused. `link_field` lays out the `Link` field
/// lines of every HTTP notification as it does every response.
pub(super) fn construct_dispatcher(delivery: &DeliveryConfig, link_field: LinkFieldLayout) -> Result<Arc<TransportDispatcher>, TenantAssemblyError> {
    let dispatcher = TransportDispatcher::new(
        HttpTransportConfig {
            timeout: delivery.request_timeout(),
            private_addresses: delivery.private_addresses,
            link_field,
            ..HttpTransportConfig::default()
        },
        MqttPoolConfig::default(),
    )
    .map_err(AssemblyInvariant::from)?;
    Ok(Arc::new(dispatcher))
}
