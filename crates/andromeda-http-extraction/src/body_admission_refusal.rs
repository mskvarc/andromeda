//! The refusals of a request body at the NGSI-LD body admission.

use crate::{body_envelope::BodyContextError, not_json_document::NotJsonDocument};
use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_number::json_value::text_admission::JsonTextRefusal;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use thiserror::Error;

/// Why a request body fails the NGSI-LD body admission once its headers passed.
#[derive(Debug, Error)]
pub(crate) enum BodyAdmissionRefusal {
    /// Clause 5.5.4: the body is not a JSON document.
    #[error(transparent)]
    NotJson(#[from] NotJsonDocument),
    /// Clause 5.5.4: the body is JSON whose text holds a number outside the
    /// supported range, a string or member name the broker cannot hold, a
    /// member named like the reader's own tokens, or an object naming one
    /// member twice (IETF RFC 8259 section 4). Never
    /// [`JsonTextRefusal::Syntax`], which is [`BodyAdmissionRefusal::NotJson`].
    #[error(transparent)]
    Inadmissible(JsonTextRefusal),
    /// Clause 5.5.4: the body is an empty array, which is neither an entity
    /// nor a batch.
    #[error("request body is an empty array, expected a JSON object")]
    EmptyArray,
    /// Clause 6.3.5: an `application/json` body carries an `@context` member,
    /// where only a `Link` header may name its context.
    #[error("an application/json request body must not contain @context")]
    ContextInJsonBody,
    /// Clause 6.3.5: a top-level `@context` is neither a string, an object nor
    /// an array of those.
    #[error("the @context member must be a string, an object, or an array of those")]
    MalformedContext,
    /// Clause 6.3.5: an `application/ld+json` body names no `@context`, or
    /// the members of an array name different ones.
    #[error(transparent)]
    BodyContext(#[from] BodyContextError),
}

impl From<JsonTextRefusal> for BodyAdmissionRefusal {
    fn from(refusal: JsonTextRefusal) -> BodyAdmissionRefusal {
        match refusal {
            JsonTextRefusal::Syntax(source) => BodyAdmissionRefusal::NotJson(NotJsonDocument(source)),
            inadmissible @ (JsonTextRefusal::OutOfRange(_)
            | JsonTextRefusal::Unholdable { .. }
            | JsonTextRefusal::ReservedMemberName { .. }
            | JsonTextRefusal::DuplicateMember { .. }) => BodyAdmissionRefusal::Inadmissible(inadmissible),
        }
    }
}

impl RequestRefusal for BodyAdmissionRefusal {
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            BodyAdmissionRefusal::NotJson(refusal) => refusal.error_type(),
            BodyAdmissionRefusal::Inadmissible(_)
            | BodyAdmissionRefusal::EmptyArray
            | BodyAdmissionRefusal::ContextInJsonBody
            | BodyAdmissionRefusal::MalformedContext
            | BodyAdmissionRefusal::BodyContext(_) => NgsiLdErrorType::BadRequestData,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{body_admission_refusal::BodyAdmissionRefusal, body_envelope::BodyContextError};
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_ngsi_ld_number::json_value::text_admission::{JsonTextRefusal, admit_json_text};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

    fn refused_text(text: &str) -> BodyAdmissionRefusal {
        BodyAdmissionRefusal::from(admit_json_text(text.as_bytes()).expect_err("a refused text"))
    }

    /// Clause 5.5.4: a text that is no JSON is `InvalidRequest`, worded as every
    /// other reader words it.
    #[test]
    fn a_text_that_is_no_json_is_an_invalid_request_in_the_shared_words() {
        let refusal = refused_text("{");
        pretty_assertions::assert_eq!(refusal.error_type(), NgsiLdErrorType::InvalidRequest);
        assert!(refusal.to_string().starts_with("request body is not a JSON document: "), "{refusal}");
    }

    /// Clause 5.5.4: a number the model cannot hold, a string the broker cannot
    /// hold, a reserved member name and a repeated member are valid JSON of
    /// unacceptable data.
    #[test]
    fn the_inadmissible_texts_are_bad_request_data_naming_their_location() {
        for (text, detail_start) in [
            (r#"{"v":1e-16384}"#, "the JSON number at /v is refused: "),
            (r#"{"v":"\u0000"}"#, "the string at /v is refused: "),
            (r#"{"$serde_json::private::Other":1}"#, "the member at /"),
            (r#"{"v":1,"v":2}"#, "the member at /v is named twice in its object"),
        ] {
            let refusal = refused_text(text);
            pretty_assertions::assert_eq!(refusal.error_type(), NgsiLdErrorType::BadRequestData, "{text}");
            assert!(
                matches!(&refusal, BodyAdmissionRefusal::Inadmissible(inner) if !matches!(inner, JsonTextRefusal::Syntax(_))),
                "{text}"
            );
            assert!(refusal.to_string().starts_with(detail_start), "{text}: {refusal}");
        }
    }

    /// Clauses 5.5.4 and 6.3.5: the remaining admission refusals are
    /// `BadRequestData` with a detail that starts lowercase.
    #[test]
    fn the_context_and_shape_refusals_are_bad_request_data_with_lowercase_details() {
        for (refusal, detail) in [
            (BodyAdmissionRefusal::EmptyArray, "request body is an empty array, expected a JSON object"),
            (
                BodyAdmissionRefusal::ContextInJsonBody,
                "an application/json request body must not contain @context",
            ),
            (
                BodyAdmissionRefusal::MalformedContext,
                "the @context member must be a string, an object, or an array of those",
            ),
            (
                BodyAdmissionRefusal::BodyContext(BodyContextError::Missing),
                "application/ld+json body must contain @context",
            ),
            (
                BodyAdmissionRefusal::BodyContext(BodyContextError::Divergent),
                "every member of an application/ld+json array body must carry the same @context",
            ),
        ] {
            pretty_assertions::assert_eq!(refusal.error_type(), NgsiLdErrorType::BadRequestData, "{refusal}");
            pretty_assertions::assert_eq!(refusal.to_string(), detail);
        }
    }
}
