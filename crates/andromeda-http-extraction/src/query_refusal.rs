//! The refusal of a query string the operation's parameter set does not admit.

use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_number::held_text::UnholdableCharacter;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::{query::deserialization::JOIN_LEVEL_NOT_AN_INTEGER, query_parameter::QueryParameter};
use serde_urlencoded::de::Error as UrlencodedError;
use thiserror::Error;

/// Why a query string does not decode into the parameter set of its operation.
#[derive(Debug, Error)]
pub enum QueryRefusal {
    /// A parameter name no NGSI-LD operation defines.
    #[error("the query names a parameter no NGSI-LD operation defines")]
    Undefined,
    /// A parameter another operation defines, but not this one.
    #[error("query parameter {0} is not defined for this operation")]
    NotDefinedHere(QueryParameter),
    /// The value of one parameter does not decode.
    #[error("invalid query parameter {parameter}: {source}")]
    Value {
        /// The parameter whose value is at fault.
        parameter: QueryParameter,
        /// Why the value does not decode.
        #[source]
        source: UrlencodedError,
    },
    /// The value of one parameter holds a character the broker cannot hold.
    #[error("invalid query parameter {parameter}: {source}")]
    Unholdable {
        /// The parameter whose value is at fault.
        parameter: QueryParameter,
        /// The character the broker cannot hold.
        #[source]
        source: UnholdableCharacter,
    },
    /// Parameters that decode alone but not together.
    #[error("invalid query parameters: {0}")]
    Combination(#[source] UrlencodedError),
}

impl RequestRefusal for QueryRefusal {
    /// Clause 6.3.20 answers a parameter the operation does not define with
    /// `InvalidRequest`, and Table 5.5.2-1 a `joinLevel` that is no integer at
    /// all, a syntactically invalid request. Every other failure is a value
    /// the operation defines but cannot use, `BadRequestData`. `format` and
    /// `options`, whose values clause 6.3.20 also gives to `InvalidRequest`,
    /// never fail here: they decode as written and are refused by the
    /// parameter set's `validate`.
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            QueryRefusal::Undefined | QueryRefusal::NotDefinedHere(_) => NgsiLdErrorType::InvalidRequest,
            QueryRefusal::Value {
                parameter: QueryParameter::JoinLevel,
                source,
            } if source.to_string() == JOIN_LEVEL_NOT_AN_INTEGER => NgsiLdErrorType::InvalidRequest,
            QueryRefusal::Value { .. } | QueryRefusal::Unholdable { .. } | QueryRefusal::Combination(_) => NgsiLdErrorType::BadRequestData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query_refusal::QueryRefusal;
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_ngsi_ld_number::held_text::UnholdableCharacter;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_request::{
        query::deserialization::{GEOREL_WITHOUT_GEOMETRY, JOIN_LEVEL_NOT_AN_INTEGER},
        query_parameter::QueryParameter,
    };
    use serde::de::Error as DecodeError;
    use serde_urlencoded::de::Error as UrlencodedError;

    /// Clauses 6.3.20 and 5.5.2: a parameter the operation does not define and
    /// a `joinLevel` that is no integer are `InvalidRequest`, a value the
    /// operation cannot use `BadRequestData`.
    #[test]
    fn each_refusal_carries_its_error_type() {
        for (refusal, expected) in [
            (QueryRefusal::Undefined, NgsiLdErrorType::InvalidRequest),
            (QueryRefusal::NotDefinedHere(QueryParameter::Limit), NgsiLdErrorType::InvalidRequest),
            (
                QueryRefusal::Value {
                    parameter: QueryParameter::JoinLevel,
                    source: UrlencodedError::custom(JOIN_LEVEL_NOT_AN_INTEGER),
                },
                NgsiLdErrorType::InvalidRequest,
            ),
            (
                QueryRefusal::Value {
                    parameter: QueryParameter::JoinLevel,
                    source: UrlencodedError::custom("joinLevel is not a positive integer: 0"),
                },
                NgsiLdErrorType::BadRequestData,
            ),
            (
                QueryRefusal::Unholdable {
                    parameter: QueryParameter::Q,
                    source: UnholdableCharacter,
                },
                NgsiLdErrorType::BadRequestData,
            ),
            (
                QueryRefusal::Combination(UrlencodedError::custom(GEOREL_WITHOUT_GEOMETRY)),
                NgsiLdErrorType::BadRequestData,
            ),
        ] {
            pretty_assertions::assert_eq!(refusal.error_type(), expected, "{refusal}");
        }
    }

    #[test]
    fn the_detail_names_the_parameter_and_starts_lowercase() {
        pretty_assertions::assert_eq!(
            QueryRefusal::NotDefinedHere(QueryParameter::Limit).to_string(),
            "query parameter limit is not defined for this operation"
        );
        pretty_assertions::assert_eq!(
            QueryRefusal::Value {
                parameter: QueryParameter::Count,
                source: UrlencodedError::custom("provided string was not `true` or `false`"),
            }
            .to_string(),
            "invalid query parameter count: provided string was not `true` or `false`"
        );
    }
}
