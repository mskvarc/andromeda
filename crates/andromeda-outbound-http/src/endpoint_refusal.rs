//! Why an endpoint is refused before a request is sent to it.

use std::{io, net::IpAddr, sync::Arc};

/// An endpoint the guard will not let a request go to.
///
/// Carries only what the guard learned; the caller already holds the endpoint
/// and attaches it to whatever error it reports.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EndpointRefusal {
    /// The endpoint URL names no host.
    #[error("endpoint names no host")]
    NoHost,
    /// The host resolved to a private or reserved address.
    #[error("resolves to private/reserved IP {ip}")]
    Blocked {
        /// The address that triggered the guard.
        ip: IpAddr,
    },
    /// The host could not be resolved.
    #[error("DNS resolution failed: {source}")]
    DnsFailed {
        /// What the resolver reported, shared because the outcome is cached.
        source: Arc<io::Error>,
    },
}
