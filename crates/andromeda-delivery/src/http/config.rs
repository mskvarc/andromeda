//! How the HTTP transport is tuned.

use andromeda_configuration::{
    notification_body_limit::NotificationBodyLimit,
    schema::{link_field_layout::LinkFieldLayout, notifications::delivery::PrivateAddresses},
};
use std::time::Duration;

/// Default per-delivery timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default time an idle pooled connection is kept for reuse.
///
/// Shorter than the keep-alive timeout of common notification receivers (five
/// seconds for Node.js), so the broker retires an idle connection before the
/// receiver closes it. A POST written onto a connection the peer has already
/// abandoned fails without a response and is not resent, since a POST is not
/// idempotent; retiring the connection first avoids that failure instead of
/// retrying into a possible duplicate.
const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(4);

/// Configuration for the HTTP notification transport.
#[derive(Debug, Clone)]
pub struct HttpTransportConfig {
    /// How long to wait for an endpoint to answer.
    pub timeout: Duration,
    /// The largest serialised notification body this transport sends.
    pub max_body: NotificationBodyLimit,
    /// Whether an endpoint that resolves into private or reserved address
    /// space is refused. A subscription's endpoint is client-supplied, so an
    /// unguarded broker POSTs to anything its network reaches; allowed by
    /// default, since a single-network deployment notifies internal hosts.
    pub private_addresses: PrivateAddresses,
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
            private_addresses: PrivateAddresses::Allowed,
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
            link_field: LinkFieldLayout::Combined,
        }
    }
}
