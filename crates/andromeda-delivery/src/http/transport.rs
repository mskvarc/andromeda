//! Delivering one notification as an HTTP POST.

use crate::{
    delivery_deadline::delivery_deadline,
    error::TransportError,
    host_port::HostPort,
    http::{
        config::{HttpTransportConfig, PrivateIpBlocking},
        private_ip::{PrivateIpCheck, resolve_private_ip_outcome},
        resolver::PrivateIpResolver,
    },
    outcome::{DeliveryOutcome, delivery_metric_outcome},
    request::DeliveryRequest,
    transport::NotificationTransport,
};
use andromeda_telemetry::delivery_ops::DeliveryTimer;
use async_trait::async_trait;
use headers::HeaderMapExt;
use moka::future::Cache;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
};
use std::{
    error::Error,
    iter,
    time::{Duration, Instant},
};
use tracing::{debug, warn};
use url::Url;

/// TTL for the DNS private-IP check cache.
///
/// Short by design: keeps the SSRF guard effective against DNS rebinding while
/// still eliminating the per-delivery `lookup_host` round trip for hot
/// notification endpoints.
const DNS_CHECK_TTL: Duration = Duration::from_secs(60);

/// Maximum entries in the DNS check cache.
const DNS_CHECK_CAPACITY: u64 = 1024;

/// Default port assumed when an endpoint URI names neither one nor a scheme
/// with a known default.
const DEFAULT_HTTP_PORT: u16 = 80;

/// HTTP notification transport backed by `reqwest::Client`.
pub struct HttpTransport {
    /// The client every delivery goes out on, pooling connections across
    /// endpoints.
    client: reqwest::Client,
    /// Timeout, size limit and SSRF policy.
    config: HttpTransportConfig,
    /// Short-TTL cache of private-IP guard outcomes keyed by the endpoint
    /// [`HostPort`].
    ///
    /// Collapses repeated `lookup_host` round trips for fan-out deliveries
    /// without undermining the guard.
    dns_cache: Cache<HostPort, PrivateIpCheck>,
}

impl HttpTransport {
    /// Build the transport, failing if a client cannot be constructed.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::ClientInitializationFailed`] when `reqwest`
    /// refuses the client configuration — a TLS backend that cannot be
    /// initialised, or a resolver the runtime will not accept.
    pub fn new(config: HttpTransportConfig) -> Result<HttpTransport, TransportError> {
        let client = reqwest::ClientBuilder::new()
            .http1_title_case_headers()
            .timeout(config.timeout)
            .pool_idle_timeout(config.pool_idle_timeout)
            // A notification POST has no business following redirects: a public
            // endpoint answering `302 Location: http://169.254.169.254/…` would
            // otherwise walk the delivery straight past the SSRF guard, which
            // only vets the original URI.
            .redirect(Policy::none())
            // Vet the addresses the client actually connects to, closing the
            // DNS-rebinding window between the pre-send guard's resolution and
            // reqwest's own.
            .dns_resolver(PrivateIpResolver::new(config.private_ip_blocking))
            .build()
            .map_err(|error| TransportError::ClientInitializationFailed { detail: error.to_string() })?;
        let dns_cache = Cache::builder().max_capacity(DNS_CHECK_CAPACITY).time_to_live(DNS_CHECK_TTL).build();
        Ok(HttpTransport { client, config, dns_cache })
    }

