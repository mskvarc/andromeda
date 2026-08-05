//! Turning a [`ContextSpec`] into the `@context` a document carries.
//!
//! Clause 5.5.7 makes the request's own contexts win over the broker's, and
//! puts the core context last so nothing it defines shadows a user term.
//! Both functions here implement that order: [`resolve_context_value`] builds
//! the value, [`apply_context`] writes it onto a document.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    core_context::{ANDROMEDA_CORE_CONTEXT_URL, is_ngsi_ld_core_context_url},
    error::JsonLdError,
};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Combine a document's own `@context` with a [`ContextSpec`].
///
/// Answers `None` when the combination is empty, which is the only case in
/// which a document ends up with no `@context` at all.
pub fn resolve_context_value(document_context: Option<&Value>, spec: &ContextSpec) -> Option<Value> {
    let mut combined = extract_context_items(document_context);
    if let Some(body_context) = spec.body_context() {
        combined.extend(extract_context_items(Some(body_context)));
    }
    // Document context first, then the Link header's: clause 5.5.7 gives the
    // header the last word among the request's own contexts.
    combined.extend(spec.link_contexts().iter().cloned().map(Value::String)); // clone: the spec is borrowed and stays valid
    let mut combined = dedupe_contexts(combined);

    match spec.core_policy() {
        CoreContextPolicy::Only => return Some(Value::String(spec.effective_core_url().to_string())),
        CoreContextPolicy::Omit => {}
        CoreContextPolicy::Append => {
            retain_non_core(&mut combined);
            append_core(&mut combined, spec);
        }
        CoreContextPolicy::AppendIfNoExplicit => {
            retain_non_core(&mut combined);
            if combined.is_empty() {
                append_core(&mut combined, spec);
            }
        }
    }

    match combined.len() {
        0 => None,
        1 => combined.pop(),
        _more => Some(Value::Array(combined)),
    }
}

/// Write the resolved `@context` onto a document, or remove it when the spec
/// resolves to nothing.
///
/// # Errors
///
/// Returns [`JsonLdError::InvalidDocument`] when the document is neither an
/// object nor an array of them.
pub fn apply_context(document: Value, spec: &ContextSpec) -> Result<Value, JsonLdError> {
    match document {
        Value::Object(object) => Ok(Value::Object(with_resolved_context(object, spec))),
        Value::Array(items) => Ok(Value::Array(
            items
                .into_iter()
                .map(|item| match item {
                    Value::Object(object) => Value::Object(with_resolved_context(object, spec)),
                    other => other,
                })
                .collect(),
        )),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Err(JsonLdError::invalid_document("Document must be an object or array")),
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

/// Drop every core-context reference so the canonical pair can be appended.
fn retain_non_core(contexts: &mut Vec<Value>) {
    contexts.retain(|value| {
        !value
            .as_str()
            .is_some_and(|url| is_ngsi_ld_core_context_url(url) || url == ANDROMEDA_CORE_CONTEXT_URL)
    });
}

/// Append Andromeda's context and then the core one, in that order.
fn append_core(contexts: &mut Vec<Value>, spec: &ContextSpec) {
    contexts.push(Value::String(ANDROMEDA_CORE_CONTEXT_URL.to_string()));
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

/// Drop repeated context URLs, keeping the first occurrence.
///
/// Only strings are compared: two inline context objects that happen to be
/// equal are still two definitions, and JSON-LD applies them in order.
fn dedupe_contexts(contexts: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut kept = Vec::with_capacity(contexts.len());
    for value in contexts {
        match value {
            Value::String(ref url) => {
                if seen.insert(url.clone()) {
                    // clone: the set keeps a copy, the list keeps the value
                    kept.push(value);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => kept.push(value),
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::{ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
        document_context::{apply_context, resolve_context_value},
    };
    use serde_json::json;

    #[test]
    fn the_document_context_comes_first_and_the_core_contexts_last() {
        let spec = ContextSpec::from_links(vec!["https://example.com/a".to_string()]);
        let document_context = json!(["https://example.com/doc", "https://example.com/a"]);

        let resolved = resolve_context_value(Some(&document_context), &spec).expect("a context value");

        pretty_assertions::assert_eq!(
            resolved,
            json!([
                "https://example.com/doc",
                "https://example.com/a",
                ANDROMEDA_CORE_CONTEXT_URL,
                NGSI_LD_CORE_CONTEXT_URL
            ])
        );
    }

    #[test]
    fn a_document_with_no_context_of_its_own_gets_the_core_contexts() {
        let resolved = resolve_context_value(None, &ContextSpec::none()).expect("a context value");
        pretty_assertions::assert_eq!(resolved, json!([ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_core_context_alias_in_the_document_is_replaced_by_the_canonical_one_last() {
        let document_context = json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_V16_URL]);

        let resolved = resolve_context_value(Some(&document_context), &ContextSpec::none()).expect("a context value");

        pretty_assertions::assert_eq!(
            resolved,
            json!(["https://example.com/compound.jsonld", ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL])
        );
    }

    #[test]
    fn a_body_context_is_kept_and_the_core_contexts_still_come_last() {
        let spec = ContextSpec::none().with_body_context(Some(json!("https://example.com/compound.jsonld")));

        let resolved = resolve_context_value(None, &spec).expect("a context value");

        pretty_assertions::assert_eq!(
            resolved,
            json!(["https://example.com/compound.jsonld", ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL])
        );
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
