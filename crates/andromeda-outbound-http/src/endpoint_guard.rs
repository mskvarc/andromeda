//! Refusing to send into address space a client should not be able to reach.
//!
//! The check is on resolved addresses rather than on the hostname, because a
//! name a client controls can resolve anywhere. `reqwest` never hands an
//! IP-literal URL to the connect-time resolver, so this pre-send check is what
//! covers those.

use crate::{address_class::is_private_ip, endpoint_refusal::EndpointRefusal};
use andromeda_configuration::schema::private_ip_blocking::PrivateIpBlocking;
use moka::future::Cache;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::net::lookup_host;
use url::{Host, Url};

/// TTL for the cached outcomes.
///
/// Short by design: keeps the guard effective against DNS rebinding while
/// still eliminating the per-request `lookup_host` round trip for hot
/// endpoints.
const OUTCOME_TTL: Duration = Duration::from_secs(60);

/// Maximum cached outcomes.
const OUTCOME_CAPACITY: u64 = 1024;

/// Port assumed when an endpoint URL names neither one nor a scheme with a
/// known default.
const DEFAULT_PORT: u16 = 80;

/// The host and port a request is sent to, the key outcomes are cached under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Authority {
    /// The host, keeping the domain / IPv4 / IPv6 distinction the URL parser drew.
    host: Host,
    /// The port the host is reached on.
    port: u16,
}

/// The pre-send guard: vets an endpoint's resolved addresses before a request
/// goes out.
#[derive(Debug, Clone)]
pub struct PrivateIpGuard {
    /// Whether the guard is armed.
    blocking: PrivateIpBlocking,
    /// Short-TTL cache of outcomes keyed by authority.
    ///
    /// Collapses repeated `lookup_host` round trips for fan-out requests
    /// without undermining the guard.
    outcomes: Cache<Authority, Result<(), EndpointRefusal>>,
}

impl PrivateIpGuard {
    /// A guard enforcing the given policy.
    #[must_use]
    pub fn new(blocking: PrivateIpBlocking) -> PrivateIpGuard {
        let outcomes = Cache::builder().max_capacity(OUTCOME_CAPACITY).time_to_live(OUTCOME_TTL).build();
        PrivateIpGuard { blocking, outcomes }
    }

    /// Refuse the endpoint when its host resolves into address space the
    /// broker will not reach on a client's behalf. A guard that allows private
    /// addresses accepts every endpoint without resolving anything.
    ///
    /// # Errors
    ///
    /// Returns the [`EndpointRefusal`] naming why the endpoint is refused.
    pub async fn check(&self, endpoint: &Url) -> Result<(), EndpointRefusal> {
        match self.blocking {
            PrivateIpBlocking::Allowed => return Ok(()),
            PrivateIpBlocking::Refused => {}
        }
        let host = endpoint.host().ok_or(EndpointRefusal::NoHost)?.to_owned();
        let port = endpoint.port_or_known_default().unwrap_or(DEFAULT_PORT);
        let authority = Authority { host, port };
        if let Some(cached) = self.outcomes.get(&authority).await {
            return cached;
        }
        let outcome = vet(&authority).await;
        self.outcomes.insert(authority, outcome.clone()).await; // clone: the cache takes one and the caller reads the other
        outcome
    }
}

/// Resolve an authority and decide whether a request may proceed.
async fn vet(authority: &Authority) -> Result<(), EndpointRefusal> {
    match &authority.host {
        Host::Ipv4(address) => refuse_if_private(IpAddr::V4(*address)),
        Host::Ipv6(address) => refuse_if_private(IpAddr::V6(*address)),
        Host::Domain(domain) => {
            let resolved = lookup_host((domain.as_str(), authority.port))
                .await
                .map_err(|error| EndpointRefusal::DnsFailed { source: Arc::new(error) })?;
            resolved.map(|socket| socket.ip()).try_for_each(refuse_if_private)
        }
    }
}

/// Refuse a private or reserved address.
fn refuse_if_private(ip: IpAddr) -> Result<(), EndpointRefusal> {
    if is_private_ip(&ip) { Err(EndpointRefusal::Blocked { ip }) } else { Ok(()) }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{endpoint_guard::PrivateIpGuard, endpoint_refusal::EndpointRefusal};
    use andromeda_configuration::schema::private_ip_blocking::PrivateIpBlocking;
    use url::Url;

    fn endpoint(text: &str) -> Url {
        Url::parse(text).expect("a well-formed endpoint")
    }

    #[tokio::test]
    async fn loopback_and_link_local_literals_are_refused_when_armed() {
        let guard = PrivateIpGuard::new(PrivateIpBlocking::Refused);
        for text in ["http://127.0.0.1:1/", "http://[::1]/", "http://169.254.169.254/latest"] {
            let refusal = guard.check(&endpoint(text)).await.expect_err(text);
            assert!(matches!(refusal, EndpointRefusal::Blocked { .. }), "{text}: {refusal}");
        }
    }

    #[tokio::test]
    async fn a_name_resolving_to_loopback_is_refused_and_the_cached_answer_agrees() {
        let guard = PrivateIpGuard::new(PrivateIpBlocking::Refused);
        let target = endpoint("http://localhost:9/");
        for _ in 0..2 {
            let refusal = guard.check(&target).await.expect_err("localhost is loopback");
            assert!(matches!(refusal, EndpointRefusal::Blocked { .. }), "{refusal}");
        }
    }

    #[tokio::test]
    async fn a_public_literal_is_allowed_when_armed() {
        let guard = PrivateIpGuard::new(PrivateIpBlocking::Refused);
        assert!(guard.check(&endpoint("http://8.8.8.8/")).await.is_ok());
    }

    #[tokio::test]
    async fn every_endpoint_is_allowed_when_the_guard_is_off() {
        let guard = PrivateIpGuard::new(PrivateIpBlocking::Allowed);
        assert!(guard.check(&endpoint("http://127.0.0.1:1/")).await.is_ok());
    }
}
