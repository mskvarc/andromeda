//! The failure to decode a request's query string into a parameter set.

use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::query::deserialization::JOIN_LEVEL_NOT_AN_INTEGER;
use serde_urlencoded::de::Error as UrlencodedError;
use thiserror::Error;

/// Serde's report of a parameter the deserialized parameter set does not define.
const UNKNOWN_PARAMETER: &str = "unknown field";

/// A query string the parameter set could not decode.
#[derive(Debug, Error)]
#[error("invalid query parameters: {0}")]
pub struct QueryDecodeError(#[from] pub UrlencodedError);

impl RequestRefusal for QueryDecodeError {
    /// Two failures can reach here and clause 6.3.20 separates them. A
    /// parameter the operation does not define is one of the "parameters that
    /// are incompatible with the operation", answered with `InvalidRequest`;
    /// serde reports it through `deny_unknown_fields`, whose message is the
    /// only signal the crate offers and is stable across versions. A
    /// `joinLevel` that is no integer at all is a syntactically invalid request
    /// (Table 5.5.2-1), also `InvalidRequest`, reported through
    /// [`JOIN_LEVEL_NOT_AN_INTEGER`]. Everything else is a value inside a
    /// parameter the operation does define -- a `limit` that is not a number, a
    /// half-specified geoquery (clauses 4.10, 5.7.2.4, 5.10.2.4) -- which is
    /// input that does not meet the requirements of the operation, answered
    /// with `BadRequestData`.
    ///
    /// The two parameters whose *values* clause 6.3.20 also gives to
    /// `InvalidRequest`, `format` and `options`, never fail here: they
    /// deserialize as written and are refused by the parameter set's
    /// `validate`, where the error type is typed rather than read out of a
    /// message. See [`andromeda_ngsi_ld_request::representation_parameter`].
    fn error_type(&self) -> NgsiLdErrorType {
        let message = self.0.to_string();
        if message.contains(UNKNOWN_PARAMETER) || message.contains(JOIN_LEVEL_NOT_AN_INTEGER) {
            NgsiLdErrorType::InvalidRequest
        } else {
            NgsiLdErrorType::BadRequestData
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query_decode_error::QueryDecodeError;
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_request::query::deserialization::{GEOREL_WITHOUT_GEOMETRY, INCOMPLETE_GEOQUERY, JOIN_LEVEL_NOT_AN_INTEGER};
    use serde::de::Error as DecodeError;
    use serde_urlencoded::de::Error as UrlencodedError;

    /// Clauses 6.3.20 and 5.5.2: an undefined parameter and a `joinLevel` that
    /// is no integer are `InvalidRequest`; an incomplete geoquery is
    /// `BadRequestData`.
    #[test]
    fn the_message_of_a_decode_failure_selects_its_error_type() {
        for (message, expected) in [
            ("unknown field `datasetId`, expected `type`", NgsiLdErrorType::InvalidRequest),
            (JOIN_LEVEL_NOT_AN_INTEGER, NgsiLdErrorType::InvalidRequest),
            (INCOMPLETE_GEOQUERY, NgsiLdErrorType::BadRequestData),
            (GEOREL_WITHOUT_GEOMETRY, NgsiLdErrorType::BadRequestData),
            ("invalid digit found in string", NgsiLdErrorType::BadRequestData),
        ] {
            pretty_assertions::assert_eq!(QueryDecodeError(UrlencodedError::custom(message)).error_type(), expected, "{message}");
        }
    }

    #[test]
    fn the_detail_names_the_decoding_and_starts_lowercase() {
        pretty_assertions::assert_eq!(
            QueryDecodeError(UrlencodedError::custom("invalid digit found in string")).to_string(),
            "invalid query parameters: invalid digit found in string"
        );
    }
}
