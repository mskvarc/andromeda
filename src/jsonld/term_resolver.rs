//! Compaction of expanded IRIs back to context terms, and validation of a
//! client-supplied `@context`.
//!
//! Expansion of a parsed `q` filter is not here: those walkers are shaped by
//! the filter AST, so they live with it in
//! [`andromeda_ngsi_ld_query::filter::expansion`], reached through the
//! [`TermExpander`](andromeda_ngsi_ld_query::term_expander::TermExpander) trait.

use crate::{
    core::error::AndromedaError,
    jsonld::{CompactOptions, ContextSpec, ContextStripping, JsonLdEngine},
};
use serde_json::Value;

/// Compact a type IRI back to a short term using the given context.
///
/// Returns the original IRI as-is if compaction fails or the IRI is not
/// an HTTP/URN type.
pub async fn compact_type_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !iri.starts_with("http") && !iri.starts_with("urn:") {
        return iri.to_string();
    }
    let doc = serde_json::json!({
        "@id": "urn:ngsi-ld:_:dummy",
        "@type": [iri]
    });
    match engine
        .compact(
            doc,
            context,
            CompactOptions {
                strip_context: ContextStripping::Strip,
            },
        )
        .await
    {
        Ok(compacted) => compacted.get("type").and_then(|t| t.as_str()).map_or_else(|| iri.to_string(), String::from),
        Err(e) => {
            tracing::debug!(error = %e, iri = iri, "compact_type_iri fallback to raw IRI");
            iri.to_string()
        }
    }
}

/// Compact an attribute IRI back to a short term using the given context.
///
/// Returns the original IRI as-is if compaction fails or the IRI is not
/// an HTTP/URN type.
pub async fn compact_attr_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !iri.starts_with("http") && !iri.starts_with("urn:") {
        return iri.to_string();
    }
    let doc = serde_json::json!([{
        "@id": "urn:ngsi-ld:_:dummy",
        iri: [{"@value": "dummy"}]
    }]);
    match engine
        .compact(
            doc,
            context,
            CompactOptions {
                strip_context: ContextStripping::Strip,
            },
        )
        .await
    {
        Ok(compacted) => {
            if let Some(obj) = compacted.as_object() {
                for key in obj.keys() {
                    if key != "id" && key != "type" && !key.starts_with('@') {
                        return key.clone();
                    }
                }
            }
            iri.to_string()
        }
        Err(e) => {
            tracing::debug!(error = %e, iri = iri, "compact_attr_iri fallback to raw IRI");
            iri.to_string()
        }
    }
}

/// Validate that a jsonldContext value is a reachable, valid JSON-LD context.
///
/// Attempts to expand a minimal document using the given context. If
/// expansion succeeds the context is considered valid and reachable.
///
/// # Errors
///
/// Returns an error if the context is not a valid URI / JSON-LD context,
/// or if expansion fails (unreachable remote context, parse error, etc.).
pub async fn validate_jsonld_context_value(engine: &JsonLdEngine, value: &Value) -> Result<(), AndromedaError> {
    let ctx_value = match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                serde_json::from_str::<Value>(trimmed).map_err(|e| AndromedaError::jsonld_processing(format!("Invalid jsonldContext JSON: {}", e)))?
            } else {
                if !is_valid_uri(trimmed) {
                    return Err(AndromedaError::jsonld_processing("jsonldContext must be a valid URI or JSON-LD context"));
                }
                Value::String(trimmed.to_string())
            }
        }
        Value::Array(_) | Value::Object(_) => value.clone(),
        Value::Null | Value::Bool(_) | Value::Number(_) => {
            return Err(AndromedaError::jsonld_processing("jsonldContext must be a valid URI or JSON-LD context"));
        }
    };

    let doc = serde_json::json!({
        "@context": ctx_value,
        "id": "urn:ngsi-ld:_:context-validate",
        "type": "Subscription"
    });

    engine.expand(doc, &ContextSpec::none()).await?;
    Ok(())
}

/// Check if a string is a valid URI (URN or URL).
pub fn is_valid_uri(id: &str) -> bool {
    if id.starts_with("urn:") {
        return id.parse::<urn_rs::Urn>().is_ok();
    }
    url::Url::parse(id).is_ok()
}
