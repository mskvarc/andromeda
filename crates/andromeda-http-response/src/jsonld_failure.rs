//! Classifying a JSON-LD processing failure.

use crate::classified_report::ClassifiedReport;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// Classify JSON-LD context availability separately from malformed input.
#[must_use]
pub fn classify_jsonld_error(error: &JsonLdError) -> ClassifiedReport {
    match error {
        JsonLdError::ContextFetchDisabled { .. } | JsonLdError::ContextLoad { .. } | JsonLdError::ContextHttp { .. } | JsonLdError::ContextTooLarge { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::LdContextNotAvailable, error.to_string())
        }
        JsonLdError::Json(source) if source.is_syntax() || source.is_eof() => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        // Clause 5.5.7: parts of a user @context that contain a JSON-LD Scoped
        // Context, or that stand below the top level of the document, "should
        // result in an error of type BadRequestData".
        JsonLdError::ProtectedContextPreload { .. }
        | JsonLdError::ContextInvalid { .. }
        | JsonLdError::PreloadedContextUnreadable { .. }
        | JsonLdError::PreloadedContextInvalid { .. }
        | JsonLdError::PreloadedContextUrl(_)
        | JsonLdError::EmbeddedContextMissing { .. }
        | JsonLdError::EmbeddedContextInvalid { .. }
        | JsonLdError::InlineContextInvalid(_)
        | JsonLdError::ScopedUserContext { .. }
        | JsonLdError::EmbeddedUserContext(_)
        | JsonLdError::ClientBuild(_)
        | JsonLdError::Expansion(_)
        | JsonLdError::Compaction(_)
        | JsonLdError::TypeNotExpanded
        | JsonLdError::TermNotExpanded
        | JsonLdError::ExpandedTermNotIri(_)
        | JsonLdError::ContextUrlNotIri(_)
        | JsonLdError::JsonLdContextText(_)
        | JsonLdError::JsonLdContextShape
        | JsonLdError::ContextText(_)
        | JsonLdError::ProcessorText(_)
        | JsonLdError::ContextGraph(_)
        | JsonLdError::Json(_) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_failure::classify_jsonld_error;
    use andromeda_jsonld::{context_graph::ContextGraphError, error::JsonLdError};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use iri_rs::IriBuf;

    #[test]
    fn malformed_json_is_invalid_request() {
        let error = JsonLdError::Json(serde_json::from_str::<serde_json::Value>("{").expect_err("malformed JSON"));
        pretty_assertions::assert_eq!(classify_jsonld_error(&error).error_type(), NgsiLdErrorType::InvalidRequest);
    }

    #[test]
    fn valid_json_with_a_deserialization_error_is_bad_request_data() {
        let error = JsonLdError::Json(serde_json::from_value::<u64>(serde_json::json!("not a number")).expect_err("wrong JSON type"));
        pretty_assertions::assert_eq!(classify_jsonld_error(&error).error_type(), NgsiLdErrorType::BadRequestData);
    }

    /// Clause 5.5.6: a `@context` whose references cycle, run too deep or
    /// overflow the processor is invalid, a `BadRequestData`.
    #[test]
    fn an_invalid_context_reference_graph_is_bad_request_data() {
        let url = IriBuf::new("https://example.org/cycle.jsonld".to_owned()).expect("a valid IRI");
        for refusal in [
            ContextGraphError::Cycle(url.clone()),
            ContextGraphError::TooDeep(url),
            ContextGraphError::ProcessorOverflow,
        ] {
            let error = JsonLdError::ContextGraph(refusal);
            pretty_assertions::assert_eq!(classify_jsonld_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error}");
        }
    }
}
