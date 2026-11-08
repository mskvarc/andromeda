//! Construction of one tenant's notification dispatcher.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_configuration::schema::{link_field_layout::LinkFieldLayout, notifications::delivery::DeliveryConfig};
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use andromeda_outbound_http::address_policy::AddressPolicy;
use std::sync::Arc;

/// Construct the notification dispatcher for one tenant.
///
/// `delivery.request_timeout` bounds every outbound notification POST; a
/// subscription's own Table 5.2.15-1 `timeout` may only shorten it.
/// `delivery.private_addresses`, `delivery.allowed_addresses` and
/// `delivery.denied_addresses` decide which addresses an HTTP endpoint or an
/// MQTT broker may resolve into. `delivery.max_http_body` and
/// `delivery.max_mqtt_packet` bound what one notification may weigh on each
/// transport. `link_field` lays out the `Link` field lines of every HTTP
/// notification as it does every response.
pub(super) fn construct_dispatcher(delivery: &DeliveryConfig, link_field: LinkFieldLayout) -> Result<Arc<TransportDispatcher>, TenantAssemblyError> {
    let addresses = AddressPolicy::new(
        delivery.private_addresses,
        delivery.allowed_addresses.clone(),
        delivery.denied_addresses.clone(),
    );
    let dispatcher = TransportDispatcher::new(
        HttpTransportConfig {
            timeout: delivery.request_timeout(),
            addresses: addresses.clone(), // clone: addresses is still needed for the MQTT pool config.
            max_body: delivery.max_http_body,
            link_field,
            ..HttpTransportConfig::default()
        },
        MqttPoolConfig {
            max_packet_size: delivery.max_mqtt_packet,
            addresses,
            ..MqttPoolConfig::default()
        },
    )
    .map_err(AssemblyInvariant::from)?;
    Ok(Arc::new(dispatcher))
}
