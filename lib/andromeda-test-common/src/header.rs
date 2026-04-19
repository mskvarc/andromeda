//! HTTP header helpers — `Link`, `Accept`, `Content-Type` — shared across
//! the integration test suites.
//!
//! The Andromeda production code consumes `mediatype::MediaType` everywhere
//! and `demurgos_headers` for `Link`; the test kit deliberately stays at
//! string level so test bodies remain easy to read and trivially diffable
//! against the raw HTTP traces the broker emits.

use reqwest::header::HeaderMap;

/// Build a `Link` header value pointing at a JSON-LD `@context` URL, per
/// NGSI-LD 6.3.17 / RFC 8288.
pub fn jsonld_link_header(context_url: &str) -> String {
    format!("<{context_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// Collect every `NGSILD-Warning` header value from a response into a `Vec`.
///
/// Notifications and distributed-operation responses can carry multiple
/// `NGSILD-Warning` entries (NGSI-LD 6.3.17, warning codes 110/111/199/299).
pub fn warning_header_values(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect()
}
