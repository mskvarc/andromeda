//! The client policy every outbound HTTP client is built from.

use crate::{address_policy::AddressPolicy, endpoint_guard::PrivateIpGuard, redirect_policy::vetted_redirects, resolver::PrivateIpResolver};
use reqwest::{ClientBuilder, redirect::Policy};
use std::time::Duration;

/// Default time an idle pooled connection is kept for reuse.
///
/// Shorter than the keep-alive timeout of common receivers (five seconds for
/// Node.js), so the broker retires an idle connection before the peer closes
/// it. A POST written onto a connection the peer has already abandoned fails
/// without a response and is not resent, since a POST is not idempotent;
/// retiring the connection first avoids that failure instead of retrying into a
/// possible duplicate.
pub const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(4);

/// How an outbound client behaves, from which both the client and the pre-send
/// guard are built so the two cannot disagree.
#[derive(Debug, Clone)]
pub struct OutboundHttpPolicy {
    /// The longest a whole request may take, unless the request sets its own.
    pub timeout: Duration,
    /// How long an idle pooled connection is kept for reuse.
    pub pool_idle_timeout: Duration,
    /// Which resolved addresses a request may be made to.
    pub addresses: AddressPolicy,
}

impl OutboundHttpPolicy {
    /// A client builder with the policy applied, for the caller to add its own
    /// settings to before building.
    ///
    /// Redirects are never followed: a public endpoint answering
    /// `302 Location: http://169.254.169.254/` would otherwise walk the request
    /// past the guard, which only vets the original URL. The connect-time
    /// resolver vets the addresses the client actually connects to, closing the
    /// DNS-rebinding window between the pre-send guard's resolution and
    /// `reqwest`'s own.
    pub fn client_builder(&self) -> ClientBuilder {
        self.builder_with(Policy::none())
    }

    /// A client builder with the policy applied that follows at most
    /// `max_redirects` redirects, for a request whose target is a document
    /// rather than an endpoint, such as a remote `@context`.
    ///
    /// Each hop is vetted before it is followed: an IP-literal target by the
    /// redirect policy, a domain by the connect-time resolver. The pre-send
    /// guard still has to vet the first URL.
    pub fn redirecting_client_builder(&self, max_redirects: usize) -> ClientBuilder {
        self.builder_with(vetted_redirects(self.addresses.clone(), max_redirects)) // clone: shares the lists with the redirect policy
    }

    /// The builder both client kinds share, with `redirect` deciding what a
    /// redirect does.
    fn builder_with(&self, redirect: Policy) -> ClientBuilder {
        ClientBuilder::new()
            .http1_title_case_headers()
            .timeout(self.timeout)
            .pool_idle_timeout(self.pool_idle_timeout)
            .redirect(redirect)
            .dns_resolver(PrivateIpResolver::new(self.addresses.clone())) // clone: shares the lists with the resolver
    }

    /// The pre-send guard matching this policy.
    #[must_use]
    pub fn guard(&self) -> PrivateIpGuard {
        PrivateIpGuard::new(self.addresses.clone()) // clone: shares the lists with the guard
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        address_policy::AddressPolicy,
        client::{DEFAULT_POOL_IDLE_TIMEOUT, OutboundHttpPolicy},
        endpoint_refusal::EndpointRefusal,
    };
    use andromeda_configuration::{address_rule::AddressRule, schema::private_addresses::PrivateAddresses};
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// An origin that answers its connections in turn with `answers`, one
    /// answer per connection, each built from the origin's own address.
    async fn origin(answers: Vec<fn(SocketAddr) -> String>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener.local_addr().expect("a bound address");
        tokio::spawn(async move {
            for answer in answers {
                let Ok((mut stream, _peer)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0_u8; 1024];
                let _read = stream.read(&mut buffer).await;
                let _written = stream.write_all(answer(address).as_bytes()).await;
            }
        });
        address
    }

