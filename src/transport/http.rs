//! HTTP notification transport.
//!
//! Implements [`NotificationTransport`] for HTTP(S) endpoint delivery using
//! `reqwest::Client` with proper timeout, connection pooling, and header
//! management.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::{debug, warn};

use crate::api::header::{jsonld_context_link, link_header_value};
use crate::transport::types::{
    self, DeliveryOutcome, DeliveryRequest, NotificationTransport, TransportError,
};

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
    pub fn max_payload_size(&self) -> usize {
        self.config.max_payload_size
    }
}

#[async_trait]
impl NotificationTransport for HttpTransport {
    async fn deliver(
        &self,
        req: &DeliveryRequest,
    ) -> Result<DeliveryOutcome, TransportError> {
        if req.payload.len() > self.config.max_payload_size {
            return Err(TransportError::PayloadTooLarge {
                size: req.payload.len(),
                limit: self.config.max_payload_size,
            });
        }

        if self.config.private_ip_blocking {
            // Future: resolve endpoint_uri host and check against private ranges.
            // For now, log that the check is enabled but not yet enforced.
            debug!(
                endpoint = %req.endpoint_uri,
                "private IP blocking enabled (enforcement pending)"
            );
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
        if req.content_type == types::APPLICATION_JSON {
            let ctx_url = req
                .context_url
                .as_deref()
                .unwrap_or(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_URL);
            let link = jsonld_context_link(ctx_url);
            if let Some(value) = link_header_value(&link) {
                request = request.header("Link", value);
            }
        }

        if let Some(ref tenant) = req.tenant {
            request = request.header("NGSILD-Tenant", tenant.as_str());
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
