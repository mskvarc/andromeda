//! Reading and rewriting the URLs a document's `@context` names.

use crate::core_context::{NGSI_LD_CORE_CONTEXT_URL, is_ngsi_ld_core_context_url};
use serde_json::Value;
use std::collections::HashSet;

/// The path prefix under which this broker serves its hosted contexts.
const HOSTED_CONTEXT_PATH: &str = "/ngsi-ld/v1/jsonldContexts/";

/// Whether a term is already an absolute IRI and needs no expansion.
pub(crate) fn is_full_iri(term: &str) -> bool {
    term.starts_with("http://") || term.starts_with("https://") || term.starts_with("urn:")
}

/// Collect every context URL a `@context` node names.
///
/// Used on the failure path, once the document itself has been consumed by
/// the processor and only its context survives for the diagnostic.
pub(crate) fn context_urls_of(context: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    collect_context_urls(context, &mut urls);
    urls
}

/// Walk a `@context` node accumulating the URLs it names.
fn collect_context_urls(value: &Value, urls: &mut Vec<String>) {
    match value {
        Value::String(url) => urls.push(url.clone()), // clone: the document is borrowed and stays valid
        Value::Array(items) => {
            for item in items {
                collect_context_urls(item, urls);
            }
        }
        Value::Object(map) => {
            if let Some(context) = map.get("@context") {
                collect_context_urls(context, urls);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Collapse core-context aliases and repeats for a log line.
///
/// A diagnostic that lists v1.6 and the versionless URL as two separate
/// contexts sends the reader looking for a difference that is not there.
pub(crate) fn canonical_context_urls(urls: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut canonical = Vec::with_capacity(urls.len());
    for url in urls {
        let candidate = if is_ngsi_ld_core_context_url(url) {
            NGSI_LD_CORE_CONTEXT_URL.to_string()
        } else {
            url.clone() // clone: the slice is borrowed and stays valid
        };
        if seen.insert(candidate.clone()) {
            // clone: the set keeps a copy, the list keeps a copy
            canonical.push(candidate);
        }
    }
    canonical
}

/// Rewrite path-relative hosted-context references to the bare identifier.
///
/// A client that read a context from `/ngsi-ld/v1/jsonldContexts/{id}` may
/// echo that path back as its `@context`. The path is not an IRI and the
/// processor cannot resolve it; the identifier alone is a URN, which the
/// loader resolves through the registry and the durable cache.
pub(crate) fn resolve_relative_context_urls(document: &mut Value) {
    match document {
        Value::Object(object) => {
            if let Some(context) = object.get_mut("@context") {
                resolve_relative_entry(context);
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Object(object) = item
                    && let Some(context) = object.get_mut("@context")
                {
                    resolve_relative_entry(context);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Rewrite one `@context` entry, or each member of an array of them.
fn resolve_relative_entry(value: &mut Value) {
    match value {
        Value::String(url) if url.starts_with(HOSTED_CONTEXT_PATH) => {
            *url = url[HOSTED_CONTEXT_PATH.len()..].to_string();
        }
        Value::Array(items) => {
            for item in items {
                resolve_relative_entry(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {}
    }
}

/// Remove `@context` from a compacted document, or from each of its members.
pub(crate) fn strip_context(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("@context");
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Object(object) = item {
                    object.remove("@context");
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core_context::{NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
        engine::context_url::{canonical_context_urls, context_urls_of, is_full_iri, resolve_relative_context_urls, strip_context},
    };
    use serde_json::json;

    #[test]
    fn a_term_counts_as_a_full_iri_only_under_http_https_or_urn() {
        assert!(is_full_iri("https://example.com/Type"));
        assert!(is_full_iri("http://example.com/Type"));
        assert!(is_full_iri("urn:ngsi-ld:Type"));
        assert!(!is_full_iri("Vehicle"));
        assert!(!is_full_iri("ex:Vehicle"));
    }

    #[test]
    fn every_url_a_nested_context_names_is_collected() {
        let context = json!(["https://example.com/a", { "@context": ["https://example.com/b"] }, { "term": "https://example.com/t" }]);
        pretty_assertions::assert_eq!(
            context_urls_of(&context),
            vec!["https://example.com/a".to_string(), "https://example.com/b".to_string()]
        );
    }

    #[test]
    fn core_context_aliases_collapse_to_one_canonical_url_for_a_diagnostic() {
        let urls = vec![
            NGSI_LD_CORE_CONTEXT_V16_URL.to_string(),
            NGSI_LD_CORE_CONTEXT_URL.to_string(),
            "https://example.com/user.jsonld".to_string(),
        ];
        pretty_assertions::assert_eq!(
            canonical_context_urls(&urls),
            vec![NGSI_LD_CORE_CONTEXT_URL.to_string(), "https://example.com/user.jsonld".to_string()]
        );
    }

    #[test]
    fn a_hosted_context_path_is_rewritten_to_the_bare_identifier() {
        let mut document = json!({
            "@context": ["/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc", "https://example.com/keep"],
            "id": "urn:ngsi-ld:Device:1"
        });

        resolve_relative_context_urls(&mut document);

        pretty_assertions::assert_eq!(document["@context"], json!(["urn:ngsi-ld:Context:abc", "https://example.com/keep"]));
    }

    #[test]
    fn stripping_removes_the_context_from_an_object_and_from_every_array_member() {
        let mut object = json!({"@context": "https://example.com/a", "id": "urn:x:1"});
        strip_context(&mut object);
        assert!(object.get("@context").is_none());

        let mut array = json!([{"@context": "https://example.com/a", "id": "urn:x:1"}]);
        strip_context(&mut array);
        assert!(array[0].get("@context").is_none());
    }
}
