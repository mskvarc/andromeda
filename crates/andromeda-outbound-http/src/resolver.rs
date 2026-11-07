//! A connect-time DNS resolver that refuses private and reserved addresses.
//!
//! The pre-send guard in [`crate::endpoint_guard`] resolves and vets an
//! endpoint's host, but `reqwest` resolves independently when it connects, so a
//! name a client controls could resolve to a public address for the guard and to
//! a private one for the connection (DNS rebinding). Installing this resolver on
//! the client closes that window: the addresses `reqwest` connects to are the
//! ones this resolver vetted, and a resolution containing any private or
//! reserved address is refused outright.

use crate::address_policy::AddressPolicy;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{error::Error, net::SocketAddr};
use tokio::net::lookup_host;

/// The result the `reqwest` DNS contract expects: resolved addresses, or a
/// boxed error that fails the connection.
type ResolveResult = Result<Addrs, Box<dyn Error + Send + Sync>>;

/// Resolver that blocks any resolution reaching an address the policy refuses.
#[derive(Debug)]
pub struct PrivateIpResolver {
    /// The rules a resolved address is judged by. A policy that refuses
    /// nothing makes this resolver a plain system-resolver passthrough.
    policy: AddressPolicy,
}

impl PrivateIpResolver {
    /// A resolver enforcing the given policy.
    #[must_use]
    pub const fn new(policy: AddressPolicy) -> PrivateIpResolver {
        PrivateIpResolver { policy }
    }
}

impl Resolve for PrivateIpResolver {
    fn resolve(&self, name: Name) -> Resolving {
        // clone: the future outlives the borrow of the resolver
        Box::pin(vetted_addrs(name.as_str().to_owned(), self.policy.clone()))
    }
}

/// Resolve a host and refuse it if the policy refuses any resolved address.
///
/// A refusal is handed back to `reqwest` as an
/// [`EndpointRefusal`](crate::endpoint_refusal::EndpointRefusal) inside its
/// boxed resolver error, which fails the connection, so a caller finds it in
/// the request error's source chain with
/// [`EndpointRefusal::find_in`](crate::endpoint_refusal::EndpointRefusal::find_in).
async fn vetted_addrs(host: String, policy: AddressPolicy) -> ResolveResult {
    // Port 0: `reqwest` overrides it with the URL's port (or the scheme default)
    // per the `Resolve` contract, so only the address is significant here.
    let resolved: Vec<SocketAddr> = lookup_host((host.as_str(), 0)).await?.collect();

    if let Some(refusal) = resolved.iter().find_map(|addr| policy.vet(addr.ip()).err()) {
        return Err(Box::new(refusal));
    }

    Ok(Box::new(resolved.into_iter()))
}

#[cfg(test)]
mod tests {
    use crate::{address_policy::AddressPolicy, endpoint_refusal::EndpointRefusal, resolver::vetted_addrs};
    use andromeda_configuration::{address_rule::AddressRule, schema::private_addresses::PrivateAddresses};

    fn loopback_rules() -> Vec<AddressRule> {
        ["127.0.0.0/8", "::1"].iter().filter_map(|entry| entry.parse().ok()).collect()
    }

    #[tokio::test]
    async fn a_loopback_resolution_is_refused_when_the_guard_is_armed() {
        let refused = vetted_addrs("localhost".to_string(), AddressPolicy::new(PrivateAddresses::Refused, Vec::new(), Vec::new())).await;
        let Err(error) = refused else {
            panic!("localhost resolves to loopback and must be refused");
        };
        assert!(error.is::<EndpointRefusal>(), "the refusal is the typed guard error: {error}");
    }

    #[tokio::test]
    async fn the_same_loopback_resolution_is_allowed_when_the_guard_is_off() {
        let allowed = vetted_addrs("localhost".to_string(), AddressPolicy::default()).await;
        assert!(allowed.is_ok(), "with the guard off, any resolvable host is requested");
    }

    #[tokio::test]
    async fn a_loopback_resolution_is_refused_when_the_address_is_denied_though_private_addresses_are_allowed() {
        let denying = AddressPolicy::new(PrivateAddresses::Allowed, Vec::new(), loopback_rules());
        let Err(error) = vetted_addrs("localhost".to_string(), denying).await else {
            panic!("localhost resolves to a denied address and must be refused");
        };
        assert!(error.is::<EndpointRefusal>(), "{error}");
    }

    #[tokio::test]
    async fn a_loopback_resolution_is_admitted_when_its_subnet_is_allowed() {
        let allowing = AddressPolicy::new(PrivateAddresses::Refused, loopback_rules(), Vec::new());
        assert!(vetted_addrs("localhost".to_string(), allowing).await.is_ok());
    }
}
