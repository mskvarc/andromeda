//! The client policy every outbound HTTP client is built from.

use crate::{endpoint_guard::PrivateIpGuard, resolver::PrivateIpResolver};
use andromeda_configuration::schema::private_ip_blocking::PrivateIpBlocking;
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
#[derive(Debug, Clone, Copy)]
pub struct OutboundHttpPolicy {
    /// The longest a whole request may take, unless the request sets its own.
    pub timeout: Duration,
    /// How long an idle pooled connection is kept for reuse.
    pub pool_idle_timeout: Duration,
    /// Whether an endpoint resolving into private or reserved address space is
    /// refused.
    pub blocking: PrivateIpBlocking,
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
        ClientBuilder::new()
            .http1_title_case_headers()
            .timeout(self.timeout)
            .pool_idle_timeout(self.pool_idle_timeout)
            .redirect(Policy::none())
            .dns_resolver(PrivateIpResolver::new(self.blocking))
    }

    /// The pre-send guard matching this policy.
    #[must_use]
    pub fn guard(&self) -> PrivateIpGuard {
        PrivateIpGuard::new(self.blocking)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::client::{DEFAULT_POOL_IDLE_TIMEOUT, OutboundHttpPolicy};
    use andromeda_configuration::schema::private_ip_blocking::PrivateIpBlocking;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn a_redirect_is_returned_not_followed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener.local_addr().expect("a bound address");
        tokio::spawn(async move {
            if let Ok((mut stream, _peer)) = listener.accept().await {
                let mut buffer = [0_u8; 1024];
                let _read = stream.read(&mut buffer).await;
                let _written = stream
                    .write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            }
        });
        let policy = OutboundHttpPolicy {
            timeout: Duration::from_secs(5),
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
            blocking: PrivateIpBlocking::Allowed,
        };
        let client = policy.client_builder().build().expect("a buildable client");

        let response = client.get(format!("http://{address}/")).send().await.expect("the 302 itself is an answer");

        pretty_assertions::assert_eq!(response.status().as_u16(), 302);
    }
}
