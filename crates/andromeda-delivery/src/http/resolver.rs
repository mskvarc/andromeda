//! A connect-time DNS resolver that refuses private and reserved addresses.
//!
//! The pre-send guard in [`crate::http::transport`] resolves and vets an
//! endpoint's host, but `reqwest` resolves independently when it connects, so a
//! name a client controls could resolve to a public address for the guard and to
//! a private one for the connection (DNS rebinding). Installing this resolver on
//! the client closes that window: the addresses `reqwest` connects to are the
//! ones this resolver vetted, and a resolution containing any private or
//! reserved address is refused outright.

use crate::http::{config::PrivateIpBlocking, private_ip::is_private_ip};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    error::Error,
    net::{IpAddr, SocketAddr},
};
use tokio::net::lookup_host;

/// The result the `reqwest` DNS contract expects: resolved addresses, or a
/// boxed error that fails the connection.
type ResolveResult = Result<Addrs, Box<dyn Error + Send + Sync>>;

/// A resolution the guard refused because it reached private or reserved
/// address space.
///
/// Handed back to `reqwest` inside its boxed resolver error, which fails the
/// connection; the host is the one `reqwest` asked to resolve and is named by
/// the request error that wraps this one.
#[derive(Debug, thiserror::Error)]
#[error("resolution blocked: resolves to private/reserved IP {ip}")]
struct PrivateAddressResolved {
    /// The first resolved address that triggered the guard.
    ip: IpAddr,
}

/// Resolver that blocks any resolution reaching private or reserved address
/// space.
#[derive(Debug)]
pub struct PrivateIpResolver {
    /// Whether the SSRF guard is armed. When disabled this resolver is a plain
    /// system-resolver passthrough.
    blocking: PrivateIpBlocking,
}

impl PrivateIpResolver {
    /// A resolver enforcing the given policy.
    #[must_use]
    pub const fn new(blocking: PrivateIpBlocking) -> PrivateIpResolver {
        PrivateIpResolver { blocking }
    }
}

impl Resolve for PrivateIpResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(vetted_addrs(name.as_str().to_owned(), self.blocking))
    }
}

/// Resolve a host and, when the guard is armed, refuse it if any resolved
/// address is private or reserved.
async fn vetted_addrs(host: String, blocking: PrivateIpBlocking) -> ResolveResult {
    // Port 0: `reqwest` overrides it with the URL's port (or the scheme default)
    // per the `Resolve` contract, so only the address is significant here.
    let resolved: Vec<SocketAddr> = lookup_host((host.as_str(), 0)).await?.collect();

    if blocking == PrivateIpBlocking::Enabled
        && let Some(private) = resolved.iter().find(|addr| is_private_ip(&addr.ip()))
    {
        return Err(Box::new(PrivateAddressResolved { ip: private.ip() }));
    }

    Ok(Box::new(resolved.into_iter()))
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::http::{
        config::PrivateIpBlocking,
        resolver::{PrivateAddressResolved, vetted_addrs},
    };

    #[tokio::test]
    async fn a_loopback_resolution_is_refused_when_the_guard_is_armed() {
        let refused = vetted_addrs("localhost".to_string(), PrivateIpBlocking::Enabled).await;
        let Err(error) = refused else {
            panic!("localhost resolves to loopback and must be refused");
        };
        assert!(error.is::<PrivateAddressResolved>(), "the refusal is the typed guard error: {error}");
    }

    #[tokio::test]
    async fn the_same_loopback_resolution_is_allowed_when_the_guard_is_off() {
        let allowed = vetted_addrs("localhost".to_string(), PrivateIpBlocking::Disabled).await;
        assert!(allowed.is_ok(), "with the guard off, any resolvable host is delivered to");
    }
}
