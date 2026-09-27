//! How the HTTP transport is tuned.

use std::time::Duration;

/// Default per-delivery timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default maximum serialised notification size, in bytes.
const DEFAULT_MAX_PAYLOAD_SIZE: usize = 10 * 1024 * 1024;

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
    /// The largest notification this transport will attempt to deliver.
    pub max_payload_size: usize,
    /// Whether to refuse endpoints that resolve into private or reserved
    /// address space.
    pub private_ip_blocking: PrivateIpBlocking,
    /// How long an idle pooled connection is kept for reuse.
    pub pool_idle_timeout: Duration,
}

/// Whether the SSRF guard is armed.
///
/// A subscription's endpoint is client-supplied, so an unguarded broker will
/// happily POST to anything its network can reach. The guard is off by default
/// because a single-network deployment legitimately notifies internal hosts,
/// and turning it on there would break every one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrivateIpBlocking {
    /// Endpoints resolving to private or reserved addresses are refused.
    Enabled,
    /// Any resolvable endpoint is delivered to.
    #[default]
    Disabled,
}

impl Default for HttpTransportConfig {
    fn default() -> HttpTransportConfig {
        HttpTransportConfig {
            timeout: DEFAULT_TIMEOUT,
            max_payload_size: DEFAULT_MAX_PAYLOAD_SIZE,
            private_ip_blocking: PrivateIpBlocking::Disabled,
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
        }
    }
}
