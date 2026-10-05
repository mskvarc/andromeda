//! Whether a client-supplied `jsonldContext` is one this broker can use.
//!
//! A subscription and a context-source registration may each carry the
//! `@context` a notification will be rendered against (clauses 5.8.2, 5.9.2).
//! Storing one that turns out to be unreachable or malformed defers the
//! failure to notification time, where no client is listening; so it is
//! resolved once, at the write, by expanding the smallest document that
//! forces every context it names to load.

use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine, error::JsonLdError};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::{Value, json};
use std::str::FromStr;

/// Check that a `jsonldContext` value is a valid, reachable JSON-LD context.
///
/// A string is either an inline context written as JSON text or the URI of
/// one; an object or array is an inline context. Anything else names no
/// context at all.
///
/// # Errors
///
/// Returns [`JsonLdError::JsonLdContextText`] when a string that opens as
/// JSON does not parse, [`JsonLdError::JsonLdContextShape`] when the value is
/// not a context in any of those forms, and a context-loading variant when it is one that
/// does not resolve.
pub async fn validate_jsonld_context_value(engine: &JsonLdEngine, value: &Value) -> Result<(), JsonLdError> {
    let context = as_context_value(value)?;
    let probe = json!({
        "@context": context,
        "id": "urn:ngsi-ld:_:context-validate",
        "type": "Subscription"
    });
    engine.expand(probe, &ContextSpec::none()).await?;
    Ok(())
}

/// Check that every context selected for one request is valid and reachable.
///
/// This validates body and `Link` contexts together with the configured core
/// context policy, preserving context-loading failures as [`JsonLdError`]
/// values before a downstream term-language adapter can erase their kind.
///
/// # Errors
///
/// Returns the typed JSON-LD processing or context-loading failure produced
/// while resolving the effective request context. What clause 5.5.7 refuses
/// in a user `@context` is the admission's
/// ([`crate::user_context_admission`]), decided before this is reached.
pub async fn validate_context_spec(engine: &JsonLdEngine, context: &ContextSpec) -> Result<(), JsonLdError> {
    let probe = json!({
        "id": "urn:ngsi-ld:_:context-validate",
        "type": "Subscription"
    });
    engine.expand(probe, context).await?;
    Ok(())
}

/// Read a `jsonldContext` member as the context value it names.
fn as_context_value(value: &Value) -> Result<Value, JsonLdError> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                serde_json::from_str::<Value>(trimmed).map_err(JsonLdError::JsonLdContextText)
            } else if Identifier::from_str(trimmed).is_ok() {
                Ok(Value::String(trimmed.to_string()))
            } else {
                Err(JsonLdError::JsonLdContextShape)
            }
        }
        Value::Array(_) | Value::Object(_) => Ok(value.clone()), // clone: the caller's payload stays valid
        Value::Null | Value::Bool(_) | Value::Number(_) => Err(JsonLdError::JsonLdContextShape),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        context_validation::{validate_context_spec, validate_jsonld_context_value},
        engine::processor::JsonLdEngine,
        error::JsonLdError,
    };
    use iri_rs::IriBuf;
    use serde_json::json;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[tokio::test]
    async fn an_inline_context_object_is_accepted() {
        let validated = validate_jsonld_context_value(&offline_engine(), &json!({ "speed": "https://example.com/speed" })).await;
        assert!(validated.is_ok(), "an inline context needs nothing resolved: {:?}", validated.err());
    }

    #[tokio::test]
    async fn an_inline_context_written_as_json_text_is_accepted() {
        let validated = validate_jsonld_context_value(&offline_engine(), &json!(r#"{"speed":"https://example.com/speed"}"#)).await;
        assert!(validated.is_ok(), "JSON text is an inline context: {:?}", validated.err());
    }

    #[tokio::test]
    async fn a_url_that_does_not_resolve_is_reported_as_a_loading_failure() {
        let error = validate_jsonld_context_value(&offline_engine(), &json!("https://example.com/absent.jsonld"))
            .await
            .expect_err("nothing resolves offline");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }

    #[tokio::test]
    async fn a_value_that_names_no_context_is_refused_before_anything_is_resolved() {
        let engine = offline_engine();
        for rejected in [json!(null), json!(true), json!(42), json!("not a uri")] {
            let error = validate_jsonld_context_value(&engine, &rejected).await.expect_err("not a context");
            assert!(
                matches!(error, JsonLdError::JsonLdContextShape),
                "expected JsonLdContextShape for {rejected}, got {error}"
            );
        }
    }

    #[tokio::test]
    async fn a_string_that_opens_like_json_but_is_not_reports_the_parse_failure() {
        let error = validate_jsonld_context_value(&offline_engine(), &json!("{not-json"))
            .await
            .expect_err("not parseable");

        assert!(matches!(error, JsonLdError::JsonLdContextText(_)), "got {error}");
    }

    #[tokio::test]
    async fn an_unavailable_link_context_remains_a_typed_loading_failure() {
        let context = ContextSpec::from_link(IriBuf::new("https://example.com/absent.jsonld".to_owned()).expect("a valid IRI"));
        let error = validate_context_spec(&offline_engine(), &context).await.expect_err("nothing resolves offline");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }

    #[tokio::test]
    async fn an_unavailable_body_context_remains_a_typed_loading_failure() {
        let context = ContextSpec::from_body(json!("https://example.com/absent.jsonld"));
        let error = validate_context_spec(&offline_engine(), &context).await.expect_err("nothing resolves offline");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }
}
