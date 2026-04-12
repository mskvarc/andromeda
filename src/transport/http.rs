//! HTTP notification transport.
//!
//! Implements [`NotificationTransport`] for HTTP(S) endpoint delivery using
//! `reqwest::Client` with proper timeout, connection pooling, and header
//! management.

use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tracing::{debug, warn};
use url::Url;

use crate::{
    http::header::{link::Link, ngsild_tenant::NgsiLdTenant},
    transport::types::{DeliveryOutcome, DeliveryRequest, NotificationTransport, TransportError},
};
use headers::Header;

/// Configuration for the HTTP notification transport.
#[derive(Debug, Clone)]
pub struct HttpTransportConfig {
    pub timeout: Duration,
    pub max_payload_size: usize,
    pub private_ip_blocking: bool,
}

impl Default for HttpTransportConfig {
    fn default() -> HttpTransportConfig {
        HttpTransportConfig {
            timeout: Duration::from_secs(10),
            max_payload_size: 10 * 1024 * 1024, // 10 MB
            private_ip_blocking: false,
        }
    }
}

/// HTTP notification transport backed by `reqwest::Client`.
pub struct HttpTransport {
    client: reqwest::Client,
    config: HttpTransportConfig,
}

impl HttpTransport {
    pub fn new(config: HttpTransportConfig) -> Result<HttpTransport, TransportError> {
        let client = reqwest::ClientBuilder::new()
            .http1_title_case_headers()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                if e.is_builder() && e.to_string().contains("tls") {
                    TransportError::TlsError {
                        endpoint: "http-client-init".to_string(),
                        detail: e.to_string(),
                    }
                } else {
                    TransportError::ConnectionFailed {
                        endpoint: "http-client-init".to_string(),
                        detail: e.to_string(),
                    }
                }
            })?;
        Ok(HttpTransport { client, config })
    }

    /// Maximum payload size this transport will accept for delivery.
    pub const fn max_payload_size(&self) -> usize {
        self.config.max_payload_size
    }

    /// Resolve the endpoint host and reject delivery if any resolved IP is
    /// private or reserved, preventing SSRF attacks against internal services.
    async fn check_private_ip(&self, endpoint_uri: &str) -> Result<(), TransportError> {
        let url = Url::parse(endpoint_uri).map_err(|e| TransportError::ConnectionFailed {
            endpoint: endpoint_uri.to_string(),
            detail: format!("invalid URL: {}", e),
        })?;

        let host = url.host_str().ok_or_else(|| TransportError::ConnectionFailed {
            endpoint: endpoint_uri.to_string(),
            detail: "URL has no host".to_string(),
        })?;

        let port = url.port_or_known_default().unwrap_or(80);
        let addr_str = format!("{}:{}", host, port);

        let addrs = tokio::net::lookup_host(&addr_str).await.map_err(|e| TransportError::DnsResolutionFailed {
            endpoint: endpoint_uri.to_string(),
            detail: e.to_string(),
        })?;

        for addr in addrs {
            if is_private_ip(&addr.ip()) {
                return Err(TransportError::PrivateIpBlocked {
                    endpoint: endpoint_uri.to_string(),
                    ip: addr.ip().to_string(),
                });
            }
        }

        Ok(())
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

        if self.config.private_ip_blocking {
            self.check_private_ip(&req.endpoint_uri).await?;
        }

        let content_type_str = req.content_type.as_str();

        debug!(
            endpoint = %req.endpoint_uri,
            content_type = content_type_str,
            subscription_id = %req.subscription_id,
            attempt = req.attempt,
            "delivering HTTP notification"
        );

        let mut request = self
            .client
            .post(&req.endpoint_uri)
            .header("Content-Type", content_type_str)
            .body(req.payload.clone());

        // Add Link header for application/json notifications (NGSI-LD spec requirement).
        if req.content_type == crate::http::model::response_format::APPLICATION_JSON {
            let ctx_url = req.context_url.as_deref().unwrap_or(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_URL);
            let link = Link::jsonld_context(ctx_url);
            if let Some(value) = link.to_header_value() {
                request = request.header("Link", value);
            }
        }

        if let Some(ref tenant_value) = req.tenant {
            request = request.header(NgsiLdTenant::name().as_str(), tenant_value.as_str());
        }

        for (key, value) in &req.receiver_info {
            if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                && let Ok(header_value) = reqwest::header::HeaderValue::from_str(value)
            {
                request = request.header(header_name, header_value);
            }
        }

        let start = Instant::now();
        let response = request.send().await.map_err(|e| {
            let endpoint = req.endpoint_uri.clone();
            if e.is_timeout() {
                TransportError::Timeout {
                    endpoint,
                    duration: self.config.timeout,
                }
            } else if e.to_string().contains("tls") || e.to_string().contains("certificate") {
                TransportError::TlsError {
                    endpoint,
                    detail: e.to_string(),
                }
            } else {
                TransportError::ConnectionFailed {
                    endpoint,
                    detail: e.to_string(),
                }
            }
        })?;
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
        // reqwest::Client handles connection pool cleanup on drop.
    }
}

/// Returns `true` if the given IP address is private, reserved, loopback,
/// link-local, or otherwise not routable on the public internet.
///
/// Covers all ranges that an SSRF attacker might target to reach internal
/// services: RFC 1918, loopback, link-local, CGNAT (100.64/10), IPv6 ULA,
/// and IPv6 link-local.
const fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()                          // 127.0.0.0/8
                || v4.is_private()                    // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local()                 // 169.254/16
                || v4.is_unspecified()                // 0.0.0.0
                || v4.is_broadcast()                  // 255.255.255.255
                || v4.octets()[0] == 100              // CGNAT 100.64.0.0/10
                    && (v4.octets()[1] & 0xC0) == 64
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()                          // ::1
                || v6.is_unspecified()                // ::
                || (v6.segments()[0] & 0xFE00) == 0xFC00  // ULA fc00::/7
                || (v6.segments()[0] & 0xFFC0) == 0xFE80 // link-local fe80::/10
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn blocks_loopback_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    }

    #[test]
    fn blocks_private_10_range() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    }

    #[test]
    fn blocks_private_172_16_range() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    }

    #[test]
    fn blocks_private_192_168_range() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn blocks_link_local_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))));
    }

    #[test]
    fn blocks_cgnat_range() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
    }

    #[test]
    fn blocks_unspecified_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    }

    #[test]
    fn blocks_broadcast_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::BROADCAST)));
    }

    #[test]
    fn blocks_loopback_v6() {
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn blocks_ula_v6() {
        // fd00::1 is in the fc00::/7 ULA range
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn blocks_link_local_v6() {
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn allows_public_v4() {
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }

    #[test]
    fn allows_public_v6() {
        assert!(!is_private_ip(&IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888))));
    }

    #[test]
    fn allows_non_cgnat_100_range() {
        // 100.128.0.1 is outside CGNAT (100.64/10 = 100.64-100.127)
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(100, 128, 0, 1))));
    }
}
