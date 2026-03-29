use serde_json::Value;
use tracing::{debug, warn};

use crate::api::header::{jsonld_context_link, link_header_value};

/// Single HTTP notification delivery function used by all workers.
///
/// Sends a POST request with the notification payload, appropriate
/// Content-Type, Link header (for application/json), and custom
/// receiver_info headers.
pub async fn deliver_http(
    client: &reqwest::Client,
    endpoint_uri: &str,
    accept: Option<&str>,
    receiver_info: Option<&[Value]>,
    jsonld_context: Option<&str>,
    tenant: Option<&str>,
    notification: &Value,
) -> bool {
    let content_type = match accept {
        Some("application/ld+json") => "application/ld+json",
        Some("application/geo+json") => "application/geo+json",
        _ => "application/json",
    };

    debug!("Delivering notification to {} with accept={:?}", endpoint_uri, accept);

    let mut request = client.post(endpoint_uri).header("Content-Type", content_type);

    // Add Link header for application/json notifications (NGSI-LD spec requirement)
    if content_type == "application/json" {
        let ctx_url = jsonld_context.unwrap_or(crate::jsonld::NGSI_LD_CORE_CONTEXT_URL);
        let link = jsonld_context_link(ctx_url);
        if let Some(value) = link_header_value(&link) {
            request = request.header("Link", value);
        }
    }

    // Add custom receiver headers
    if let Some(infos) = receiver_info {
        for info in infos {
            if let (Some(key), Some(value)) = (info.get("key").and_then(|v| v.as_str()), info.get("value").and_then(|v| v.as_str())) {
                if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                    if let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) {
                        request = request.header(header_name, header_value);
                    }
                }
            }
        }
    }

    if let Some(tenant) = tenant {
        request = request.header("NGSILD-Tenant", tenant);
    }

    match request.json(notification).send().await {
        Ok(response) => {
            if response.status().is_success() {
                debug!("Notification delivery to {} succeeded ({})", endpoint_uri, response.status());
                true
            } else {
                warn!("Notification delivery to {} returned HTTP {}", endpoint_uri, response.status());
                false
            }
        }
        Err(e) => {
            warn!("Notification delivery to {} failed: {}", endpoint_uri, e);
            false
        }
    }
}
