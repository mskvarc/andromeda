//! Refusing to send into address space a client should not be able to reach.
//!
//! The check is on resolved addresses rather than on the hostname, because a
//! name a client controls can resolve anywhere. `reqwest` never hands an
//! IP-literal URL to the connect-time resolver, so this pre-send check is what
//! covers those.

use crate::{address_policy::AddressPolicy, endpoint_refusal::EndpointRefusal};
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
    /// The rules a resolved address is judged by.
    policy: AddressPolicy,
    /// Short-TTL cache of outcomes keyed by authority.
    ///
    /// Collapses repeated `lookup_host` round trips for fan-out requests
    /// without undermining the guard.
    outcomes: Cache<Authority, Result<(), EndpointRefusal>>,
}

impl PrivateIpGuard {
    /// A guard enforcing the given policy.
    #[must_use]
    pub fn new(policy: AddressPolicy) -> PrivateIpGuard {
        let outcomes = Cache::builder().max_capacity(OUTCOME_CAPACITY).time_to_live(OUTCOME_TTL).build();
        PrivateIpGuard { policy, outcomes }
    }

    /// Refuse the endpoint when its host resolves into address space the
    /// broker will not reach on a client's behalf. A guard whose policy
    /// refuses nothing accepts every endpoint without resolving anything.
    ///
    /// # Errors
    ///
    /// Returns the [`EndpointRefusal`] naming why the endpoint is refused.
    pub async fn check(&self, endpoint: &Url) -> Result<(), EndpointRefusal> {
        let host = endpoint.host().ok_or(EndpointRefusal::NoHost)?;
        let port = endpoint.port_or_known_default().unwrap_or(DEFAULT_PORT);
        self.check_authority(&host.to_owned(), port).await
    }

    /// Refuse a host and port under the same rule as [`PrivateIpGuard::check`],
    /// for a protocol whose endpoint is not an HTTP URL, such as an MQTT broker.
    ///
    /// # Errors
    ///
    /// Returns the [`EndpointRefusal`] naming why the host is refused.
    pub async fn check_authority(&self, host: &Host, port: u16) -> Result<(), EndpointRefusal> {
        if self.policy.is_open() {
            return Ok(());
        }
        let authority = Authority { host: host.clone(), port }; // clone: the cache key owns the host it is looked up by
        if let Some(cached) = self.outcomes.get(&authority).await {
            return cached;
        }
        let outcome = vet(&self.policy, &authority).await;
        self.outcomes.insert(authority, outcome.clone()).await; // clone: the cache takes one and the caller reads the other
        outcome
    }
}

