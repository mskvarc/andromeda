//! Two rewrites every context document passes through before it is used.
//!
//! Both exist because a context can name itself, or name the core context,
//! in a form the processor would otherwise resolve a second time. Neither is
//! a JSON-LD rule: they are what this broker does with the URLs it hosts and
//! with the core context it already has in memory.

use crate::core_context::is_ngsi_ld_core_context_url;
use serde_json::Value;

/// The doubled API prefix a self-referencing hosted context arrives with.
const DUPLICATED_API_PREFIX_PATH: &str = "/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/";
/// The prefix clause 6.3.18 actually defines.
const CANONICAL_API_PREFIX_PATH: &str = "/ngsi-ld/v1/jsonldContexts/";

/// Collapse a doubled `/ngsi-ld/v1` prefix in a locally hosted context URL.
///
/// A context this broker hosts and serves can come back with the API prefix
/// applied twice; resolving it verbatim would miss the registry entry keyed by
/// the canonical URL. Only local hosts are rewritten — a remote server's path
/// is that server's business.
pub(crate) fn normalize_local_hosted_context_url(url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let Some(host) = parsed.host_str() else {
        return url.to_string();
    };
    if !is_local_host(host) {
        return url.to_string();
    }

    let path = parsed.path();
    if !path.contains(DUPLICATED_API_PREFIX_PATH) {
        return url.to_string();
    }

    let normalized = path.replacen(DUPLICATED_API_PREFIX_PATH, CANONICAL_API_PREFIX_PATH, 1);
    parsed.set_path(&normalized);
    parsed.to_string()
}

/// Whether a host names this broker.
fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Strip embedded references to the NGSI-LD core context from a loaded
/// document.
///
/// A user context that imports the core context would make the processor
/// resolve the core a second time and process a definition the embedded copy
/// already carries. The core context document itself is returned untouched,
/// because that one *is* the definition.
pub(crate) fn normalize_loaded_context_document(url: &str, value: jsonld_syntax::Value) -> jsonld_syntax::Value {
    if is_ngsi_ld_core_context_url(url) {
        return value;
    }

    let mut json = jsonld_syntax::Value::into_serde_json(value);
    remove_embedded_core_context_aliases(&mut json);
    jsonld_syntax::Value::from_serde_json(json)
}

/// Walk a document and strip core-context references from every `@context`.
fn remove_embedded_core_context_aliases(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(context) = map.get_mut("@context") {
                strip_core_aliases_from_context_slot(context);
            }
            for inner in map.values_mut() {
                remove_embedded_core_context_aliases(inner);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_embedded_core_context_aliases(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Strip core-context references from one `@context` slot.
fn strip_core_aliases_from_context_slot(context: &mut Value) {
    match context {
        Value::String(url) if is_ngsi_ld_core_context_url(url) => {
            *context = Value::Array(Vec::new());
        }
        Value::Array(items) => {
            items.retain(|item| !item.as_str().is_some_and(is_ngsi_ld_core_context_url));
            for item in items {
                remove_embedded_core_context_aliases(item);
            }
        }
        Value::Object(_) => {
            remove_embedded_core_context_aliases(context);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::url_normalization::{normalize_loaded_context_document, normalize_local_hosted_context_url};
    use serde_json::json;

    #[test]
    fn a_locally_hosted_context_url_with_a_doubled_api_prefix_collapses_to_the_canonical_one() {
        let url = "http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc";
        pretty_assertions::assert_eq!(
            normalize_local_hosted_context_url(url),
            "http://localhost:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc"
        );
    }

    #[test]
    fn a_remote_host_keeps_whatever_path_it_serves() {
        let url = "https://forge.etsi.org/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc";
        pretty_assertions::assert_eq!(normalize_local_hosted_context_url(url), url);
    }

    #[test]
    fn a_user_context_importing_the_core_context_loses_that_import() {
        let input = jsonld_syntax::Value::from_serde_json(json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "location": "https://example.com/location" }
            ]
        }));

        let normalized = normalize_loaded_context_document("https://example.com/compound.jsonld", input);

        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json(normalized),
            json!({ "@context": [{ "location": "https://example.com/location" }] })
        );
    }

    #[test]
    fn the_core_context_document_itself_is_left_alone() {
        let core_url = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
        let payload = json!({
            "@context": [
                core_url,
                { "location": "https://example.com/location" }
            ]
        });
        let input = jsonld_syntax::Value::from_serde_json(payload.clone()); // clone: the expectation compares against the input

        let normalized = normalize_loaded_context_document(core_url, input);

        pretty_assertions::assert_eq!(jsonld_syntax::Value::into_serde_json(normalized), payload);
    }
}
