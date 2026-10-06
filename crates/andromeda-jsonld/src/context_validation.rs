//! Whether a client-supplied `jsonldContext` is one this broker can use.
//!
//! A subscription and a context-source registration may each carry the
//! `@context` a notification will be rendered against (clauses 5.8.2, 5.9.2).
//! Storing one that turns out to be unreachable or malformed defers the
//! failure to notification time, where no client is listening; so it is
//! resolved and processed once, at the write, which loads every context it
//! names.

use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine, error::JsonLdError};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError, piece::JsonKind};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
/// not a context in any of those forms, and a context-loading variant when it
/// is one that does not resolve.
pub async fn validate_jsonld_context_value(engine: &JsonLdEngine, value: &JsonLiteral) -> Result<(), JsonLdError> {
    let context = as_context_value(value)?;
    validate_context_spec(engine, &ContextSpec::from_body(context)).await
}

/// Check that every context selected for one request is valid and reachable.
///
/// This validates body and `Link` contexts together with the configured core
/// context policy, preserving context-loading failures as [`JsonLdError`]
/// values before a downstream term-language adapter can erase their kind:
/// the context is resolved and processed exactly as a document written under
/// it would be. What clause 5.5.7 refuses in a user `@context` is the
/// admission's ([`crate::user_context_admission`]), decided before this is
/// reached.
///
/// # Errors
///
/// Returns the typed JSON-LD processing or context-loading failure produced
/// while resolving the effective request context.
pub async fn validate_context_spec(engine: &JsonLdEngine, context: &ContextSpec) -> Result<(), JsonLdError> {
    engine.expansion_context(context).await?;
    Ok(())
}

/// Read a `jsonldContext` member as the context value it names.
fn as_context_value(value: &JsonLiteral) -> Result<JsonLiteral, JsonLdError> {
    match value.piece().kind() {
        JsonKind::String => {
            let text = value.piece().as_str().ok_or(JsonLdError::JsonLdContextShape)?;
            let trimmed = text.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                JsonLiteral::admit(trimmed).map_err(|error| match error {
                    JsonLiteralError::Syntax { source } => JsonLdError::JsonLdContextText(source),
                    other @ (JsonLiteralError::DuplicateMember { .. }
                    | JsonLiteralError::Number { .. }
                    | JsonLiteralError::TooDeep { .. }
                    | JsonLiteralError::ProcessorText { .. }) => JsonLdError::ContextText(other),
                })
            } else if Identifier::from_str(trimmed).is_ok() {
                Ok(JsonLiteral::string(trimmed)?)
            } else {
                Err(JsonLdError::JsonLdContextShape)
            }
        }
        JsonKind::Array | JsonKind::Object => Ok(value.clone()), // clone: the caller's payload stays valid
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number => Err(JsonLdError::JsonLdContextShape),
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
    use andromeda_json_literal::JsonLiteral;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn literal(value: &serde_json::Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible value")
    }

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[tokio::test]
    async fn an_inline_context_object_is_accepted() {
        let validated = validate_jsonld_context_value(&offline_engine(), &literal(&json!({ "speed": "https://example.com/speed" }))).await;
        assert!(validated.is_ok(), "an inline context needs nothing resolved: {:?}", validated.err());
    }

    #[tokio::test]
    async fn an_inline_context_written_as_json_text_is_accepted() {
        let validated = validate_jsonld_context_value(&offline_engine(), &literal(&json!(r#"{"speed":"https://example.com/speed"}"#))).await;
        assert!(validated.is_ok(), "JSON text is an inline context: {:?}", validated.err());
    }

    #[tokio::test]
    async fn a_url_that_does_not_resolve_is_reported_as_a_loading_failure() {
        let error = validate_jsonld_context_value(&offline_engine(), &literal(&json!("https://example.com/absent.jsonld")))
            .await
            .expect_err("nothing resolves offline");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }

    #[tokio::test]
    async fn a_value_that_names_no_context_is_refused_before_anything_is_resolved() {
        let engine = offline_engine();
        for rejected in [json!(null), json!(true), json!(42), json!("not a uri")] {
            let error = validate_jsonld_context_value(&engine, &literal(&rejected)).await.expect_err("not a context");
            assert!(
                matches!(error, JsonLdError::JsonLdContextShape),
                "expected JsonLdContextShape for {rejected}, got {error}"
            );
        }
    }

    #[tokio::test]
    async fn a_string_that_opens_like_json_but_is_not_reports_the_parse_failure() {
        let error = validate_jsonld_context_value(&offline_engine(), &literal(&json!("{not-json")))
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
        let context = ContextSpec::from_body(literal(&json!("https://example.com/absent.jsonld")));
        let error = validate_context_spec(&offline_engine(), &context).await.expect_err("nothing resolves offline");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }
}
