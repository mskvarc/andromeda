//! How the HTTP transport is tuned.

use andromeda_configuration::{notification_body_limit::NotificationBodyLimit, schema::link_field_layout::LinkFieldLayout};
use andromeda_outbound_http::{address_policy::AddressPolicy, client::DEFAULT_POOL_IDLE_TIMEOUT};
use std::time::Duration;

/// Default per-delivery timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Configuration for the HTTP notification transport.
#[derive(Debug, Clone)]
pub struct HttpTransportConfig {
    /// How long to wait for an endpoint to answer.
    pub timeout: Duration,
    /// The largest serialised notification body this transport sends.
    pub max_body: NotificationBodyLimit,
    /// Which resolved addresses an endpoint may be notified at. A
    /// subscription's endpoint is client-supplied, so an unguarded broker
    /// POSTs to anything its network reaches; everything is permitted by
    /// default, since a single-network deployment notifies internal hosts.
    pub addresses: AddressPolicy,
    /// How long an idle pooled connection is kept for reuse.
    pub pool_idle_timeout: Duration,
    /// How a notification's link-values are laid out over `Link` field lines.
    pub link_field: LinkFieldLayout,
}

impl Default for HttpTransportConfig {
    fn default() -> HttpTransportConfig {
        HttpTransportConfig {
            timeout: DEFAULT_TIMEOUT,
            max_body: NotificationBodyLimit::DEFAULT,
            addresses: AddressPolicy::default(),
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
            link_field: LinkFieldLayout::Combined,
        }
    }
}
