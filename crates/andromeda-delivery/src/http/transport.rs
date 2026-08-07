//! Delivering one notification as an HTTP POST.

use crate::{
    error::TransportError,
    http::{
        config::{HttpTransportConfig, PrivateIpBlocking},
        private_ip::{PrivateIpCheck, resolve_private_ip_outcome},
        resolver::PrivateIpResolver,
    },
    outcome::DeliveryOutcome,
    request::DeliveryRequest,
    transport::NotificationTransport,
};
use andromeda_ngsi_ld_http_headers::ngsild_tenant::NgsiLdTenant;
use async_trait::async_trait;
use headers::Header;
use moka::future::Cache;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue, LINK};
use std::time::{Duration, Instant};
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
    /// Short-TTL cache of private-IP guard outcomes keyed by `host:port`.
    ///
    /// Collapses repeated `lookup_host` round trips for fan-out deliveries
    /// without undermining the guard.
    dns_cache: Cache<String, PrivateIpCheck>,
}

impl HttpTransport {
    /// Build the transport, failing if a client cannot be constructed.
    pub fn new(config: HttpTransportConfig) -> Result<HttpTransport, TransportError> {
        let client = reqwest::ClientBuilder::new()
            .http1_title_case_headers()
            .timeout(config.timeout)
            // A notification POST has no business following redirects: a public
            // endpoint answering `302 Location: http://169.254.169.254/…` would
            // otherwise walk the delivery straight past the SSRF guard, which
            // only vets the original URI.
            .redirect(reqwest::redirect::Policy::none())
            // Vet the addresses the client actually connects to, closing the
            // DNS-rebinding window between the pre-send guard's resolution and
            // reqwest's own.
            .dns_resolver(PrivateIpResolver::new(config.private_ip_blocking))
            .build()
            .map_err(|error| {
                if chain_indicates_tls(&error) {
                    TransportError::TlsError {
                        endpoint: "http-client-init".to_string(),
                        detail: error.to_string(),
                    }
                } else {
                    TransportError::ConnectionFailed {
                        endpoint: "http-client-init".to_string(),
                        detail: error.to_string(),
                    }
                }
            })?;
        let dns_cache = Cache::builder().max_capacity(DNS_CHECK_CAPACITY).time_to_live(DNS_CHECK_TTL).build();
        Ok(HttpTransport { client, config, dns_cache })
    }

    /// Refuse delivery when the endpoint host resolves into address space the
    /// broker will not reach on a client's behalf.
    ///
    /// Results are cached for [`DNS_CHECK_TTL`] per `host:port` so a fan-out to
    /// the same endpoint skips the DNS round trip.
    async fn check_private_ip(&self, endpoint_uri: &Url) -> Result<(), TransportError> {
        let host = endpoint_uri.host_str().ok_or_else(|| TransportError::ConnectionFailed {
            endpoint: endpoint_uri.to_string(),
            detail: "URL has no host".to_string(),
        })?;

        let port = endpoint_uri.port_or_known_default().unwrap_or(DEFAULT_HTTP_PORT);
        let addr = format!("{host}:{port}");

        let outcome = match self.dns_cache.get(&addr).await {
            Some(cached) => cached,
            None => {
                let computed = resolve_private_ip_outcome(&addr).await;
                self.dns_cache.insert(addr, computed.clone()).await; // clone: the cache takes one and the caller reads the other
                computed
            }
        };

        match outcome {
            PrivateIpCheck::Allowed => Ok(()),
            PrivateIpCheck::Blocked { ip } => Err(TransportError::PrivateIpBlocked {
                endpoint: endpoint_uri.to_string(),
                ip,
            }),
            PrivateIpCheck::DnsFailed { detail } => Err(TransportError::DnsResolutionFailed {
                endpoint: endpoint_uri.to_string(),
                detail,
            }),
        }
    }

    /// Turn a `reqwest` send failure into the transport failure it was.
    fn send_failure(&self, endpoint: String, error: &reqwest::Error) -> TransportError {
        if error.is_timeout() {
            return TransportError::Timeout {
                endpoint,
                duration: self.config.timeout,
            };
        }
        if chain_indicates_tls(error) {
            TransportError::TlsError {
                endpoint,
                detail: error.to_string(),
            }
        } else {
            TransportError::ConnectionFailed {
                endpoint,
                detail: error.to_string(),
            }
        }
    }
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
fn chain_indicates_tls(error: &(dyn std::error::Error + 'static)) -> bool {
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
fn chain_mentions_tls_terms(error: &(dyn std::error::Error + 'static)) -> bool {
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
        if req.payload.len() > self.config.max_payload_size {
            return Err(TransportError::PayloadTooLarge {
                size: req.payload.len(),
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

        let mut request = self
            .client
            // clone: `reqwest` takes the `Url` by value, and cloning a parsed URL is cheaper than reparsing the string
            .post(req.endpoint_uri.clone())
            .header(CONTENT_TYPE, req.content_type.as_str())
            .body(req.payload.clone()); // clone: `Bytes` is refcounted, and the body takes ownership

        if let Some(link) = req.context_link()
            && let Some(value) = link.to_header_value()
        {
            request = request.header(LINK, value);
        }

        if let Some(tenant) = req.tenant.as_ref() {
            request = request.header(NgsiLdTenant::name(), tenant.as_str());
        }

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
        let response = request.send().await.map_err(|error| self.send_failure(req.endpoint_uri.to_string(), &error))?; // the error path owns the endpoint label it names
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
    use crate::http::transport::chain_indicates_tls;
    use std::io::{Error, ErrorKind};

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
