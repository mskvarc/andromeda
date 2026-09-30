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
//!
//! What the removal takes away is also what a response needs to know about the
//! document: whether it leaves the Core in force last (clause 4.4, Annex C.8).
//! That fact is read from the document as written, in the same pass, and kept
//! beside the processed copy as its [`ContextTail`].

use crate::{
    context_tail::{ContextTail, IMPORT_ONLY_KEYWORDS},
    core_context::is_ngsi_ld_core_context_url,
};
use iri_rs::{Iri, IriBuf};
use serde_json::{Map, Value};

/// The keyword JSON-LD 1.1 section 4.1.10 imports a context through.
pub(crate) const IMPORT_KEYWORD: &str = "@import";

/// A loaded context document, ready for the JSON-LD processor, beside what the
/// document as written leaves in force last.
#[derive(Debug)]
pub(crate) struct PreparedContext {
    /// The document with every embedded Core @context reference removed.
    pub(crate) value: jsonld_syntax::Value,
    /// What the document as written leaves in force last.
    pub(crate) tail: ContextTail,
}

/// Prepare a loaded context document for processing: remove every embedded
/// Core @context reference, so the authoritative Core the broker appends last
/// stays the single occurrence, and record what the document as written left
/// in force last.
///
/// The Core @context document itself is returned untouched — that one *is* the
/// definition, not a redundant import of it.
pub(crate) fn prepared_context(url: &IriBuf, mut json: Value) -> PreparedContext {
    let tail = ContextTail::of_document(&json);
    if !is_ngsi_ld_core_context_url(url.as_ref()) {
        remove_embedded_core_references(&mut json);
    }
    PreparedContext {
        value: jsonld_syntax::Value::from_serde_json(json),
        tail,
    }
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
        Value::String(url) if is_core_context_json_string(url) => {
            *context = Value::Array(Vec::new());
        }
        Value::Array(items) => {
            items.retain(|item| !item.as_str().is_some_and(is_core_context_json_string));
            for item in items {
                if let Value::Object(definition) = item {
                    drop_core_import(definition);
                }
                remove_embedded_core_references(item);
            }
        }
        Value::Object(definition) => {
            drop_core_import(definition);
            remove_embedded_core_references(context);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Whether a context definition imported the Core @context before
/// [`drop_core_import`] saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreImport {
    /// The definition imported a Core @context, and the import was dropped.
    Dropped,
    /// The definition imported no Core @context.
    Absent,
}

/// Drop an `@import` of a Core @context from one context definition.
///
/// JSON-LD 1.1 section 4.1.10 merges an imported context into the definition
/// that imports it, so an imported Core is an earlier, protected occurrence of
/// it exactly as a referenced one is: with the authoritative Core appended last,
/// an imported v1.8 would be a `protected term redefinition`. Clause 5.5.7 gives
/// the Core absolute precedence however a user context names it, so the import
/// goes like every other embedded reference.
///
/// A definition that imported the Core and set nothing else but `@version` and
/// `@protected` defined nothing of its own, so it is emptied altogether: left
/// as `{"@version": 1.1}` it would stand as a context of its own after the
/// import is gone, and a response would have to host a wrapper for a
/// definition that says nothing.
pub(crate) fn drop_core_import(definition: &mut Map<String, Value>) -> CoreImport {
    if definition.get(IMPORT_KEYWORD).and_then(Value::as_str).is_some_and(is_core_context_json_string) {
        definition.remove(IMPORT_KEYWORD);
        if definition.keys().all(|key| IMPORT_ONLY_KEYWORDS.contains(&key.as_str())) {
            definition.clear();
        }
        CoreImport::Dropped
    } else {
        CoreImport::Absent
    }
}

/// Whether a JSON string contains a validated NGSI-LD Core Context IRI.
pub(crate) fn is_core_context_json_string(value: &str) -> bool {
    Iri::parse(value).is_ok_and(is_ngsi_ld_core_context_url)
}

#[cfg(test)]
mod tests {
    use crate::{
        context_tail::ContextTail,
        core_context::{NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V18_URL},
        core_context_precedence::{drop_core_import, prepared_context},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::{IriBuf, iri};
    use serde_json::{Value, json};

    /// Prepare `document` as if it were loaded from `url`.
    fn prepare(url: &str, document: &Value) -> (Value, ContextTail) {
        let url = IriBuf::new(url.to_owned()).expect("a valid URL");
        let prepared = prepared_context(&url, document.clone()); // clone: the caller keeps the original to compare
        (jsonld_syntax::Value::into_serde_json(prepared.value), prepared.tail)
    }

    #[test]
    fn a_user_context_importing_the_core_loses_only_that_redundant_import() {
        // Clause 5.5.7: the broker appends the authoritative Core last, so a
        // user context's own embedded Core reference is redundant and is
        // dropped, while its genuine term definitions are kept untouched.
        let (normalized, tail) = prepare(
            "https://example.com/compound.jsonld",
            &json!({ "@context": [NGSI_LD_CORE_CONTEXT_URL.as_str(), { "location": "https://example.com/location" }] }),
        );

        pretty_assertions::assert_eq!(normalized, json!({ "@context": [{ "location": "https://example.com/location" }] }));
        pretty_assertions::assert_eq!(tail, ContextTail::LocalDefinitions);
    }

    #[test]
    fn a_context_slot_naming_the_core_twice_is_left_with_none() {
        // Deduplication is a consequence of the same rule: every embedded Core
        // reference goes, so the broker's single appended copy is the only one.
        let (normalized, _) = prepare(
            "https://example.com/twice.jsonld",
            &json!({ "@context": [NGSI_LD_CORE_CONTEXT_URL.as_str(), NGSI_LD_CORE_CONTEXT_URL.as_str(), { "speed": "https://example.com/speed" }] }),
        );

        pretty_assertions::assert_eq!(normalized, json!({ "@context": [{ "speed": "https://example.com/speed" }] }));
    }

    /// Clause 4.4 and Annex C.8: what the document leaves in force last is read
    /// from the document as written, before its Core reference is dropped for
    /// processing.
    #[test]
    fn a_document_ending_with_the_core_keeps_that_fact_although_the_reference_is_dropped() {
        let (normalized, tail) = prepare(
            "https://example.com/compound.jsonld",
            &json!({ "@context": ["https://example.com/terms.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()] }),
        );

        pretty_assertions::assert_eq!(normalized, json!({ "@context": ["https://example.com/terms.jsonld"] }));
        pretty_assertions::assert_eq!(tail, ContextTail::CurrentCore);
    }

    /// JSON-LD 1.1 section 4.1.10 and clause 5.5.7: an imported Core is an
    /// earlier, protected occurrence exactly as a referenced one is, so the
    /// import is dropped in every release.
    #[test]
    fn an_imported_core_of_any_release_is_dropped() {
        for core in [NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_URL] {
            let (normalized, _) = prepare(
                "https://example.com/importing.jsonld",
                &json!({ "@context": { "@import": core.as_str(), "speed": "https://example.com/speed" } }),
            );

            pretty_assertions::assert_eq!(normalized, json!({ "@context": { "speed": "https://example.com/speed" } }), "{core}");
        }
        let (in_array, _) = prepare(
            "https://example.com/importing.jsonld",
            &json!({ "@context": ["https://example.com/a.jsonld", { "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str() }] }),
        );
        pretty_assertions::assert_eq!(in_array, json!({ "@context": ["https://example.com/a.jsonld", {}] }));
    }

    /// A definition that imported the Core and set nothing else but
    /// `@version` or `@protected` defined nothing of its own, so nothing of it
    /// remains once the import is dropped.
    #[test]
    fn a_definition_left_with_only_version_or_protected_after_its_core_import_is_emptied() {
        for remainder in [
            json!({ "@version": 1.1 }),
            json!({ "@protected": true }),
            json!({ "@version": 1.1, "@protected": true }),
        ] {
            let mut definition = remainder.as_object().expect("an object").clone(); // clone: the fixture keeps its remainder for the message
            definition.insert("@import".to_owned(), json!(NGSI_LD_CORE_CONTEXT_V18_URL.as_str()));

            drop_core_import(&mut definition);

            assert!(definition.is_empty(), "{remainder} should leave nothing behind");
        }
        let mut with_a_term = json!({ "@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "@version": 1.1, "speed": "https://example.com/speed" })
            .as_object()
            .expect("an object")
            .clone(); // clone: the definition is edited in place
        drop_core_import(&mut with_a_term);
        pretty_assertions::assert_eq!(Value::Object(with_a_term), json!({ "@version": 1.1, "speed": "https://example.com/speed" }));
    }

    #[test]
    fn an_import_of_a_user_context_is_kept() {
        let document = json!({ "@context": { "@import": "https://example.com/base.jsonld", "speed": "https://example.com/speed" } });
        let (normalized, _) = prepare("https://example.com/importing.jsonld", &document);

        pretty_assertions::assert_eq!(normalized, document);
    }

    #[test]
    fn the_core_context_document_itself_is_left_alone() {
        let core_url = IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"));
        let payload = json!({ "@context": [core_url.as_str(), { "location": "https://example.com/location" }] });

        let (normalized, _) = prepare(core_url.as_str(), &payload);

        pretty_assertions::assert_eq!(normalized, payload);
    }
}
