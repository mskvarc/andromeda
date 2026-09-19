//! Shared helpers for the opaque-cursor pagination suites (clause 5.5.9.1).
//!
//! Every cursor suite reads the same `Link` header, extracts the same opaque
//! `cursor` tokens, and walks the collection the same way, so those are written
//! once here. Only the endpoint-specific query call differs, and each suite owns
//! that.

use reqwest::header::HeaderMap;
use serde_json::Value;

/// The JSON-LD `@context` `Link` header a raw request carries so the broker
/// expands the response terms against the core context.
pub const fn context_link_rel() -> &'static str {
    "http://www.w3.org/ns/json-ld#context"
}

/// Every `Link` header value the response carries, joined for inspection.
pub fn joined_links(headers: &HeaderMap) -> String {
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The target URL of the `rel="<rel>"` pagination link, if the response carries
/// one. `Link` values are comma-separated and the cursor tokens are base64url,
/// so a comma never appears inside a pagination target here.
pub fn link_target(headers: &HeaderMap, rel: &str) -> Option<String> {
    let needle = format!("rel=\"{rel}\"");
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|header| header.split(','))
        .find(|value| value.contains(&needle))
        .and_then(|value| {
            let start = value.find('<')? + 1;
            let end = value[start..].find('>')? + start;
            Some(value[start..end].to_string())
        })
}

/// The query-string portion of a link target, which is what a query helper
/// takes.
pub fn query_string_of(target: &str) -> &str {
    target.split_once('?').map_or("", |(_, query)| query)
}

/// The `id` of every element in a collection response body, in order.
pub fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a collection response is a JSON array")
        .iter()
        .map(|element| element.get("id").and_then(Value::as_str).expect("every element carries an id").to_string())
        .collect()
}

/// Assert that a response's pagination links are the opaque-cursor kind: they
/// carry a `cursor` and never an `offset`.
pub fn assert_cursor_links(headers: &HeaderMap) {
    let links = joined_links(headers);
    assert!(links.contains("cursor="), "cursor-mode links carry a cursor: {links}");
    assert!(!links.contains("offset="), "cursor-mode links never carry an offset: {links}");
}
