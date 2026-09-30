//! Turning a [`ContextSpec`] into the `@context` a document carries.
//!
//! Clause 5.5.7 gives the core context absolute precedence: it is appended
//! *last* so that, under JSON-LD 1.1's rule of later entries overriding earlier
//! ones, its definitions shadow any user context that redefines a core term (a
//! user `@vocab`, a redeclared `observedAt`). A user context still contributes
//! its own new terms; it just cannot override the core's. Both functions here
//! implement that order: [`resolve_context_value`] builds the value,
//! [`apply_context`] writes it onto a document.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    core_context_precedence::{CoreImport, drop_core_import, is_core_context_json_string},
    error::JsonLdError,
};
use serde_json::{Map, Value};

/// Combine a document's own `@context` with a [`ContextSpec`].
///
/// Answers `None` when the combination is empty, which is the only case in
/// which a document ends up with no `@context` at all.
#[must_use]
pub fn resolve_context_value(document_context: Option<&Value>, spec: &ContextSpec) -> Option<Value> {
    // The document's own context first, then the one place the request named
    // its context (clause 6.3.5: a `Link` header or the body, never both).
    // Repeats are kept: JSON-LD processes an `@context` array in order, so
    // `[A, B, A]` leaves A's definitions in force where `[A, B]` leaves B's,
    // and clause 5.5.7 processes the context the call provided.
    let mut combined = extract_context_items(document_context);
    combined.extend(spec.request_context().entries());

    match spec.core_policy() {
        CoreContextPolicy::Only => return Some(Value::Array(vec![Value::String(spec.effective_core_url().to_string())])),
        CoreContextPolicy::Append => {
            retain_non_core(&mut combined);
            append_core(&mut combined, spec);
        }
    }

    // What a response names is `crate::presented_context`'s; this is the
    // context a document is processed under, which the processor takes in
    // either shape.
    if combined.is_empty() { None } else { Some(Value::Array(combined)) }
}

/// Write the resolved `@context` onto a document, or remove it when the spec
/// resolves to nothing.
///
/// # Errors
///
/// Returns [`JsonLdError::DocumentShape`] when the document is neither an
/// object nor an array of them.
pub fn apply_context(document: Value, spec: &ContextSpec) -> Result<Value, JsonLdError> {
    match document {
        Value::Object(object) => Ok(Value::Object(with_resolved_context(object, spec))),
        Value::Array(items) => Ok(Value::Array(
            items
                .into_iter()
                .map(|item| match item {
                    Value::Object(object) => Value::Object(with_resolved_context(object, spec)),
                    other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => other,
                })
                .collect(),
        )),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Err(JsonLdError::DocumentShape),
    }
}

/// Replace one object's `@context` with the resolution of its own and the
/// spec's.
fn with_resolved_context(mut object: Map<String, Value>, spec: &ContextSpec) -> Map<String, Value> {
    let document_context = object.get("@context").cloned(); // clone: the slot is overwritten below
    match resolve_context_value(document_context.as_ref(), spec) {
        Some(context) => {
            object.insert("@context".to_string(), context);
        }
        None => {
            object.remove("@context");
        }
    }
    object
}

