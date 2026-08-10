//! JSON-LD normalization of Context Source properties used by `csf`.

use crate::{error::RegistrationError, payload::extract_additional_properties};
use andromeda_jsonld::{compaction::compact_attr_iri, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use serde_json::{Map, Value};

/// Expand the non-standard Context Source properties from a registration payload.
///
/// A `csf` expression is expanded against the request context before evaluation,
/// so its target registration properties must use the same canonical IRI keys and
/// stored key. Context Source Property values retain their registration wire
/// shape because CSR query pushdown unwraps that shape directly.
pub(super) async fn expand_context_source_properties(
    engine: &JsonLdEngine,
    payload: &Value,
    context: &ContextSpec,
) -> Result<Option<Value>, RegistrationError> {
    let Some(Value::Object(properties)) = extract_additional_properties(payload) else {
        return Ok(None);
    };
    let prepared_context = engine.prepare_term_context(context);
    let mut expanded = Map::new();
    for (key, value) in properties {
        let key = engine.expand_term(&key, &prepared_context).await?.to_string();
        expanded.insert(key, value);
    }
    Ok((!expanded.is_empty()).then_some(Value::Object(expanded)))
}

/// Compact stored Context Source properties for a registration response.
pub(super) async fn compact_context_source_properties(
    engine: &JsonLdEngine,
    properties: Value,
    context: &ContextSpec,
) -> Result<Option<Value>, RegistrationError> {
    let Value::Object(properties) = properties else {
        return Ok(None);
    };
    let mut compacted = Map::new();
    for (key, value) in properties {
        compacted.insert(compact_attr_iri(engine, &key, context).await, value);
    }
    Ok((!compacted.is_empty()).then_some(Value::Object(compacted)))
}
