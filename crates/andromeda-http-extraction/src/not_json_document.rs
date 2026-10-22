//! The refusal of a request body that is not a JSON document.

use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use thiserror::Error;

/// A request body whose text is not a JSON document.
///
/// Clause 5.5.4 raises `InvalidRequest` for a payload body that "is not a valid
/// JSON document". Every reader of a request body states this refusal in these
/// words, whichever model the body was to decode into.
#[derive(Debug, Error)]
#[error("request body is not a JSON document: {0}")]
pub struct NotJsonDocument(#[source] pub serde_json::Error);

impl RequestRefusal for NotJsonDocument {
    fn error_type(&self) -> NgsiLdErrorType {
        NgsiLdErrorType::InvalidRequest
    }
}

#[cfg(test)]
mod tests {
    use crate::not_json_document::NotJsonDocument;
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::http::{StatusCode, Uri};

    fn refusal() -> NotJsonDocument {
        NotJsonDocument(serde_json::from_str::<serde_json::Value>("{").expect_err("not a JSON document"))
    }

    #[test]
    fn the_detail_names_the_document_and_carries_the_parser_error() {
        pretty_assertions::assert_eq!(
            refusal().to_string(),
            "request body is not a JSON document: EOF while parsing an object at line 1 column 1"
        );
    }

    #[test]
    fn the_refusal_is_an_invalid_request_naming_the_request() {
        let problem = refusal().into_problem(Uri::from_static("/ngsi-ld/v1/entities"));
        pretty_assertions::assert_eq!(problem.problem().error_type(), NgsiLdErrorType::InvalidRequest);
        pretty_assertions::assert_eq!(problem.problem().status_code(), StatusCode::BAD_REQUEST);
        pretty_assertions::assert_eq!(problem.problem().instance().map(ToString::to_string).as_deref(), Some("/ngsi-ld/v1/entities"));
    }
}
