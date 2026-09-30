//! Resolving the relative context references of a downloaded document against
//! the URL it was finally read from.
//!
//! JSON-LD 1.1 API section 4.1.2 step 5.2.6 processes a remote context with
//! "the documentUrl of context document for base URL", the documentUrl being
//! "the final URL of the loaded document" after redirects and a followed
//! alternate. The processor Andromeda runs on passes the URL it asked for
//! instead, so a context reached through a redirect would resolve its relative
//! `@context` and `@import` references against the wrong directory. The loader
//! therefore resolves them itself, against the document's own URL, before the
//! processor reads the document; an absolute reference left behind resolves to
//! itself whatever base the processor passes.
//!
//! Only the members context processing resolves against the base URL are
//! touched: a string in a `@context` slot (steps 5.2.1 and, through a scoped
//! context, Create Term Definition step 21) and an `@import` value (step
//! 5.6.3). `@vocab` resolves against the active context's base IRI, not the
//! context's URL, and is left as written.

use crate::core_context_precedence::IMPORT_KEYWORD;
use iri_rs::{IriBuf, IriRef};
use serde_json::{Map, Value};

/// The keyword a context slot is named by, at the top of a context document and
/// inside a term definition.
const CONTEXT_KEYWORD: &str = "@context";

/// Resolve every relative context reference in `document` against
/// `document_url`, the URL it was finally read from.
///
/// A reference that is not a valid IRI reference, or does not resolve, is left
/// as written: the processor then reports it exactly as it would have.
pub(crate) fn resolve_relative_context_references(document: &mut Value, document_url: &IriBuf) {
    if let Value::Object(members) = document
        && let Some(slot) = members.get_mut(CONTEXT_KEYWORD)
    {
        resolve_slot(slot, document_url);
    }
}

/// Resolve the references of one `@context` slot: a reference, a definition, or
/// an array of both.
fn resolve_slot(slot: &mut Value, document_url: &IriBuf) {
    match slot {
        Value::String(reference) => resolve_reference(reference, document_url),
        Value::Object(definition) => resolve_definition(definition, document_url),
        Value::Array(entries) => entries.iter_mut().for_each(|entry| resolve_slot(entry, document_url)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Resolve a context definition's `@import` and the scoped contexts of its term
/// definitions.
fn resolve_definition(definition: &mut Map<String, Value>, document_url: &IriBuf) {
    for (key, value) in definition.iter_mut() {
        match value {
            Value::String(import) if key == IMPORT_KEYWORD => resolve_reference(import, document_url),
            Value::Object(term_definition) => {
                if let Some(scoped) = term_definition.get_mut(CONTEXT_KEYWORD) {
                    resolve_slot(scoped, document_url);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {}
        }
    }
}

/// Replace a relative reference with its resolution against `document_url`.
fn resolve_reference(reference: &mut String, document_url: &IriBuf) {
    let Ok(parsed) = IriRef::parse(reference.as_str()) else {
        return;
    };
    if parsed.is_absolute() {
        return;
    }
    let mut resolved = String::with_capacity(document_url.as_str().len() + reference.len());
    if parsed.resolve_into(document_url, &mut resolved).is_ok() {
        *reference = resolved;
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::relative_context_references::resolve_relative_context_references;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    /// Resolve `document` as if it were finally read from
    /// `https://origin.example.com/contexts/v2/wrapper.jsonld`.
    fn resolved(mut document: Value) -> Value {
        let document_url = IriBuf::new("https://origin.example.com/contexts/v2/wrapper.jsonld".to_owned()).expect("a valid IRI");
        resolve_relative_context_references(&mut document, &document_url);
        document
    }

    /// JSON-LD 1.1 API 4.1.2 step 5.2.6: a document's own `@context` reference
    /// resolves against the URL the document was read from.
    #[test]
    fn a_relative_context_reference_resolves_against_the_document_url() {
        pretty_assertions::assert_eq!(
            resolved(json!({ "@context": "terms.jsonld" })),
            json!({ "@context": "https://origin.example.com/contexts/v2/terms.jsonld" })
        );
    }

    /// Every entry of a context array is resolved, an `@import` (step 5.6.3)
    /// included, and an absolute reference is left as written.
    #[test]
    fn every_relative_entry_of_a_context_array_and_an_import_resolve() {
        pretty_assertions::assert_eq!(
            resolved(json!({ "@context": [
                "../shared/base.jsonld",
                "https://other.example.org/absolute.jsonld",
                { "@import": "/root.jsonld", "speed": "https://example.org/speed" }
            ] })),
            json!({ "@context": [
                "https://origin.example.com/contexts/shared/base.jsonld",
                "https://other.example.org/absolute.jsonld",
                { "@import": "https://origin.example.com/root.jsonld", "speed": "https://example.org/speed" }
            ] })
        );
    }

    /// A scoped context is processed with the base URL of the definition that
    /// carries it (Create Term Definition step 21), at any depth.
    #[test]
    fn a_scoped_context_resolves_at_any_depth() {
        pretty_assertions::assert_eq!(
            resolved(json!({ "@context": {
                "Vehicle": { "@id": "https://example.org/Vehicle", "@context": ["vehicle.jsonld", {
                    "engine": { "@id": "https://example.org/engine", "@context": "engine/terms.jsonld" }
                }] }
            } })),
            json!({ "@context": {
                "Vehicle": { "@id": "https://example.org/Vehicle", "@context": ["https://origin.example.com/contexts/v2/vehicle.jsonld", {
                    "engine": { "@id": "https://example.org/engine", "@context": "https://origin.example.com/contexts/v2/engine/terms.jsonld" }
                }] }
            } })
        );
    }

    /// Only context references are touched: `@vocab`, a term's `@id`, a
    /// compact IRI and members outside the `@context` slot are left as written.
    #[test]
    fn members_that_are_not_context_references_are_left_as_written() {
        let document = json!({
            "@context": { "@vocab": "vocab/", "speed": "ex:speed", "brand": { "@id": "relative/brand" }, "ex": "https://example.org/" },
            "@import": "outside.jsonld"
        });

        pretty_assertions::assert_eq!(resolved(document.clone()), document); // clone: the expectation is the input unchanged
    }
}