    /// Refuse delivery when the endpoint host resolves into address space the
    /// broker will not reach on a client's behalf.
    ///
    /// Results are cached for [`DNS_CHECK_TTL`] per endpoint [`HostPort`] so a
    /// fan-out to the same endpoint skips the DNS round trip.
    async fn check_private_ip(&self, endpoint_uri: &Url) -> Result<(), TransportError> {
        let host = endpoint_uri
            .host()
            .ok_or_else(|| TransportError::ConnectionFailed {
                endpoint: endpoint_uri.clone(), // clone: the error owns the endpoint it names
                detail: "URL has no host".to_string(),
            })?
            .to_owned();
        let port = endpoint_uri.port_or_known_default().unwrap_or(DEFAULT_HTTP_PORT);
        let authority = HostPort::new(host, port);

        let outcome = if let Some(cached) = self.dns_cache.get(&authority).await {
            cached
        } else {
            let computed = resolve_private_ip_outcome(&authority).await;
            self.dns_cache.insert(authority, computed.clone()).await; // clone: the cache takes one and the caller reads the other
            computed
        };

        match outcome {
            PrivateIpCheck::Allowed => Ok(()),
            PrivateIpCheck::Blocked { ip } => Err(TransportError::PrivateIpBlocked {
                endpoint: endpoint_uri.clone(), // clone: the error owns the endpoint it names
                ip,
            }),
            PrivateIpCheck::DnsFailed { detail } => Err(TransportError::DnsResolutionFailed {
                endpoint: endpoint_uri.clone(), // clone: same
                detail,
            }),
        }
    }
}

/// Turn a `reqwest` send failure into the transport failure it was.
///
/// `deadline` is the bound the attempt actually ran under: the endpoint's own
/// Table 5.2.15-1 `timeout` where it named one shorter than the system bound,
/// and the system bound otherwise, so a reported timeout names the deadline
/// that expired rather than one that did not apply.
fn send_failure(endpoint: Url, deadline: Duration, error: &reqwest::Error) -> TransportError {
    if error.is_timeout() {
        return TransportError::Timeout { endpoint, duration: deadline };
    }
    if chain_indicates_tls(error) {
        TransportError::TlsError {
            endpoint,
            detail: error_chain_text(error),
        }
    } else {
        TransportError::ConnectionFailed {
            endpoint,
            detail: error_chain_text(error),
        }
    }
}

