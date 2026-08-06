//! What an MQTT notification carries instead of headers.

use crate::request::DeliveryRequest;

/// The `Link` member, named as the HTTP header it stands in for.
const LINK_KEY: &str = "Link";

/// The tenant member (clause 6.3.14).
///
/// Spelled as the specification spells the header rather than taken from
/// [`andromeda_ngsi_ld_http_headers::ngsild_tenant::NgsiLdTenant`]: an HTTP
/// header name is lowercased because HTTP treats it case-insensitively, and an
/// MQTT user property and a JSON object member are both case-sensitive.
const TENANT_KEY: &str = "NGSILD-Tenant";

/// The metadata an MQTT notification carries because MQTT has no headers.
///
/// Read once for both protocol versions. `Content-Type` is deliberately absent:
/// MQTT 5 has a native content-type property and the envelope has a named
/// member for it, so each spells that one in its own idiom. Everything here has
/// no protocol slot at all — the clause 5.5.7 `@context` link, the
/// subscription's `receiverInfo`, and the tenant.
pub fn transported_headers(request: &DeliveryRequest) -> Vec<(String, String)> {
    let mut headers = Vec::with_capacity(request.receiver_info.len() + 2);

    if let Some(link) = request.context_link()
        && let Some(value) = link.to_header_string()
    {
        headers.push((LINK_KEY.to_string(), value));
    }

    for pair in &request.receiver_info {
        headers.push((pair.key.to_string(), pair.value.to_string()));
    }

    if let Some(tenant) = request.tenant.as_ref() {
        headers.push((TENANT_KEY.to_string(), tenant.as_str().to_string()));
    }

    headers
}