/// Resolve an authority and decide whether a request may proceed.
async fn vet(policy: &AddressPolicy, authority: &Authority) -> Result<(), EndpointRefusal> {
    match &authority.host {
        Host::Ipv4(address) => policy.vet(IpAddr::V4(*address)),
        Host::Ipv6(address) => policy.vet(IpAddr::V6(*address)),
        Host::Domain(domain) => {
            let resolved = lookup_host((domain.as_str(), authority.port))
                .await
                .map_err(|error| EndpointRefusal::DnsFailed { source: Arc::new(error) })?;
            resolved.map(|socket| socket.ip()).try_for_each(|ip| policy.vet(ip))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{address_policy::AddressPolicy, endpoint_guard::PrivateIpGuard, endpoint_refusal::EndpointRefusal};
    use andromeda_configuration::{address_rule::AddressRule, schema::private_addresses::PrivateAddresses};
    use std::net::Ipv4Addr;
    use url::{Host, Url};

    fn endpoint(text: &str) -> Url {
        Url::parse(text).expect("a well-formed endpoint")
    }

    fn rules(entries: &[&str]) -> Vec<AddressRule> {
        entries.iter().map(|entry| entry.parse().expect("a well-formed rule")).collect()
    }

    fn armed() -> PrivateIpGuard {
        PrivateIpGuard::new(AddressPolicy::new(PrivateAddresses::Refused, Vec::new(), Vec::new()))
    }

    fn open() -> PrivateIpGuard {
        PrivateIpGuard::new(AddressPolicy::default())
    }

    #[tokio::test]
    async fn loopback_and_link_local_literals_are_refused_when_armed() {
        let guard = armed();
        for text in ["http://127.0.0.1:1/", "http://10.0.0.1/", "http://[::1]/", "http://169.254.169.254/latest"] {
            let refusal = guard.check(&endpoint(text)).await.expect_err(text);
            assert!(matches!(refusal, EndpointRefusal::Blocked { .. }), "{text}: {refusal}");
        }
    }

    #[tokio::test]
    async fn a_name_resolving_to_loopback_is_refused_and_the_cached_answer_agrees() {
        let guard = armed();
        let target = endpoint("http://localhost:9/");
        for _ in 0..2 {
            let refusal = guard.check(&target).await.expect_err("localhost is loopback");
            assert!(matches!(refusal, EndpointRefusal::Blocked { .. }), "{refusal}");
        }
    }

    #[tokio::test]
    async fn a_broker_authority_is_vetted_like_an_endpoint() {
        let armed = armed();
        let refusal = armed.check_authority(&Host::Ipv4(Ipv4Addr::LOCALHOST), 1883).await.expect_err("loopback");
        assert!(matches!(refusal, EndpointRefusal::Blocked { .. }), "{refusal}");
        let off = open();
        assert!(off.check_authority(&Host::Ipv4(Ipv4Addr::LOCALHOST), 1883).await.is_ok());
    }

    #[tokio::test]
    async fn a_public_literal_is_allowed_when_armed() {
        let guard = armed();
        assert!(guard.check(&endpoint("http://8.8.8.8/")).await.is_ok());
    }

    #[tokio::test]
    async fn every_endpoint_is_allowed_when_the_guard_is_off() {
        let guard = open();
        assert!(guard.check(&endpoint("http://127.0.0.1:1/")).await.is_ok());
    }

    #[tokio::test]
    async fn an_allowed_subnet_is_reached_and_the_rest_of_private_space_is_not() {
        let guard = PrivateIpGuard::new(AddressPolicy::new(
            PrivateAddresses::Refused,
            rules(&["127.0.0.0/8", "10.2.0.0/24"]),
            Vec::new(),
        ));
        for text in ["http://127.0.0.1:1/", "http://10.2.0.5/", "http://[::ffff:10.2.0.5]/"] {
            assert!(guard.check(&endpoint(text)).await.is_ok(), "{text}");
        }
        for text in ["http://10.3.0.5/", "http://[::1]/"] {
            assert!(guard.check(&endpoint(text)).await.is_err(), "{text}");
        }
    }

    #[tokio::test]
    async fn a_denied_address_is_refused_even_when_private_addresses_are_allowed() {
        let guard = PrivateIpGuard::new(AddressPolicy::new(
            PrivateAddresses::Allowed,
            Vec::new(),
            rules(&["203.0.113.0/24", "198.51.100.9"]),
        ));
        for text in ["http://203.0.113.5/", "http://198.51.100.9:8080/"] {
            let refusal = guard.check(&endpoint(text)).await.expect_err(text);
            assert!(matches!(refusal, EndpointRefusal::Blocked { .. }), "{text}: {refusal}");
        }
        assert!(guard.check(&endpoint("http://198.51.100.10/")).await.is_ok());
        assert!(guard.check(&endpoint("http://127.0.0.1:1/")).await.is_ok());
    }

    #[tokio::test]
    async fn a_name_resolving_to_a_denied_address_is_refused_and_one_resolving_to_an_allowed_subnet_is_not() {
        let denying = PrivateIpGuard::new(AddressPolicy::new(PrivateAddresses::Allowed, Vec::new(), rules(&["127.0.0.0/8", "::1"])));
        assert!(denying.check(&endpoint("http://localhost:9/")).await.is_err());
        let allowing = PrivateIpGuard::new(AddressPolicy::new(PrivateAddresses::Refused, rules(&["127.0.0.0/8", "::1"]), Vec::new()));
        assert!(allowing.check(&endpoint("http://localhost:9/")).await.is_ok());
    }
}
