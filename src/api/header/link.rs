//! RFC 8288 Link header helpers built on typed headers.
//!
//! Provides type-safe construction of Link headers for NGSI-LD responses.

use axum::http::{HeaderMap, HeaderValue};
use demurgos_headers::{
    Header,
    HeaderMapExt,
    link::{Link, LinkValue, RelationType},
};
use mime::Mime;

/// Well-known link relation types used in NGSI-LD.
pub mod rel {
    /// JSON-LD context relation (used for @context in Link header)
    pub fn json_ld_context() -> &'static str {
        crate::jsonld::vocab::JSONLD_CONTEXT_REL.as_str()
    }
}

/// Well-known media types for Link header type parameter.
pub mod media_type {
    pub const JSON_LD: &str = "application/ld+json";
    pub const _JSON: &str = "application/json";
}

fn link_value_with_params(target: &str, rel: Option<&str>, media_type: Option<&str>) -> LinkValue {
    let mut value = LinkValue::new(target.to_string());
    if let Some(rel) = rel
        && let Ok(rel) = rel.parse::<RelationType>()
    {
        value = value.push_rel(rel);
    }
    if let Some(media_type) = media_type
        && let Ok(parsed) = media_type.parse::<Mime>()
    {
        value = value.set_media_type(parsed);
    }
    value
}

/// Build a typed LinkValue with optional rel and type parameters.
pub fn link_value(target: &str, rel: Option<&str>, media_type: Option<&str>) -> LinkValue {
    link_value_with_params(target, rel, media_type)
}

/// Build a Link from a list of LinkValues, returning None if empty.
pub fn link_from_values(values: Vec<LinkValue>) -> Option<Link> {
    if values.is_empty() { None } else { Some(Link::new(values)) }
}

/// Create a single-value JSON-LD context Link header.
pub fn jsonld_context_link(context_url: &str) -> Link {
    Link::new(vec![link_value_with_params(
        context_url,
        Some(rel::json_ld_context()),
        Some(media_type::JSON_LD),
    )])
}

/// Create a multi-value JSON-LD context Link header from an ordered URL list.
pub fn jsonld_context_links(urls: &[String]) -> Option<Link> {
    let values = urls
        .iter()
        .map(|url| link_value_with_params(url, Some(rel::json_ld_context()), Some(media_type::JSON_LD)))
        .collect::<Vec<_>>();
    link_from_values(values)
}

/// Insert JSON-LD context links into a header map as a single Link header.
pub fn insert_jsonld_contexts(headers: &mut HeaderMap, urls: &[String]) {
    if let Some(link) = jsonld_context_links(urls) {
        headers.typed_insert(link);
    }
}

/// Encode a typed Link header into a raw HeaderValue.
///
/// Returns None if encoding yields no values.
pub fn link_header_value(link: &Link) -> Option<HeaderValue> {
    let mut values = Vec::new();
    link.encode(&mut values);
    if values.is_empty() {
        return None;
    }
    if values.len() == 1 {
        return values.into_iter().next();
    }
    let mut combined = String::new();
    for (idx, value) in values.iter().enumerate() {
        let part = value.to_str().ok()?;
        if idx > 0 {
            combined.push_str(", ");
        }
        combined.push_str(part);
    }
    HeaderValue::from_str(&combined).ok()
}

/// Encode a typed Link header into a string, if possible.
pub fn link_header_string(link: &Link) -> Option<String> {
    link_header_value(link).and_then(|v| v.to_str().ok().map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonld_context_link_encodes() {
        let link = jsonld_context_link("https://example.com/context.jsonld");
        let raw = link_header_string(&link).unwrap();
        assert!(raw.contains("<https://example.com/context.jsonld>"));
        assert!(raw.contains("rel=\"http://www.w3.org/ns/json-ld#context\""));
        assert!(raw.contains("type=\"application/ld+json\""));
    }

    #[test]
    fn multi_value_link_encodes() {
        let values = vec![
            link_value("https://example.com/one", Some("prev"), None),
            link_value("https://example.com/two", Some("next"), None),
        ];
        let link = link_from_values(values).unwrap();
        let raw = link_header_string(&link).unwrap();
        assert!(raw.contains("<https://example.com/one>"));
        assert!(raw.contains("<https://example.com/two>"));
    }

    #[test]
    fn empty_values_return_none() {
        assert!(link_from_values(Vec::new()).is_none());
    }
}
