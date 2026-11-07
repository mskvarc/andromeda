//! The refusal of a `@context` creation body that is not admissible JSON.

use andromeda_http_extraction::not_json_document::NotJsonDocument;
use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use thiserror::Error;

/// Why a `@context` creation body is refused before it is stored.
#[derive(Debug, Error)]
pub(crate) enum InadmissibleBody {
    /// Clause 5.5.4: the body is not a JSON document.
    #[error(transparent)]
    NotJson(#[from] NotJsonDocument),
    /// The JSON literal admission refused the document.
    #[error("request body is not admissible JSON: {0}")]
    Refused(#[source] JsonLiteralError),
}

impl InadmissibleBody {
    /// The refusal of a body the JSON literal admission refused, a text that is
    /// no JSON being worded as every other body reader words it.
    pub(crate) fn of(error: JsonLiteralError) -> InadmissibleBody {
        match error {
            JsonLiteralError::Syntax { source } => InadmissibleBody::NotJson(NotJsonDocument(source)),
            refused @ (JsonLiteralError::DuplicateMember { .. }
            | JsonLiteralError::Number { .. }
            | JsonLiteralError::TooDeep { .. }
            | JsonLiteralError::ProcessorText { .. }) => InadmissibleBody::Refused(refused),
        }
    }
}

impl RequestRefusal for InadmissibleBody {
    /// Clause 5.5.4: an object naming one member twice is well-formed JSON
    /// whose data is not syntactically correct (IETF RFC 8259 section 4), the
    /// same refusal every other body draws.
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            InadmissibleBody::NotJson(refusal) => refusal.error_type(),
            InadmissibleBody::Refused(JsonLiteralError::DuplicateMember { .. }) => NgsiLdErrorType::BadRequestData,
            InadmissibleBody::Refused(
                JsonLiteralError::Syntax { .. } | JsonLiteralError::Number { .. } | JsonLiteralError::TooDeep { .. } | JsonLiteralError::ProcessorText { .. },
            ) => NgsiLdErrorType::InvalidRequest,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inadmissible_body::InadmissibleBody;
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

    /// The refusal the literal admission draws for `text`.
    fn refusal_of(text: &str) -> InadmissibleBody {
        InadmissibleBody::of(JsonLiteral::admit(text).expect_err("a refused text"))
    }

    /// A text that is no JSON is `InvalidRequest`, worded as every other body
    /// reader words it.
    #[test]
    fn a_text_that_is_no_json_is_an_invalid_request_in_the_shared_words() {
        let refusal = refusal_of("{");
        pretty_assertions::assert_eq!(refusal.error_type(), NgsiLdErrorType::InvalidRequest);
        assert!(refusal.to_string().starts_with("request body is not a JSON document:"), "{refusal}");
    }

    /// Clause 5.5.4: an object naming one member twice is `BadRequestData`.
    #[test]
    fn a_repeated_member_is_bad_request_data() {
        let refusal = refusal_of(r#"{"a":1,"a":2}"#);
        pretty_assertions::assert_eq!(refusal.error_type(), NgsiLdErrorType::BadRequestData);
        assert!(refusal.to_string().starts_with("request body is not admissible JSON:"), "{refusal}");
    }
}
