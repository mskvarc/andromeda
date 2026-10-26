//! Construction of one tenant's notification dispatcher.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_configuration::schema::link_field_layout::LinkFieldLayout;
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use std::{sync::Arc, time::Duration};

/// Construct the notification dispatcher for one tenant.
///
/// `request_timeout` bounds every outbound notification POST; a subscription's
/// own Table 5.2.15-1 `timeout` may only shorten it. `link_field` lays out the
/// `Link` field lines of every HTTP notification as it does every response.
pub(super) fn construct_dispatcher(request_timeout: Duration, link_field: LinkFieldLayout) -> Result<Arc<TransportDispatcher>, TenantAssemblyError> {
    let dispatcher = TransportDispatcher::new(
        HttpTransportConfig {
            timeout: request_timeout,
            link_field,
            ..HttpTransportConfig::default()
        },
        MqttPoolConfig::default(),
    )
    .map_err(AssemblyInvariant::from)?;
    Ok(Arc::new(dispatcher))
}