    fn redirect_to(location: &str) -> String {
        format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    fn redirect_to_itself(address: SocketAddr) -> String {
        redirect_to(&format!("http://{address}/next"))
    }

    fn found(_address: SocketAddr) -> String {
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
    }

    fn policy(addresses: AddressPolicy) -> OutboundHttpPolicy {
        OutboundHttpPolicy {
            timeout: Duration::from_secs(5),
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
            addresses,
        }
    }

    /// Private addresses refused, with only the origin's own loopback address
    /// exempt.
    fn refusing_all_but_the_origin() -> AddressPolicy {
        let origin: AddressRule = "127.0.0.1".parse().expect("a well-formed rule");
        AddressPolicy::new(PrivateAddresses::Refused, vec![origin], Vec::new())
    }

    #[tokio::test]
    async fn a_redirect_is_returned_not_followed() {
        let address = origin(vec![|_| redirect_to("http://127.0.0.1:1/")]).await;
        let client = policy(AddressPolicy::default()).client_builder().build().expect("a buildable client");

        let response = client.get(format!("http://{address}/")).send().await.expect("the 302 itself is an answer");

        pretty_assertions::assert_eq!(response.status().as_u16(), 302);
    }

    #[tokio::test]
    async fn a_redirecting_client_follows_a_redirect_to_an_admitted_target() {
        let address = origin(vec![redirect_to_itself, found]).await;
        let client = policy(refusing_all_but_the_origin())
            .redirecting_client_builder(5)
            .build()
            .expect("a buildable client");

        let response = client.get(format!("http://{address}/")).send().await.expect("the redirect is followed");

        pretty_assertions::assert_eq!(response.status().as_u16(), 200);
        pretty_assertions::assert_eq!(response.url().path(), "/next");
    }

    #[tokio::test]
    async fn a_redirecting_client_refuses_a_redirect_to_an_ip_literal_the_policy_refuses() {
        let address = origin(vec![|_| redirect_to("http://127.0.0.2:1/")]).await;
        let client = policy(refusing_all_but_the_origin())
            .redirecting_client_builder(5)
            .build()
            .expect("a buildable client");

        let error = client.get(format!("http://{address}/")).send().await.expect_err("the hop is refused");

        let refused = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
        assert!(
            matches!(EndpointRefusal::find_in(&error), Some(EndpointRefusal::Blocked { ip }) if *ip == refused),
            "{error:?}"
        );
    }

    /// `localhost` resolves to loopback in either family, and the policy
    /// exempts neither loopback address for a domain hop: only the literal
    /// first URL, which the redirect policy never sees, reaches the origin.
    #[tokio::test]
    async fn a_redirecting_client_refuses_a_redirect_to_a_domain_resolving_where_the_policy_refuses() {
        let address = origin(vec![|address| redirect_to(&format!("http://localhost:{}/", address.port()))]).await;
        let refusing = AddressPolicy::new(PrivateAddresses::Refused, Vec::new(), Vec::new());
        let client = policy(refusing).redirecting_client_builder(5).build().expect("a buildable client");

        let error = client.get(format!("http://{address}/")).send().await.expect_err("the hop is refused");

        assert!(matches!(EndpointRefusal::find_in(&error), Some(EndpointRefusal::Blocked { .. })), "{error:?}");
    }

    #[tokio::test]
    async fn a_redirecting_client_follows_exactly_its_limit_of_redirects() {
        let address = origin(vec![redirect_to_itself, redirect_to_itself, found]).await;
        let client = policy(AddressPolicy::default())
            .redirecting_client_builder(2)
            .build()
            .expect("a buildable client");

        let response = client.get(format!("http://{address}/")).send().await.expect("two redirects are followed");

        pretty_assertions::assert_eq!(response.status().as_u16(), 200);
    }

    #[tokio::test]
    async fn a_redirecting_client_fails_a_chain_past_its_limit_without_a_refusal() {
        let address = origin(vec![redirect_to_itself, redirect_to_itself, found]).await;
        let client = policy(AddressPolicy::default())
            .redirecting_client_builder(1)
            .build()
            .expect("a buildable client");

        let error = client
            .get(format!("http://{address}/"))
            .send()
            .await
            .expect_err("the second redirect is past the limit");

        assert!(error.is_redirect(), "{error:?}");
        assert!(EndpointRefusal::find_in(&error).is_none(), "{error:?}");
    }
}
