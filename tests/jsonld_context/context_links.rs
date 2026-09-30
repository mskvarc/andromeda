//! The JSON-LD context `Link` a request names and a response carries.
//!
//! Clause 6.3.5 reads a request's `@context` from "a Link header [...] as
//! mandated by JSON-LD", and clause 6.3.6 has an `application/json` response
//! name its `@context` the same way; JSON-LD 1.1 section 6.1 allows exactly one
//! such link. A response's context links are counted across every `Link`
//! field line, since IETF RFC 8288 section 3 makes every link-value of a field
//! a link of its own.

use reqwest::header::HeaderMap;

/// The JSON-LD context relation.
pub const CONTEXT_RELATION: &str = "http://www.w3.org/ns/json-ld#context";

/// A `Link` value naming `target` as the JSON-LD context, with the relation
/// spelled `relation`.
pub fn link_with_relation(target: &str, relation: &str) -> String {
    format!(r#"<{target}>; rel="{relation}"; type="application/ld+json""#)
}

/// A `Link` value naming `target` as the JSON-LD context.
pub fn context_link(target: &str) -> String {
    link_with_relation(target, CONTEXT_RELATION)
}

/// A clause 6.3.21 profile `Link` value, which names no context.
pub fn profile_link() -> String {
    r#"<https://uri.etsi.org/ngsi-ld/v1.9>; rel="profile""#.to_owned()
}

/// The target of every JSON-LD context link-value across all `Link` lines of
/// `headers`, in order. The relation is matched case-insensitively (IETF RFC
/// 8288 section 2.1.2).
pub fn context_link_targets(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("link")
        .iter()
        .flat_map(|value| value.to_str().expect("a visible-ASCII Link").split(',').map(str::to_owned).collect::<Vec<_>>())
        .filter(|link_value| link_value.to_ascii_lowercase().contains(CONTEXT_RELATION))
        .map(|link_value| {
            let start = link_value.find('<').expect("a link target") + 1;
            let end = link_value.find('>').expect("a closed link target");
            link_value[start..end].to_owned()
        })
        .collect()
}

/// The one JSON-LD context link target `headers` carry, asserting there is
/// exactly one (JSON-LD 1.1 section 6.1).
pub fn single_context_link(headers: &HeaderMap) -> String {
    let targets = context_link_targets(headers);
    assert_eq!(targets.len(), 1, "exactly one context link: {targets:?}");
    targets.into_iter().next().expect("one context link")
}
