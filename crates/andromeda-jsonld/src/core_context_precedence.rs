//! Keeping the NGSI-LD Core @context authoritative and singular.
//!
//! Clauses 4.4 and 5.5.7 of ETSI GS CIM 009 v1.9.1 make the Core @context
//! protected, immutable, and possessed of "absolute precedence, regardless of
//! the position of the Core @context in the @context array". A broker realises
//! that by applying the Core @context exactly once, in the last (authoritative)
//! position. [`crate::document_context`] does this for the `@context` array it
//! assembles for each operation: it drops any core reference the request
//! carried, wherever the request placed it, then appends the canonical core
//! pair last.
//!
//! This module is the same policy one layer deeper. A context *document* the
//! loader fetches, or the operator preloads, may itself import the Core
//! @context — a user context written as `["<core-url>", { ...user terms... }]`.
//! The array-level policy above only ever sees the URL of such a document, not
//! the core reference nested inside it, so once the broker appends the
//! authoritative Core last, the document's embedded copy is an *earlier*,
//! protected occurrence. A conforming JSON-LD processor then rejects the later
//! authoritative copy as a `ProtectedTermRedefinition` instead of letting it
//! take precedence. Removing the embedded reference at load time is what keeps
//! the Core singular and last, so a user context that redefines a core term
//! resolves to the Core's definition — the Core wins, exactly as 5.5.7 requires
//! — rather than failing.
//!
//! This is the spec-mandated Core-precedence obligation, not a workaround: it is
//! how a broker built on an off-the-shelf JSON-LD processor meets clause 5.5.7.

use crate::core_context::is_ngsi_ld_core_context_url;
use serde_json::Value;

/// A loaded context document with any embedded Core @context reference removed,
/// so the authoritative Core the broker appends last stays the single
/// occurrence.
///
/// The Core @context document itself is returned untouched — that one *is* the
/// definition, not a redundant import of it.
pub(crate) fn without_embedded_core_context(url: &str, value: jsonld_syntax::Value) -> jsonld_syntax::Value {
    if is_ngsi_ld_core_context_url(url) {
        return value;
    }

    let mut json = jsonld_syntax::Value::into_serde_json(value);
    remove_embedded_core_references(&mut json);
    jsonld_syntax::Value::from_serde_json(json)
}

/// Walk a document and drop Core @context references from every `@context`.
fn remove_embedded_core_references(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(context) = map.get_mut("@context") {
                strip_core_references_from_context_slot(context);
            }
            for inner in map.values_mut() {
                remove_embedded_core_references(inner);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_embedded_core_references(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Drop Core @context references from one `@context` slot.
///
/// Every core reference is dropped, so a slot that names the Core twice is left
/// with none: the broker supplies the one authoritative copy, last.
fn strip_core_references_from_context_slot(context: &mut Value) {
    match context {
        Value::String(url) if is_ngsi_ld_core_context_url(url) => {
            *context = Value::Array(Vec::new());
        }
        Value::Array(items) => {
            items.retain(|item| !item.as_str().is_some_and(is_ngsi_ld_core_context_url));
            for item in items {
                remove_embedded_core_references(item);
            }
        }
        Value::Object(_) => {
            remove_embedded_core_references(context);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::core_context_precedence::without_embedded_core_context;
    use serde_json::json;

    #[test]
    fn a_user_context_importing_the_core_loses_only_that_redundant_import() {
        // Clause 5.5.7: the broker appends the authoritative Core last, so a
        // user context's own embedded Core reference is redundant and is
        // dropped, while its genuine term definitions are kept untouched.
        let input = jsonld_syntax::Value::from_serde_json(json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "location": "https://example.com/location" }
            ]
        }));

        let normalized = without_embedded_core_context("https://example.com/compound.jsonld", input);

        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json(normalized),
            json!({ "@context": [{ "location": "https://example.com/location" }] })
        );
    }

    #[test]
    fn a_context_slot_naming_the_core_twice_is_left_with_none() {
        // Deduplication is a consequence of the same rule: every embedded Core
        // reference goes, so the broker's single appended copy is the only one.
        let input = jsonld_syntax::Value::from_serde_json(json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "speed": "https://example.com/speed" }
            ]
        }));

        let normalized = without_embedded_core_context("https://example.com/twice.jsonld", input);

        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json(normalized),
            json!({ "@context": [{ "speed": "https://example.com/speed" }] })
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

        let normalized = without_embedded_core_context(core_url, input);

        pretty_assertions::assert_eq!(jsonld_syntax::Value::into_serde_json(normalized), payload);
    }
}
