//! Why an endpoint is refused before a request is sent to it.

use std::{error::Error, io, iter::successors, net::IpAddr, sync::Arc};

/// An endpoint the guard will not let a request go to.
///
/// Carries only what the guard learned; the caller already holds the endpoint
/// and attaches it to whatever error it reports.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EndpointRefusal {
    /// The endpoint URL names no host.
    #[error("endpoint names no host")]
    NoHost,
    /// The host resolved to an address the policy refuses: one on the deny
    /// list, or a private or reserved one that the allow list does not exempt.
    #[error("resolves to refused IP {ip}")]
    Blocked {
        /// The address the policy refused.
        ip: IpAddr,
    },
    /// The host could not be resolved.
    #[error("DNS resolution failed: {source}")]
    DnsFailed {
        /// What the resolver reported, shared because the outcome is cached.
        source: Arc<io::Error>,
    },
}

impl EndpointRefusal {
    /// The refusal in the source chain of a request `error`.
    ///
    /// The connect-time resolver and the redirect policy can only refuse from
    /// inside `reqwest`, which wraps what they return in its own error, so a
    /// caller that tells a refusal from a transport failure looks for it here.
    #[must_use]
    pub fn find_in(error: &reqwest::Error) -> Option<&EndpointRefusal> {
        successors(error.source(), |&current| current.source()).find_map(|current| current.downcast_ref::<EndpointRefusal>())
    }
}

#[cfg(test)]
mod tests {
    use crate::endpoint_refusal::EndpointRefusal;
    use reqwest::Client;

    #[test]
    fn a_request_error_without_a_refusal_finds_none() {
        let Err(error) = Client::new().get("not a url").build() else {
            return;
        };
        assert!(EndpointRefusal::find_in(&error).is_none());
    }
}
