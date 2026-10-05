//! What an MQTT notification carries instead of headers.

use crate::request::DeliveryRequest;
use andromeda_http_headers::link::field_layout::LinkFieldLayout;

/// The `Link` member, named as the HTTP header it stands in for.
const LINK_KEY: &str = "Link";

/// The tenant member (clause 6.3.14).
///
/// Spelled as the specification spells the header rather than taken from
/// [`andromeda_http_headers::ngsild_tenant::NgsiLdTenant`]: an HTTP
/// header name is lowercased because HTTP treats it case-insensitively, and an
/// MQTT user property and a JSON object member are both case-sensitive.
const TENANT_KEY: &str = "NGSILD-Tenant";

/// The Snapshot branch member (clause 6.3.22).
const SNAPSHOT_KEY: &str = "NGSILD-Snapshot";

/// The metadata an MQTT notification carries because MQTT has no headers.
///
/// Read once for both protocol versions. `Content-Type` is deliberately absent:
/// MQTT 5 has a native content-type property and the envelope has a named
/// member for it, so each spells that one in its own idiom. Everything here has
/// no protocol slot at all — the clause 5.5.7 `@context` link, the
/// subscription's `receiverInfo`, and the tenant.
///
/// The `@context` link and every `receiverInfo` entry named `Link` (in any
/// case, as an HTTP field name is) travel as one `Link` member: the envelope's
/// `metadata` is a JSON object, which holds one member per name, so a second
/// `Link` member would replace the `@context` link rather than join it.
#[must_use]
pub fn transported_headers(request: &DeliveryRequest) -> Vec<(String, String)> {
    let mut headers = Vec::with_capacity(request.receiver_info.len() + 2);

    let context_link = request.context_link().and_then(|link| link.to_header_string());
    let receiver_links = request
        .receiver_info
        .iter()
        .filter(|pair| pair.key.eq_ignore_ascii_case(LINK_KEY))
        .map(|pair| pair.value.as_str());
    if let Some(value) = LinkFieldLayout::combined_value(context_link.as_deref().into_iter().chain(receiver_links)) {
        headers.push((LINK_KEY.to_string(), value));
    }

    for pair in request.receiver_info.iter().filter(|pair| !pair.key.eq_ignore_ascii_case(LINK_KEY)) {
        headers.push((pair.key.to_string(), pair.value.to_string()));
    }

    if let Some(tenant) = request.tenant.as_ref() {
        headers.push((TENANT_KEY.to_string(), tenant.as_str().to_string()));
    }

    if let Some(snapshot) = request.snapshot.as_ref() {
        headers.push((SNAPSHOT_KEY.to_string(), snapshot.as_str().to_string()));
    }

    headers
}
