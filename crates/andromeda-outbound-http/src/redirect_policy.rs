//! The redirect policy of a client that follows redirects.
//!
//! The pre-send guard vets only the URL a request is built with, and the
//! connect-time resolver never sees a host written as an IP literal. A client
//! that follows redirects therefore vets every hop's literal host here, before
//! the hop is followed, while a hop naming a domain is vetted by the resolver
//! when it connects.

use crate::{address_policy::AddressPolicy, endpoint_refusal::EndpointRefusal};
use reqwest::redirect::Policy;
use std::net::IpAddr;
use url::{Host, Url};

/// Follow at most `max_redirects` redirects, refusing a hop whose target the
/// policy does not admit.
///
/// A refused hop fails the request with the [`EndpointRefusal`] in its source
/// chain; past the limit the request fails as `reqwest`'s own limit fails it.
#[must_use]
pub fn vetted_redirects(addresses: AddressPolicy, max_redirects: usize) -> Policy {
    let limit = Policy::limited(max_redirects);
    Policy::custom(move |attempt| match literal_refusal(&addresses, attempt.url()) {
        Some(refusal) => attempt.error(refusal),
        None => limit.redirect(attempt),
    })
}

/// Why the hop to `target` is refused before it is followed: a target with no
/// host, or an IP-literal host the policy refuses. `None` for a domain, which
/// the resolver vets as the hop connects.
fn literal_refusal(addresses: &AddressPolicy, target: &Url) -> Option<EndpointRefusal> {
    match target.host() {
        None => Some(EndpointRefusal::NoHost),
        Some(Host::Ipv4(address)) => addresses.vet(IpAddr::V4(address)).err(),
        Some(Host::Ipv6(address)) => addresses.vet(IpAddr::V6(address)).err(),
        Some(Host::Domain(_)) => None,
    }
}
