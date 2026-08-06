//! Delivering one notification as an HTTP POST.

use crate::{
    error::TransportError,
    http::{
        config::{HttpTransportConfig, PrivateIpBlocking},
        private_ip::{PrivateIpCheck, resolve_private_ip_outcome},
    },
    outcome::DeliveryOutcome,
    request::DeliveryRequest,
    transport::NotificationTransport,
};
use andromeda_ngsi_ld_http_headers::ngsild_tenant::NgsiLdTenant;
use async_trait::async_trait;
use headers::Header;
use moka::future::Cache;
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
            .build()
            .map_err(|error| {
                if error.is_builder() && error.to_string().contains("tls") {
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

    /// Maximum payload size this transport will accept for delivery.
    pub const fn max_payload_size(&self) -> usize {
        self.config.max_payload_size
    }

    /// Refuse delivery when the endpoint host resolves into address space the
    /// broker will not reach on a client's behalf.
    ///
    /// Results are cached for [`DNS_CHECK_TTL`] per `host:port` so a fan-out to
    /// the same endpoint skips the DNS round trip.
    async fn check_private_ip(&self, endpoint_uri: &str) -> Result<(), TransportError> {
        let url = Url::parse(endpoint_uri).map_err(|error| TransportError::ConnectionFailed {
            endpoint: endpoint_uri.to_string(),
            detail: format!("invalid URL: {error}"),
        })?;

        let host = url.host_str().ok_or_else(|| TransportError::ConnectionFailed {
            endpoint: endpoint_uri.to_string(),
            detail: "URL has no host".to_string(),
        })?;

        let port = url.port_or_known_default().unwrap_or(DEFAULT_HTTP_PORT);
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
        let detail = error.to_string();
        if detail.contains("tls") || detail.contains("certificate") {
            TransportError::TlsError { endpoint, detail }
        } else {
            TransportError::ConnectionFailed { endpoint, detail }
        }
    }
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
            attempt = req.attempt,
            "delivering HTTP notification"
        );

        let mut request = self
            .client
            .post(&req.endpoint_uri)
            .header("Content-Type", req.content_type.as_str())
            .body(req.payload.clone()); // clone: `Bytes` is refcounted, and the body takes ownership

        if let Some(link) = req.context_link()
            && let Some(value) = link.to_header_value()
        {
            request = request.header("Link", value);
        }

        if let Some(tenant) = req.tenant.as_ref() {
            request = request.header(NgsiLdTenant::name().as_str(), tenant.as_str());
        }

        for pair in &req.receiver_info {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(pair.key.as_bytes())
                && let Ok(value) = reqwest::header::HeaderValue::from_str(&pair.value)
            {
                request = request.header(name, value);
            }
        }

        let start = Instant::now();
        let response = request.send().await.map_err(|error| self.send_failure(req.endpoint_uri.clone(), &error))?; // clone: the error path owns the endpoint it names
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

    async fn shutdown(&self) {
        // `reqwest::Client` cleans its connection pool up on drop.
    }
}