/// Drop every core-context reference so the canonical one can be appended.
///
/// An inline definition's `@import` of a core context is such a reference too
/// (JSON-LD 1.1 section 4.1.10): left in, an imported v1.8 would come before
/// the appended v1.9 as a protected occurrence of differing definitions, which
/// the processor refuses as a `protected term redefinition`.
///
/// A definition that held nothing but the core import (and `@version` or
/// `@protected`) is removed with it, exactly as the presented context removes
/// it ([`crate::presented_context`]), so the two readings of one request agree.
fn retain_non_core(contexts: &mut Vec<Value>) {
    contexts.retain_mut(|context| match context {
        Value::String(reference) => !is_core_context_json_string(reference),
        Value::Object(definition) => !(drop_core_import(definition) == CoreImport::Dropped && definition.is_empty()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => true,
    });
}

/// Append the core context.
fn append_core(contexts: &mut Vec<Value>, spec: &ContextSpec) {
    contexts.push(Value::String(spec.effective_core_url().to_string()));
}

/// Flatten a `@context` slot into the list of contexts it names.
fn extract_context_items(context: Option<&Value>) -> Vec<Value> {
    match context {
        Some(Value::Array(items)) => items.clone(), // clone: the document is borrowed and stays valid
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![other.clone()], // clone: the document is borrowed and stays valid
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::{NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V18_URL},
        document_context::{apply_context, resolve_context_value},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::json;

    /// A context URL as the absolute IRI a `Link` header resolves to.
    fn iri_ref(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    #[test]
    fn the_document_context_comes_first_and_the_core_context_last() {
        let spec = ContextSpec::from_link(iri_ref("https://example.com/a"));
        let document_context = json!(["https://example.com/doc", "https://example.com/a"]);

        let resolved = resolve_context_value(Some(&document_context), &spec).expect("a context value");

        pretty_assertions::assert_eq!(
            resolved,
            json!([
                "https://example.com/doc",
                "https://example.com/a",
                "https://example.com/a",
                NGSI_LD_CORE_CONTEXT_URL
            ])
        );
    }

    /// JSON-LD 1.1 processes an `@context` array in order, so a repeated
    /// context is processed again and keeps the last word on the terms it
    /// defines (clause 5.5.7: the context the call provided).
    #[test]
    fn a_repeated_request_context_is_kept_in_place() {
        let spec = ContextSpec::from_body(json!(["https://example.com/a", "https://example.com/b", "https://example.com/a"]));

        let resolved = resolve_context_value(None, &spec).expect("a context value");

        pretty_assertions::assert_eq!(
            resolved,
            json!([
                "https://example.com/a",
                "https://example.com/b",
                "https://example.com/a",
                NGSI_LD_CORE_CONTEXT_URL
            ])
        );
    }

    #[test]
    fn a_document_with_no_context_of_its_own_gets_the_core_context() {
        let resolved = resolve_context_value(None, &ContextSpec::none()).expect("a context value");
        pretty_assertions::assert_eq!(resolved, json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_core_context_alias_in_the_document_is_replaced_by_the_canonical_one_last() {
        let document_context = json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_V16_URL]);

        let resolved = resolve_context_value(Some(&document_context), &ContextSpec::none()).expect("a context value");

        pretty_assertions::assert_eq!(resolved, json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_body_context_is_kept_and_the_core_context_still_comes_last() {
        let spec = ContextSpec::from_body(json!("https://example.com/compound.jsonld"));

        let resolved = resolve_context_value(None, &spec).expect("a context value");

        pretty_assertions::assert_eq!(resolved, json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// JSON-LD 1.1 section 4.1.10 and clause 5.5.7: a core context an inline
    /// definition imports is dropped like a referenced one, so the v1.9 core
    /// appended last is the only occurrence.
    #[test]
    fn a_core_context_an_inline_definition_imports_is_dropped_before_the_core_is_appended() {
        let spec = ContextSpec::from_body(json!({
            "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str(),
            "speed": "https://example.com/speed"
        }));

        let resolved = resolve_context_value(None, &spec).expect("a context value");

        pretty_assertions::assert_eq!(resolved, json!([{ "speed": "https://example.com/speed" }, NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// A definition that imported the core context and set nothing else but
    /// `@version` or `@protected` leaves nothing behind, as the presented
    /// context leaves nothing behind for it.
    #[test]
    fn a_definition_holding_only_a_core_import_and_version_leaves_nothing_behind() {
        let spec = ContextSpec::from_body(json!({ "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str(), "@version": 1.1 }));

        let resolved = resolve_context_value(None, &spec).expect("a context value");

        pretty_assertions::assert_eq!(resolved, json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn applying_a_context_writes_the_slot_on_an_object_and_on_every_array_member() {
        let spec = ContextSpec::none();

        let object = apply_context(json!({"id": "urn:test:1", "type": "Test"}), &spec).expect("an object document");
        assert!(object.get("@context").is_some());

        let array = apply_context(json!([{"id": "urn:test:1", "type": "Test"}]), &spec).expect("an array document");
        assert!(array[0].get("@context").is_some());
    }

    #[test]
    fn a_document_that_is_neither_an_object_nor_an_array_is_refused() {
        assert!(apply_context(json!("string"), &ContextSpec::none()).is_err());
    }
}