/// The message of `error` followed by that of every error below it.
///
/// `reqwest`'s own message names only the request that failed ("error sending
/// request for url ..."); whether the connection was refused, reset, or closed
/// before a response is said by the errors below it.
fn error_chain_text(error: &(dyn Error + 'static)) -> String {
    iter::successors(Some(error), |&current| current.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

/// Whether a send failure is a TLS handshake or certificate failure.
///
/// `reqwest` exposes no `is_tls()` and abstracts the TLS backend away, so the
/// classification walks the typed error source chain — the same idiom
/// `reqwest::Error::is_connect` uses internally to find a `hyper` error. This
/// build links `reqwest` against `rustls` (see the crate's Cargo.lock), so a
/// handshake or certificate failure surfaces as a [`rustls::Error`] somewhere
/// below `reqwest`'s own error; downcasting to it is immune to the spelling and
/// capitalisation changes that broke the previous `Display`-substring check. The
/// text pass is only a backstop for a future wrapping that erases the type
/// before it reaches the chain, and the unit tests pin both paths.
fn chain_indicates_tls(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if source.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        current = source.source();
    }
    chain_mentions_tls_terms(error)
}

/// Backstop for [`chain_indicates_tls`]: a case-insensitive scan of the source
/// chain for the vocabulary `rustls` uses, for the case where the typed error is
/// wrapped into an opaque form before it reaches the chain.
fn chain_mentions_tls_terms(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        let text = source.to_string().to_ascii_lowercase();
        if text.contains("tls") || text.contains("certificate") || text.contains("handshake") || text.contains("rustls") {
            return true;
        }
        current = source.source();
    }
    false
}

#[async_trait]
impl NotificationTransport for HttpTransport {
    async fn deliver(&self, req: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        // One delivery attempt, timed and counted as a whole: the bracket spans
        // serialisation, the SSRF guard and the send, which is the latency a
        // notification actually incurs.
        let timer = DeliveryTimer::start();
        let result = self.deliver_inner(req).await;
        timer.finish(delivery_metric_outcome(&result));
        result
    }
}

impl HttpTransport {
    /// The delivery body itself, wrapped by [`HttpTransport::deliver`] for timing.
    async fn deliver_inner(&self, req: &DeliveryRequest) -> Result<DeliveryOutcome, TransportError> {
        // The body is serialised here, once, so the size limit is checked
        // against the bytes actually sent rather than an estimate.
        let body = serde_json::to_vec(req.payload.as_ref()).map_err(|source| TransportError::PayloadSerialization {
            endpoint: req.endpoint_uri.clone(), // clone: the error owns the endpoint it names
            source,
        })?;
        if body.len() > self.config.max_payload_size {
            return Err(TransportError::PayloadTooLarge {
                size: body.len(),
                limit: self.config.max_payload_size,
            });
        }

        if self.config.private_ip_blocking == PrivateIpBlocking::Enabled {
            self.check_private_ip(&req.endpoint_uri).await?;
        }

        debug!(
            endpoint = %req.endpoint_uri,
            content_type = %req.content_type,
            subscription_id = %req.subscription_id,
            "delivering HTTP notification"
        );

        let mut header_map = HeaderMap::new();
        header_map.typed_insert(req.content_type);
        if let Some(link) = req.context_link() {
            header_map.typed_insert(link);
        }
        if let Some(tenant) = req.tenant.as_ref() {
            header_map.typed_insert(tenant.clone());
        }
        if let Some(snapshot) = req.snapshot.as_ref() {
            header_map.typed_insert(snapshot.clone());
        }
        // Table 5.2.15-1: the endpoint's `timeout` is the "maximum period of
        // time in milliseconds which may elapse before a notification is
        // assumed to have failed", and "the NGSI-LD system can override this
        // value" — which the broker-wide bound below does, in the one direction
        // an override runs. The row also confines the member to a binding whose
        // "protocol always returns a response".
        let deadline = delivery_deadline(req.timeout, self.config.timeout);
        let mut request = self
            .client
            // clone: `reqwest` takes the `Url` by value, and cloning a parsed URL is cheaper than reparsing the string
            .post(req.endpoint_uri.clone())
            .headers(header_map)
            .timeout(deadline)
            .body(body); // the serialised notification, moved into the request

        for pair in &req.receiver_info {
            match (HeaderName::from_bytes(pair.key.as_bytes()), HeaderValue::from_str(&pair.value)) {
                (Ok(name), Ok(value)) => request = request.header(name, value),
                // A receiverInfo entry that cannot become an HTTP header is not
                // dropped in silence: a discarded `Authorization` header would
                // deliver the notification unauthenticated, which the receiver
                // would refuse or, worse, accept as anonymous. Surface it so the
                // misconfiguration is visible rather than a quiet auth failure.
                (Err(_), _) | (_, Err(_)) => {
                    warn!(
                        subscription_id = %req.subscription_id,
                        endpoint = %req.endpoint_uri,
                        header_key = %pair.key,
                        "dropping receiverInfo entry that is not a valid HTTP header name/value"
                    );
                }
            }
        }

        let start = Instant::now();
        let response = request.send().await.map_err(|error| send_failure(req.endpoint_uri.clone(), deadline, &error))?; // clone: the error path owns the endpoint it names
        let status = response.status().as_u16();
        let duration = start.elapsed();

        if response.status().is_success() {
            debug!(
                endpoint = %req.endpoint_uri,
                status,
                ?duration,
                "HTTP notification delivery succeeded"
            );
            Ok(DeliveryOutcome::Success {
                status_code: Some(status),
                duration,
            })
        } else {
            warn!(
                endpoint = %req.endpoint_uri,
                status,
                "HTTP notification delivery rejected"
            );
            Ok(DeliveryOutcome::Rejected {
                reason: format!("HTTP {status}"),
                retryable: status >= 500 || status == 429,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        http::{
            config::HttpTransportConfig,
            transport::{HttpTransport, chain_indicates_tls, error_chain_text},
        },
        outcome::DeliveryOutcome,
        request::NotificationDelivery,
        transport::NotificationTransport,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::{
        io::{Error, ErrorKind},
        str::FromStr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        time::sleep,
    };
    use url::Url;

    /// Reads one request head and its `Content-Length` body from `stream`,
    /// answering `None` when the peer closes first.
    async fn read_request(stream: &mut TcpStream) -> Option<()> {
        let mut received = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await.ok()?;
            if read == 0 {
                return None;
            }
            received.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&received);
            if let Some(head_end) = text.find("\r\n\r\n") {
                let length = text[..head_end]
                    .lines()
                    .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().to_owned()))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_default();
                if received.len() >= head_end + 4 + length {
                    return Some(());
                }
            }
        }
    }

    /// A receiver that answers the first request on each connection and keeps
    /// the connection open, but drops any later request sent over it without
    /// answering -- a peer that has abandoned an idle keep-alive connection
    /// which the client still holds as open. Counts the connections accepted.
    async fn receiver_abandoning_idle_connections() -> (Url, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback listener binds");
        let address = listener.local_addr().expect("the listener has an address");
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    if read_request(&mut stream).await.is_some() {
                        let _written = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n").await;
                        let _next = read_request(&mut stream).await;
                    }
                });
            }
        });
        (Url::parse(&format!("http://{address}/notify")).expect("a well-formed endpoint"), accepted)
    }

    async fn deliver(transport: &HttpTransport, endpoint: &Url) -> DeliveryOutcome {
        let subscription = Uri::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id");
        let request = NotificationDelivery {
            payload: &json!({"id": "urn:ngsi-ld:Notification:1"}),
            endpoint_uri: endpoint,
            accept: None,
            receiver_info: None,
            jsonld_context: None,
            tenant: None,
            snapshot: None,
            timeout: None,
            subscription_id: &subscription,
        }
        .assemble();
        transport.deliver(&request).await.expect("the notification is delivered")
    }

    /// A connection idle for longer than the pool keeps it is not reused, so a
    /// notification after a quiet period reaches a receiver that abandoned the
    /// idle connection instead of failing on it.
    #[tokio::test]
    async fn a_connection_idle_past_the_pool_timeout_is_not_reused_for_the_next_notification() {
        let (endpoint, accepted) = receiver_abandoning_idle_connections().await;
        let transport = HttpTransport::new(HttpTransportConfig {
            pool_idle_timeout: Duration::from_millis(100),
            ..HttpTransportConfig::default()
        })
        .expect("the transport builds");

        let first = deliver(&transport, &endpoint).await;
        sleep(Duration::from_millis(400)).await;
        let second = deliver(&transport, &endpoint).await;

        assert!(matches!(first, DeliveryOutcome::Success { .. }), "{first:?}");
        assert!(matches!(second, DeliveryOutcome::Success { .. }), "{second:?}");
        pretty_assertions::assert_eq!(accepted.load(Ordering::SeqCst), 2, "the second notification opened a fresh connection");
    }

    /// An error that names only the request, as `reqwest`'s own does.
    #[derive(Debug, thiserror::Error)]
    #[error("error sending request")]
    struct SendingRequest(#[source] Error);

    #[test]
    fn a_failure_detail_names_every_cause_below_the_outermost_error() {
        let outer = SendingRequest(Error::new(ErrorKind::ConnectionReset, "connection reset by peer"));
        pretty_assertions::assert_eq!(error_chain_text(&outer), "error sending request: connection reset by peer");
    }

    #[test]
    fn a_rustls_error_anywhere_in_the_chain_is_classified_as_tls_by_type() {
        // The typed path: a rustls error nested below an io::Error, found by
        // downcast rather than by reading its message.
        let rustls_error = rustls::Error::General("peer sent no certificates".to_string());
        let wrapped = Error::other(rustls_error);
        assert!(chain_indicates_tls(&wrapped), "a rustls::Error in the chain is a TLS error");
    }

    #[test]
    fn a_type_erased_tls_failure_is_still_caught_by_the_text_backstop() {
        // rustls capitalises these; if the type is erased before reaching the
        // chain, the case-insensitive backstop still recognises the vocabulary.
        let handshake = Error::other("received fatal alert: HandshakeFailure");
        assert!(chain_indicates_tls(&handshake), "a rustls HandshakeFailure is a TLS error");

        let certificate = Error::other("invalid peer certificate: Expired");
        assert!(chain_indicates_tls(&certificate), "a certificate rejection is a TLS error");
    }

    #[test]
    fn a_plain_connection_refusal_is_not_classified_as_tls() {
        let refused = Error::new(ErrorKind::ConnectionRefused, "Connection refused (os error 61)");
        assert!(!chain_indicates_tls(&refused), "a connection refusal is not a TLS error");
    }
}
